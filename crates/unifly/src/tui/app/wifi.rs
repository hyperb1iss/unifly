use std::sync::Arc;

use unifly_api::MacAddress;

use super::App;
use crate::tui::action::Action;

impl App {
    pub(super) fn fetch_wifi_neighbors(&self, within_secs: Option<i64>) {
        let Some(controller) = self.controller.clone() else {
            return;
        };
        let tx = self.action_tx.clone();
        let sanitizer = self.sanitizer.clone();

        tokio::spawn(async move {
            match controller.list_rogue_aps(within_secs).await {
                Ok(neighbors) => {
                    let clean = if let Some(ref san) = sanitizer {
                        san.sanitize_rogue_aps(&neighbors)
                    } else {
                        neighbors
                    };
                    let _ = tx.send(Action::WifiNeighborsUpdated(Arc::new(clean)));
                }
                Err(error) => {
                    let _ = tx.send(Action::Notify(crate::tui::action::Notification::error(
                        format!("WiFi neighbors: {error}"),
                    )));
                }
            }
        });
    }

    pub(super) fn fetch_wifi_channels(&self) {
        let Some(controller) = self.controller.clone() else {
            return;
        };
        let tx = self.action_tx.clone();

        tokio::spawn(async move {
            match controller.list_channels().await {
                Ok(channels) => {
                    let _ = tx.send(Action::WifiChannelsUpdated(Arc::new(channels)));
                }
                Err(error) => {
                    let _ = tx.send(Action::Notify(crate::tui::action::Notification::error(
                        format!("WiFi channels: {error}"),
                    )));
                }
            }
        });
    }

    pub(super) fn fetch_wifi_client_detail(&self, ip: &str) {
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
                    let _ = tx.send(Action::WifiClientDetailFailed {
                        ip,
                        error: format!("WiFi client detail: {error}"),
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
    pub(super) fn fetch_wifi_roam_history(&self, mac: &str, limit: Option<u32>) {
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
                    let _ = tx.send(Action::WifiRoamHistoryFailed {
                        mac,
                        error: format!("WiFi roam history: {error}"),
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
