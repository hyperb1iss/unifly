mod ssids;

use std::collections::HashSet;
use std::sync::Arc;

use color_eyre::eyre::Result;
use unifly_api::{EntityId, MacAddress};

use super::App;
use crate::tui::action::Action;
use crate::tui::screen::ScreenId;

/// What the App keeps for the WiFi screen's work on the controller.
#[derive(Default)]
pub(super) struct WifiState {
    /// Counts the controllers this session has had. A fetch carries the
    /// count it started under, and its answer is dropped once that count
    /// has moved on.
    generation: u64,
    /// SSIDs with a write in flight. Every write reads the record, changes
    /// it, and sends it whole, so two at once on one SSID would have the
    /// second undo the first.
    writes: HashSet<EntityId>,
}

impl App {
    /// The WiFi screen's fetches and their failures. Returns whether
    /// `action` was one of them.
    pub(super) fn handle_wifi_action(&mut self, action: &Action) -> Result<bool> {
        match action {
            Action::ControllerChanged => {
                self.wifi = WifiState {
                    generation: self.wifi.generation + 1,
                    writes: HashSet::new(),
                };
                return Ok(false);
            }
            Action::WifiAnswer { generation, answer } => {
                if *generation == self.wifi.generation {
                    self.process_action(answer)?;
                }
            }
            Action::RequestWifiNeighbors(within_secs) => self.fetch_wifi_neighbors(*within_secs),
            Action::RequestWifiChannels => self.fetch_wifi_channels(),
            Action::RequestWifiClientDetail(ip) => self.fetch_wifi_client_detail(ip),
            Action::RequestWifiRoamHistory { mac, limit } => {
                self.fetch_wifi_roam_history(mac, *limit);
            }
            Action::RequestWifiBroadcastDetail(id) => self.fetch_wifi_broadcast_detail(id),
            Action::RequestSsidVisibilityToggle(id) => self.read_ssid_visibility(id),
            Action::WifiSsidVisibilityRead { id, detail } => {
                self.confirm_ssid_visibility(id, detail)?;
            }
            Action::RequestSsidRename { id, typed } => {
                self.prompt_ssid_rename(id, typed.as_deref());
            }
            Action::RequestSsidRenameTo { id, name } => self.confirm_ssid_rename(id, name),
            Action::WifiSsidWrite { id, change, notice } => self.write_ssid(id, change, notice),
            Action::WifiSsidWriteSettled { id, .. } => self.ssid_write_settled(id, action)?,
            Action::RequestSsidPrompt(request) => self.open_ssid_prompt(request),
            Action::WifiNeighborsFailed(_)
            | Action::WifiChannelsFailed(_)
            | Action::WifiBroadcastDetailLoaded { .. }
            | Action::WifiBroadcastDetailFailed { .. } => {
                self.forward_to_screen(ScreenId::Wifi, action)?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// The way back for a WiFi fetch started under the current controller.
    /// The answer is checked when it is handled, after any controller
    /// change queued ahead of it, so nothing from the old controller lands
    /// on the cleared screen.
    fn wifi_reply(&self) -> impl Fn(Action) + Send + 'static {
        let tx = self.action_tx.clone();
        let generation = self.wifi.generation;
        move |answer| {
            let _ = tx.send(Action::WifiAnswer {
                generation,
                answer: Box::new(answer),
            });
        }
    }

    fn fetch_wifi_neighbors(&self, within_secs: Option<i64>) {
        let Some(controller) = self.controller.clone() else {
            return;
        };
        let reply = self.wifi_reply();
        let sanitizer = self.sanitizer.clone();

        tokio::spawn(async move {
            reply(match controller.list_rogue_aps(within_secs).await {
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
            });
        });
    }

    fn fetch_wifi_channels(&self) {
        let Some(controller) = self.controller.clone() else {
            return;
        };
        let reply = self.wifi_reply();
        let sanitizer = self.sanitizer.clone();

        tokio::spawn(async move {
            reply(match controller.list_channels().await {
                Ok(channels) => Action::WifiChannelsUpdated(Arc::new(channels)),
                Err(error) => Action::WifiChannelsFailed(sanitizer.as_ref().map_or_else(
                    || error.to_string(),
                    |san| san.sanitize_text(&error.to_string()),
                )),
            });
        });
    }

    fn fetch_wifi_client_detail(&self, ip: &str) {
        let Some(controller) = self.controller.clone() else {
            return;
        };
        let reply = self.wifi_reply();
        let real_ip = self.real_client_ip(ip);
        let ip = ip.to_owned();
        let sanitizer = self.sanitizer.clone();

        tokio::spawn(async move {
            reply(
                match controller.get_client_wifi_experience(&real_ip).await {
                    Ok(data) => {
                        let clean = if let Some(ref san) = sanitizer {
                            san.sanitize_json_value(&data)
                        } else {
                            data
                        };
                        Action::WifiClientDetailLoaded {
                            ip,
                            data: Arc::new(clean),
                        }
                    }
                    Err(error) => {
                        let error = error.to_string();
                        Action::WifiClientDetailFailed {
                            ip,
                            error: sanitizer
                                .as_ref()
                                .map_or_else(|| error.clone(), |san| san.sanitize_text(&error)),
                        }
                    }
                },
            );
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
        let reply = self.wifi_reply();
        let real_mac = self.real_client_mac(mac);
        let mac = mac.to_owned();
        let sanitizer = self.sanitizer.clone();

        tokio::spawn(async move {
            reply(match controller.get_client_roams(&real_mac, limit).await {
                Ok(events) => {
                    let clean = if let Some(ref san) = sanitizer {
                        events.iter().map(|e| san.sanitize_json_value(e)).collect()
                    } else {
                        events
                    };
                    Action::WifiRoamHistoryLoaded {
                        mac,
                        events: Arc::new(clean),
                    }
                }
                Err(error) => {
                    let error = error.to_string();
                    Action::WifiRoamHistoryFailed {
                        mac,
                        error: sanitizer
                            .as_ref()
                            .map_or_else(|| error.clone(), |san| san.sanitize_text(&error)),
                    }
                }
            });
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

    fn wifi_frame(app: &App) -> String {
        let screen = app.screens.get(&ScreenId::Wifi).expect("the WiFi screen");
        crate::tui::table::render_lines(120, 20, |frame, area| screen.render(frame, area))
            .join("\n")
    }

    fn heard(essid: &str) -> Action {
        let neighbor = serde_json::from_value(serde_json::json!({
            "bssid": "11:22:33:44:55:66", "essid": essid, "channel": 6,
            "signal": -60, "radio": "ng"
        }))
        .expect("neighbor fixture should deserialize");
        Action::WifiNeighborsUpdated(Arc::new(vec![neighbor]))
    }

    #[test]
    fn an_answer_from_the_previous_controller_never_reaches_the_cleared_screen() {
        use crate::tui::action::{Access, WifiSubTab};

        let mut app = App::new(None, None, false, true);
        let access = Action::AccessUpdated(Access {
            integration: true,
            session: true,
            live_events: true,
        });
        app.process_action(&access).expect("access");
        app.process_action(&Action::WifiSubTab(WifiSubTab::Neighbors))
            .expect("sub-tab");
        app.forward_to_screen(ScreenId::Wifi, &Action::WifiSubTab(WifiSubTab::Neighbors))
            .expect("sub-tab");
        let answer = |essid: &str, generation: u64| Action::WifiAnswer {
            generation,
            answer: Box::new(heard(essid)),
        };

        app.process_action(&answer("old-site-cafe", app.wifi.generation))
            .expect("a current answer");
        assert!(wifi_frame(&app).contains("old-site-cafe"));

        // The switch is queued ahead of whatever the old controller still
        // has in flight, so it is handled first.
        let old = app.wifi.generation;
        app.process_action(&Action::ControllerChanged)
            .expect("switch");
        app.process_action(&access).expect("the next controller");
        app.process_action(&answer("old-site-late", old))
            .expect("a late answer");
        let frame = wifi_frame(&app);
        assert!(!frame.contains("old-site"), "{frame}");

        app.process_action(&answer("new-site-cafe", app.wifi.generation))
            .expect("the next controller answers");
        assert!(wifi_frame(&app).contains("new-site-cafe"));
    }

    #[tokio::test]
    async fn a_fetch_reports_back_tagged_with_its_controller() {
        let controller = unifly_api::Controller::demo(unifly_api::DemoFleetOptions::default());
        controller.connect().await.expect("demo");
        let mut app = App::new(Some(controller.clone()), None, false, true);
        app.process_action(&Action::ControllerChanged)
            .expect("switch");

        app.process_action(&Action::RequestWifiChannels)
            .expect("request");
        let answer = tokio::time::timeout(std::time::Duration::from_secs(10), app.action_rx.recv())
            .await
            .expect("the fetch reports back")
            .expect("the action channel stays open");
        assert!(
            matches!(&answer, Action::WifiAnswer { generation, answer }
                if *generation == app.wifi.generation
                    && matches!(**answer, Action::WifiChannelsUpdated(_))),
            "{answer:?}"
        );
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
