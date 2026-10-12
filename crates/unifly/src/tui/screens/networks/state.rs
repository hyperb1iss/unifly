use std::sync::Arc;

use super::NetworksScreen;

use ratatui::widgets::TableState;

use unifly_api::{Network, UpdateNetworkRequest};

use crate::cli::commands::util::order;
use crate::tui::action::Action;
use crate::tui::table::reselect;

/// Editable fields for a network. Initialized from the selected `Network`.
#[allow(clippy::struct_excessive_bools)]
pub(super) struct NetworkEditState {
    name: String,
    vlan_id: String,
    dhcp_enabled: bool,
    isolation_enabled: bool,
    internet_access_enabled: bool,
    mdns_forwarding_enabled: bool,
    ipv6_enabled: bool,
    enabled: bool,
    /// Which field is currently focused (0-indexed).
    pub(super) field_idx: usize,
}

impl NetworkEditState {
    pub(super) const FIELD_COUNT: usize = 8;

    pub(super) fn from_network(network: &Network) -> Self {
        Self {
            name: network.name.clone(),
            vlan_id: network
                .vlan_id
                .map_or_else(String::new, |value| value.to_string()),
            dhcp_enabled: network.dhcp.as_ref().is_some_and(|dhcp| dhcp.enabled),
            isolation_enabled: network.isolation_enabled,
            internet_access_enabled: network.internet_access_enabled,
            mdns_forwarding_enabled: network.mdns_forwarding_enabled,
            ipv6_enabled: network.ipv6_enabled,
            enabled: network.enabled,
            field_idx: 0,
        }
    }

    pub(super) fn field_label(index: usize) -> &'static str {
        match index {
            0 => "Name",
            1 => "VLAN ID",
            2 => "Enabled",
            3 => "DHCP",
            4 => "Isolation",
            5 => "Internet",
            6 => "mDNS Fwd",
            7 => "IPv6",
            _ => "",
        }
    }

    pub(super) fn field_value(&self, index: usize) -> String {
        match index {
            0 => self.name.clone(),
            1 => self.vlan_id.clone(),
            2 => bool_label(self.enabled),
            3 => bool_label(self.dhcp_enabled),
            4 => bool_label(self.isolation_enabled),
            5 => bool_label(self.internet_access_enabled),
            6 => bool_label(self.mdns_forwarding_enabled),
            7 => bool_label(self.ipv6_enabled),
            _ => String::new(),
        }
    }

    pub(super) fn is_bool_field(index: usize) -> bool {
        index >= 2
    }

    pub(super) fn toggle_bool(&mut self) {
        match self.field_idx {
            2 => self.enabled = !self.enabled,
            3 => self.dhcp_enabled = !self.dhcp_enabled,
            4 => self.isolation_enabled = !self.isolation_enabled,
            5 => self.internet_access_enabled = !self.internet_access_enabled,
            6 => self.mdns_forwarding_enabled = !self.mdns_forwarding_enabled,
            7 => self.ipv6_enabled = !self.ipv6_enabled,
            _ => {}
        }
    }

    pub(super) fn handle_text_input(&mut self, ch: char) {
        match self.field_idx {
            0 => self.name.push(ch),
            1 if ch.is_ascii_digit() => self.vlan_id.push(ch),
            _ => {}
        }
    }

    pub(super) fn handle_backspace(&mut self) {
        match self.field_idx {
            0 => {
                self.name.pop();
            }
            1 => {
                self.vlan_id.pop();
            }
            _ => {}
        }
    }

    pub(super) fn build_request(&self) -> UpdateNetworkRequest {
        UpdateNetworkRequest {
            name: Some(self.name.clone()),
            vlan_id: self.vlan_id.parse().ok(),
            enabled: Some(self.enabled),
            dhcp_enabled: Some(self.dhcp_enabled),
            isolation_enabled: Some(self.isolation_enabled),
            internet_access_enabled: Some(self.internet_access_enabled),
            mdns_forwarding_enabled: Some(self.mdns_forwarding_enabled),
            ipv6_enabled: Some(self.ipv6_enabled),
            subnet: None,
            dhcp: None,
        }
    }
}

fn bool_label(value: bool) -> String {
    if value {
        "Enabled".into()
    } else {
        "Disabled".into()
    }
}

pub(super) fn format_lease_time(secs: u64) -> String {
    if secs >= 86400 && secs.is_multiple_of(86400) {
        format!("{}d", secs / 86400)
    } else if secs >= 3600 && secs.is_multiple_of(3600) {
        format!("{}h", secs / 3600)
    } else if secs >= 60 && secs.is_multiple_of(60) {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

impl NetworksScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            networks: Arc::new(Vec::new()),
            table_state: TableState::default(),
            detail_open: false,
            edit_state: None,
            action_tx: None,
        }
    }

    pub(super) fn selected_index(&self) -> usize {
        self.table_state.selected().unwrap_or(0)
    }

    pub(super) fn select(&mut self, index: usize) {
        let clamped = if self.networks.is_empty() {
            0
        } else {
            index.min(self.networks.len() - 1)
        };
        self.table_state.select(Some(clamped));
    }

    #[allow(clippy::cast_sign_loss, clippy::as_conversions)]
    pub(super) fn move_selection(&mut self, delta: isize) {
        if self.networks.is_empty() {
            return;
        }

        #[allow(clippy::cast_possible_wrap)]
        let current = self.selected_index() as isize;
        #[allow(clippy::cast_possible_wrap)]
        let next = (current + delta).clamp(0, self.networks.len() as isize - 1);
        self.select(next as usize);
    }

    pub(super) fn selected_network(&self) -> Option<&Arc<Network>> {
        self.networks.get(self.selected_index())
    }

    pub(super) fn apply_action(&mut self, action: &Action) {
        if let Action::NetworksUpdated(networks) = action {
            self.apply_networks_update(networks);
        }
    }

    /// Store a refreshed network list in table order, keeping the selection
    /// on the network it was on.
    fn apply_networks_update(&mut self, networks: &[Arc<Network>]) {
        let selected = self.selected_network().map(|network| network.id.clone());
        let previous = self.table_state.selected();
        self.networks = Arc::new(order::networks(networks));
        let next = reselect(
            self.networks.iter().map(|network| &network.id),
            selected.as_ref(),
            previous,
        );
        self.table_state.select(next.or(previous));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{NetworkEditState, NetworksScreen, format_lease_time};

    use serde_json::json;
    use unifly_api::{EntityId, Network};
    use uuid::Uuid;

    use crate::tui::action::Action;

    fn test_network() -> Network {
        serde_json::from_value(json!({
            "id": "11111111-1111-1111-1111-111111111111",
            "name": "Primary LAN",
            "vlan_id": 100,
            "enabled": true,
            "management": "Gateway",
            "purpose": "Corporate",
            "is_default": false,
            "subnet": "192.168.100.0/24",
            "gateway_ip": "192.168.100.1",
            "isolation_enabled": false,
            "internet_access_enabled": true,
            "mdns_forwarding_enabled": false,
            "ipv6_enabled": true,
            "ipv6_mode": "Static",
            "ipv6_prefix": null,
            "dhcpv6_enabled": false,
            "slaac_enabled": false,
            "ntp_server": null,
            "pxe_enabled": false,
            "tftp_server": null,
            "firewall_zone_id": null,
            "cellular_backup_enabled": false,
            "origin": null,
            "dhcp": {
                "enabled": true,
                "range_start": "192.168.100.10",
                "range_stop": "192.168.100.250",
                "lease_time_secs": 3600,
                "dns_servers": ["1.1.1.1"],
                "gateway": "192.168.100.1"
            }
        }))
        .expect("network fixture should deserialize")
    }

    #[test]
    fn edit_state_build_request_tracks_toggles() {
        let mut edit = NetworkEditState::from_network(&test_network());
        edit.field_idx = 2;
        edit.toggle_bool();

        let request = edit.build_request();

        assert_eq!(request.name.as_deref(), Some("Primary LAN"));
        assert_eq!(request.vlan_id, Some(100));
        assert_eq!(request.enabled, Some(false));
        assert_eq!(request.dhcp_enabled, Some(true));
        assert_eq!(request.ipv6_enabled, Some(true));
    }

    #[test]
    fn networks_list_by_name_and_selection_follows_the_network() {
        let network = |id: u128, name: &str, vlan: u16| -> Arc<Network> {
            let mut network = test_network();
            network.id = EntityId::Uuid(Uuid::from_u128(id));
            network.name = name.to_owned();
            network.vlan_id = Some(vlan);
            Arc::new(network)
        };
        let names = |screen: &NetworksScreen| -> Vec<String> {
            screen.networks.iter().map(|n| n.name.clone()).collect()
        };

        let mut screen = NetworksScreen::new();
        screen.apply_action(&Action::NetworksUpdated(Arc::new(vec![
            network(3, "Studio", 10),
            network(1, "guest", 30),
            network(2, "IoT", 40),
        ])));
        assert_eq!(names(&screen), vec!["guest", "IoT", "Studio"]);

        screen.select(1);
        screen.apply_action(&Action::NetworksUpdated(Arc::new(vec![
            network(2, "IoT", 40),
            network(4, "Cameras", 50),
            network(3, "Studio", 10),
            network(1, "guest", 30),
        ])));
        assert_eq!(names(&screen), vec!["Cameras", "guest", "IoT", "Studio"]);
        assert_eq!(
            screen.selected_network().map(|n| n.name.as_str()),
            Some("IoT")
        );
    }

    #[test]
    fn lease_time_formats_common_units() {
        assert_eq!(format_lease_time(3600), "1h");
        assert_eq!(format_lease_time(120), "2m");
        assert_eq!(format_lease_time(90), "90s");
    }
}
