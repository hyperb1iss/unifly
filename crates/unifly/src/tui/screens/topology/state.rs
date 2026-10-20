use std::sync::Arc;

use super::TopologyScreen;
use crate::tui::action::Action;

impl TopologyScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            devices: Arc::new(Vec::new()),
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
            hits: std::cell::RefCell::default(),
        }
    }

    pub(super) fn apply_action(&mut self, action: &Action) {
        if let Action::DevicesUpdated(devices) = action {
            self.devices = Arc::clone(devices);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::TopologyScreen;

    use serde_json::json;
    use unifly_api::{Device, DeviceType};

    use crate::tui::action::Action;

    fn test_device(id: &str, name: &str, device_type: DeviceType) -> Arc<Device> {
        let device_type_name = match device_type {
            DeviceType::Gateway => "Gateway",
            DeviceType::Switch => "Switch",
            DeviceType::AccessPoint => "AccessPoint",
            _ => "Other",
        };

        Arc::new(
            serde_json::from_value(json!({
                "id": id,
                "mac": format!("aa:bb:cc:dd:ee:{:0>2}", id.chars().last().unwrap_or('0')),
                "ip": "192.168.1.10",
                "wan_ipv6": null,
                "name": name,
                "model": "UniFi",
                "device_type": device_type_name,
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
                "has_switching": device_type == DeviceType::Switch || device_type == DeviceType::Gateway,
                "has_access_point": device_type == DeviceType::AccessPoint,
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
                "client_count": 3,
                "origin": null
            }))
            .expect("device fixture should deserialize"),
        )
    }

    #[test]
    fn devices_update_replaces_visible_topology_devices() {
        let mut screen = TopologyScreen::new();
        let devices = Arc::new(vec![
            test_device("gateway-1", "Gateway", DeviceType::Gateway),
            test_device("switch-1", "Switch", DeviceType::Switch),
        ]);

        screen.apply_action(&Action::DevicesUpdated(Arc::clone(&devices)));

        assert_eq!(screen.devices.len(), 2);
        assert_eq!(screen.devices[0].name.as_deref(), Some("Gateway"));
        assert_eq!(screen.devices[1].name.as_deref(), Some("Switch"));
    }
}
