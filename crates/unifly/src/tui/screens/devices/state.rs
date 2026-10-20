use std::sync::Arc;

use super::DevicesScreen;

use ratatui::widgets::TableState;
use unifly_api::{Client, Device, EntityId};

use crate::cli::commands::util::order;
use crate::tui::table::TableScroll;

impl DevicesScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            action_tx: None,
            devices: Arc::new(Vec::new()),
            clients: Arc::new(Vec::new()),
            table_state: TableState::default(),
            table_scroll: TableScroll::default(),
            selected_device_id: None,
            detail_open: false,
            detail_device_id: None,
            detail_tab: crate::tui::action::DeviceDetailTab::default(),
            hits: std::cell::RefCell::default(),
            search_query: String::new(),
        }
    }

    /// Devices shown in the table: the name-ordered list, narrowed by the
    /// search query.
    pub(super) fn filtered_devices(&self) -> Vec<&Arc<Device>> {
        if self.search_query.is_empty() {
            self.devices.iter().collect()
        } else {
            let query = self.search_query.to_lowercase();
            self.devices
                .iter()
                .filter(|device| {
                    device
                        .name
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&query)
                        || device
                            .model
                            .as_deref()
                            .unwrap_or("")
                            .to_lowercase()
                            .contains(&query)
                        || device
                            .ip
                            .map(|ip| ip.to_string())
                            .unwrap_or_default()
                            .contains(&query)
                        || device.mac.to_string().contains(&query)
                })
                .collect()
        }
    }

    pub(super) fn selected_index(&self) -> usize {
        self.table_state.selected().unwrap_or(0)
    }

    pub(super) fn selected_row_index(&self, filtered: &[&Arc<Device>]) -> Option<usize> {
        if filtered.is_empty() {
            return None;
        }

        if let Some(selected_id) = self.selected_device_id.as_ref() {
            return filtered
                .iter()
                .position(|device| &device.id == selected_id)
                .or(Some(0));
        }

        self.table_state
            .selected()
            .filter(|index| *index < filtered.len())
            .or(Some(0))
    }

    pub(super) fn selected_device(&self) -> Option<&Arc<Device>> {
        let filtered = self.filtered_devices();
        let index = self.selected_row_index(&filtered)?;
        filtered.get(index).copied()
    }

    pub(super) fn detail_device(&self) -> Option<&Arc<Device>> {
        self.detail_device_id
            .as_ref()
            .and_then(|detail_id| self.devices.iter().find(|device| device.id == *detail_id))
            .or_else(|| self.selected_device())
    }

    pub(super) fn select(&mut self, idx: usize) {
        let next_selection = {
            let filtered = self.filtered_devices();
            if filtered.is_empty() {
                None
            } else {
                let clamped = idx.min(filtered.len() - 1);
                Some((clamped, filtered[clamped].id.clone()))
            }
        };

        if let Some((index, device_id)) = next_selection {
            self.table_state.select(Some(index));
            self.selected_device_id = Some(device_id);
        } else {
            self.table_state.select(None);
            self.selected_device_id = None;
        }
    }

    #[allow(clippy::cast_sign_loss, clippy::as_conversions)]
    pub(super) fn move_selection(&mut self, delta: isize) {
        let len = self.filtered_devices().len();
        if len == 0 {
            self.table_state.select(None);
            self.selected_device_id = None;
            return;
        }

        #[allow(clippy::cast_possible_wrap)]
        let current = self.selected_index() as isize;
        #[allow(clippy::cast_possible_wrap)]
        let next = (current + delta).clamp(0, len as isize - 1);
        self.select(next as usize);
    }

    pub(super) fn select_last(&mut self) {
        let len = self.filtered_devices().len();
        if len > 0 {
            self.select(len - 1);
        }
    }

    pub(super) fn open_selected_detail(&mut self) -> Option<EntityId> {
        let detail_id = self.selected_device().map(|device| device.id.clone())?;
        self.detail_open = true;
        self.detail_tab = crate::tui::action::DeviceDetailTab::Overview;
        self.detail_device_id = Some(detail_id.clone());
        Some(detail_id)
    }

    pub(super) fn close_detail(&mut self) {
        self.detail_open = false;
        self.detail_device_id = None;
    }

    pub(super) fn current_action_device_id(&self) -> Option<EntityId> {
        if self.detail_open {
            self.detail_device_id.clone()
        } else {
            self.selected_device_id.clone()
        }
    }

    pub(super) fn device_clients(&self, device: &Device) -> Vec<&Arc<Client>> {
        let device_mac = device.mac.to_string().to_lowercase();
        self.clients
            .iter()
            .filter(|c| {
                c.uplink_device_mac
                    .as_ref()
                    .is_some_and(|mac| mac.to_string().to_lowercase() == device_mac)
            })
            .collect()
    }

    pub(super) fn apply_devices_update(&mut self, devices: &[Arc<Device>]) {
        self.devices = Arc::new(order::devices(devices));

        if self
            .detail_device_id
            .as_ref()
            .is_some_and(|detail_id| !self.devices.iter().any(|device| device.id == *detail_id))
        {
            self.close_detail();
        }

        self.sync_table_selection();
    }

    pub(super) fn set_search_query(&mut self, query: &str) {
        self.search_query.clear();
        self.search_query.push_str(query);
        self.sync_table_selection();
    }

    pub(super) fn clear_search(&mut self) {
        self.search_query.clear();
        self.sync_table_selection();
    }

    fn sync_table_selection(&mut self) {
        let next_selection = {
            let filtered = self.filtered_devices();
            self.selected_row_index(&filtered)
                .map(|index| (index, filtered[index].id.clone()))
        };

        if let Some((index, device_id)) = next_selection {
            self.table_state.select(Some(index));
            self.selected_device_id = Some(device_id);
        } else {
            self.table_state.select(None);
            self.selected_device_id = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::DevicesScreen;

    use serde_json::json;
    use unifly_api::Device;

    fn test_device(id: &str, name: &str) -> Arc<Device> {
        Arc::new(
            serde_json::from_value(json!({
                "id": id,
                "mac": format!("aa:bb:cc:dd:ee:{:0>2}", id.chars().last().unwrap_or('0')),
                "ip": null,
                "wan_ipv6": null,
                "name": name,
                "model": "U7 Pro",
                "device_type": "AccessPoint",
                "state": "Online",
                "firmware_version": null,
                "firmware_updatable": false,
                "adopted_at": null,
                "provisioned_at": null,
                "last_seen": null,
                "serial": null,
                "supported": true,
                "ports": [],
                "radios": [],
                "uplink_device_id": null,
                "uplink_device_mac": null,
                "has_switching": false,
                "has_access_point": true,
                "stats": {
                    "uptime_secs": null,
                    "cpu_utilization_pct": null,
                    "memory_utilization_pct": null,
                    "load_average_1m": null,
                    "load_average_5m": null,
                    "load_average_15m": null,
                    "uplink_bandwidth": null,
                    "last_heartbeat": null,
                    "next_heartbeat": null
                },
                "client_count": null,
                "origin": null
            }))
            .expect("device fixture should deserialize"),
        )
    }

    fn apply_devices(screen: &mut DevicesScreen, devices: &[Arc<Device>]) {
        screen.apply_devices_update(devices);
    }

    #[test]
    fn filtered_devices_are_sorted_by_name() {
        let mut screen = DevicesScreen::new();
        apply_devices(
            &mut screen,
            &[
                test_device("device-b", "Zulu AP"),
                test_device("device-a", "alpha AP"),
            ],
        );

        let names: Vec<_> = screen
            .filtered_devices()
            .into_iter()
            .map(|device| device.name.as_deref().unwrap_or_default().to_string())
            .collect();

        assert_eq!(names, vec!["alpha AP".to_string(), "Zulu AP".to_string()]);
    }

    #[test]
    fn selection_tracks_device_when_sort_position_changes() {
        let mut screen = DevicesScreen::new();
        apply_devices(
            &mut screen,
            &[
                test_device("device-a", "Alpha AP"),
                test_device("device-b", "Zulu AP"),
            ],
        );
        screen.select(1);

        apply_devices(
            &mut screen,
            &[
                test_device("device-a", "Zulu AP"),
                test_device("device-b", "Aaron AP"),
            ],
        );

        assert_eq!(
            screen
                .selected_device()
                .and_then(|device| device.name.clone()),
            Some("Aaron AP".to_string())
        );
        assert_eq!(screen.table_state.selected(), Some(0));
    }

    #[test]
    fn detail_target_survives_filter_changes() {
        let mut screen = DevicesScreen::new();
        apply_devices(
            &mut screen,
            &[
                test_device("device-a", "Alpha AP"),
                test_device("device-b", "Beta AP"),
            ],
        );
        screen.select(1);
        let detail_id = screen.open_selected_detail();
        screen.set_search_query("alpha");

        assert_eq!(detail_id, screen.detail_device_id.clone());
        assert_eq!(
            screen
                .detail_device()
                .and_then(|device| device.name.clone()),
            Some("Beta AP".to_string())
        );
        assert_eq!(
            screen
                .selected_device()
                .and_then(|device| device.name.clone()),
            Some("Alpha AP".to_string())
        );
    }
}
