use std::sync::Arc;

use color_eyre::eyre::Result;
use unifly_api::MacAddress;

use super::App;
use crate::tui::action::Action;
use crate::tui::screen::ScreenId;

impl App {
    /// The WiFi screen's fetches and their failures. Returns whether
    /// `action` was one of them.
    pub(super) fn handle_wifi_action(&mut self, action: &Action) -> Result<bool> {
        match action {
            Action::RequestWifiNeighbors(within_secs) => self.fetch_wifi_neighbors(*within_secs),
            Action::RequestWifiChannels => self.fetch_wifi_channels(),
            Action::RequestWifiClientDetail(ip) => self.fetch_wifi_client_detail(ip),
            Action::RequestWifiRoamHistory { mac, limit } => {
                self.fetch_wifi_roam_history(mac, *limit);
            }
            Action::WifiNeighborsFailed(_) | Action::WifiChannelsFailed(_) => {
                self.forward_to_screen(ScreenId::Wifi, action)?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn fetch_wifi_neighbors(&self, within_secs: Option<i64>) {
        let Some(controller) = self.controller.clone() else {
            return;
        };
        let tx = self.action_tx.clone();
        let sanitizer = self.sanitizer.clone();

        tokio::spawn(async move {
            let action = match controller.list_rogue_aps(within_secs).await {
                Ok(neighbors) => {
                    let clean = if let Some(ref san) = sanitizer {
                        san.sanitize_rogue_aps(&neighbors)
                    } else {
                        neighbors
                    };
                    Action::WifiNeighborsUpdated(Arc::new(clean))
                }
                Err(error) => Action::WifiNeighborsFailed(sanitizer.as_ref().map_or_else(
                    || error.to_string(),
                    |san| san.sanitize_text(&error.to_string()),
                )),
            };
            let _ = tx.send(action);
        });
    }

    fn fetch_wifi_channels(&self) {
        let Some(controller) = self.controller.clone() else {
            return;
        };
        let tx = self.action_tx.clone();
        let sanitizer = self.sanitizer.clone();

        tokio::spawn(async move {
            let action = match controller.list_channels().await {
                Ok(channels) => Action::WifiChannelsUpdated(Arc::new(channels)),
                Err(error) => Action::WifiChannelsFailed(sanitizer.as_ref().map_or_else(
                    || error.to_string(),
                    |san| san.sanitize_text(&error.to_string()),
                )),
            };
            let _ = tx.send(action);
        });
    }

    fn fetch_wifi_client_detail(&self, ip: &str) {
        let Some(controller) = self.controller.clone() else {
            return;
        };
        let tx = self.action_tx.clone();
        let real_ip = self.real_client_ip(ip);
        let ip = ip.to_owned();
        let sanitizer = self.sanitizer.clone();

        tokio::spawn(async move {
            match controller.get_client_wifi_experience(&real_ip).await {
                Ok(data) => {
                    let clean = if let Some(ref san) = sanitizer {
                        san.sanitize_json_value(&data)
                    } else {
                        data
                    };
                    let _ = tx.send(Action::WifiClientDetailLoaded {
                        ip,
                        data: Arc::new(clean),
                    });
                }
                Err(error) => {
                    let error = error.to_string();
                    let _ = tx.send(Action::WifiClientDetailFailed {
                        ip,
                        error: sanitizer
                            .as_ref()
                            .map_or_else(|| error.clone(), |san| san.sanitize_text(&error)),
                    });
                }
            }
        });
    }

    /// Resolve a displayed address back to the controller's client identity.
    fn real_client_ip(&self, shown: &str) -> String {
        let (Some(sanitizer), Some(controller)) = (&self.sanitizer, &self.controller) else {
            return shown.to_owned();
        };
        resolve_client_ip(sanitizer, shown, &controller.store().clients_snapshot())
    }

    /// The real MAC of the client a screen knows as `shown`. Under
    /// `--demo` with MAC redaction the screens only hold stand-ins, which
    /// the controller has never seen.
    fn real_client_mac(&self, shown: &str) -> String {
        let (Some(sanitizer), Some(controller)) = (&self.sanitizer, &self.controller) else {
            return shown.to_owned();
        };
        let shown_mac = MacAddress::new(shown);
        controller
            .store()
            .clients_snapshot()
            .iter()
            .find(|client| sanitizer.sanitize_mac(&client.mac) == shown_mac)
            .map_or_else(|| shown.to_owned(), |client| client.mac.to_string())
    }

    /// Fetch a client's roam history from the controller by its real MAC,
    /// and report back under `mac`, the MAC the screen asked with.
    fn fetch_wifi_roam_history(&self, mac: &str, limit: Option<u32>) {
        let Some(controller) = self.controller.clone() else {
            return;
        };
        let tx = self.action_tx.clone();
        let real_mac = self.real_client_mac(mac);
        let mac = mac.to_owned();
        let sanitizer = self.sanitizer.clone();

        tokio::spawn(async move {
            match controller.get_client_roams(&real_mac, limit).await {
                Ok(events) => {
                    let clean = if let Some(ref san) = sanitizer {
                        events.iter().map(|e| san.sanitize_json_value(e)).collect()
                    } else {
                        events
                    };
                    let _ = tx.send(Action::WifiRoamHistoryLoaded {
                        mac,
                        events: Arc::new(clean),
                    });
                }
                Err(error) => {
                    let error = error.to_string();
                    let _ = tx.send(Action::WifiRoamHistoryFailed {
                        mac,
                        error: sanitizer
                            .as_ref()
                            .map_or_else(|| error.clone(), |san| san.sanitize_text(&error)),
                    });
                }
            }
        });
    }
}

fn resolve_client_ip(
    sanitizer: &crate::sanitizer::Sanitizer,
    shown: &str,
    clients: &[Arc<unifly_api::Client>],
) -> String {
    let Ok(shown_ip) = shown.parse::<std::net::IpAddr>() else {
        return shown.to_owned();
    };
    clients
        .iter()
        .find(|client| sanitizer.sanitize_ip_opt(&client.ip) == Some(shown_ip))
        .and_then(|client| client.ip)
        .map_or_else(|| shown.to_owned(), |ip| ip.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entering_the_wifi_screen_fetches_nothing_by_itself() {
        let mut app = App::new(None, None, false, true);
        app.active_screen = ScreenId::Dashboard;

        app.switch_screen(ScreenId::Wifi).expect("screen switch");

        // The screen asks for what it shows, once it knows what the
        // controller can reach.
        assert!(app.action_rx.try_recv().is_err());
    }

    #[test]
    fn a_failed_wifi_fetch_goes_to_the_screen_and_raises_no_toast() {
        let mut app = App::new(None, None, false, true);
        for failed in [
            Action::WifiNeighborsFailed("HTTP 503".into()),
            Action::WifiChannelsFailed("HTTP 503".into()),
            Action::WifiClientDetailFailed {
                ip: "10.40.10.1".into(),
                error: "HTTP 503".into(),
            },
            Action::WifiRoamHistoryFailed {
                mac: "cc:00:00:00:00:01".into(),
                error: "HTTP 503".into(),
            },
        ] {
            app.process_action(&failed).expect("failure is handled");
            assert!(app.notices.latest().is_none(), "{failed:?} toasted");
            assert!(app.action_rx.try_recv().is_err(), "{failed:?} queued work");
        }
    }

    #[tokio::test]
    async fn an_ssid_key_runs_through_the_app_and_changes_the_controller() {
        use crossterm::event::{KeyCode, KeyEvent};

        let controller = unifly_api::Controller::demo(unifly_api::DemoFleetOptions::default());
        controller.connect().await.expect("demo");
        let mut app = App::new(Some(controller.clone()), None, false, true);
        app.active_screen = ScreenId::Wifi;
        app.process_action(&Action::WifiBroadcastsUpdated(
            controller.wifi_broadcasts_snapshot(),
        ))
        .expect("SSIDs reach the screen");
        app.process_action(&Action::WifiSubTab(crate::tui::action::WifiSubTab::Ssids))
            .expect("sub-tab");

        // The first SSID in name order is enabled, so `E` asks first.
        for key in ['E', 'y'] {
            let pressed = app
                .handle_key_event(KeyEvent::from(KeyCode::Char(key)))
                .expect("key")
                .expect("the key does something");
            app.process_action(&pressed).expect("key action");
            while let Ok(queued) = app.action_rx.try_recv() {
                app.process_action(&queued).expect("queued action");
            }
        }
        let finished =
            tokio::time::timeout(std::time::Duration::from_secs(10), app.action_rx.recv())
                .await
                .expect("the command reports back")
                .expect("the action channel stays open");
        app.process_action(&finished).expect("result");

        let disabled: Vec<_> = controller
            .wifi_broadcasts_snapshot()
            .iter()
            .filter(|ssid| !ssid.enabled)
            .map(|ssid| ssid.name.clone())
            .collect();
        assert_eq!(disabled.len(), 1, "{disabled:?}");
        let toast = app.notices.latest().expect("a toast reports the result");
        assert_eq!(toast.message, format!("Disabled {}", disabled[0]));
        controller.disconnect().await;
    }

    #[tokio::test]
    async fn detail_lookup_uses_real_ip_while_display_keeps_stand_in() {
        let controller = unifly_api::Controller::demo(unifly_api::DemoFleetOptions::default());
        controller.connect().await.expect("demo");
        let mut client = (**controller
            .store()
            .clients_snapshot()
            .first()
            .expect("client"))
        .clone();
        controller.disconnect().await;
        client.ip = Some("8.8.4.4".parse().expect("public address"));
        let sanitizer = crate::sanitizer::Sanitizer::new(&crate::config::DemoConfig::default());
        let shown = sanitizer
            .sanitize_ip_opt(&client.ip)
            .expect("stand-in")
            .to_string();
        assert_ne!(shown, "8.8.4.4");
        assert_eq!(
            resolve_client_ip(&sanitizer, &shown, &[Arc::new(client)]),
            "8.8.4.4"
        );
        assert_eq!(resolve_client_ip(&sanitizer, &shown, &[]), shown);
    }
}
