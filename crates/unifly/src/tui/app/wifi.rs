use std::sync::Arc;

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
        let ip = ip.to_owned();
        let sanitizer = self.sanitizer.clone();

        tokio::spawn(async move {
            match controller.get_client_wifi_experience(&ip).await {
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

    pub(super) fn fetch_wifi_roam_history(&self, mac: &str, limit: Option<u32>) {
        let Some(controller) = self.controller.clone() else {
            return;
        };
        let tx = self.action_tx.clone();
        let mac = mac.to_owned();
        let sanitizer = self.sanitizer.clone();

        tokio::spawn(async move {
            match controller.get_client_roams(&mac, limit).await {
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
                    let _ = tx.send(Action::Notify(crate::tui::action::Notification::error(
                        format!("WiFi roam history: {error}"),
                    )));
                }
            }
        });
    }
}
