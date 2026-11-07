use std::sync::Arc;

use super::NetworksScreen;

use ratatui::widgets::TableState;

use unifly_api::Network;

use crate::cli::commands::util::order;
use crate::tui::action::Action;
use crate::tui::table::{TableScroll, reselect};

pub(super) use super::edit::NetworkEditState;

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
            table_scroll: TableScroll::default(),
            detail_open: false,
            edit_state: None,
            hits: std::cell::RefCell::default(),
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
    use unifly_api::EntityId;

    use super::{NetworkEditState, NetworksScreen, format_lease_time};

    use serde_json::json;
    use unifly_api::Network;
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
    fn tapping_a_row_selects_it_and_a_second_tap_toggles_its_detail() {
        use crate::tui::component::Component;
        use crate::tui::table::render_lines;
        use crate::tui::widgets::click::click_label;

        let mut studio = test_network();
        studio.id = EntityId::Uuid(Uuid::from_u128(2));
        studio.name = "Studio".into();
        let mut screen = NetworksScreen::new();
        screen.apply_action(&Action::NetworksUpdated(Arc::new(vec![
            Arc::new(test_network()),
            Arc::new(studio),
        ])));
        let draw = |screen: &NetworksScreen| {
            render_lines(120, 20, |frame, area| screen.render(frame, area))
        };

        let rows = draw(&screen);
        screen.handle_mouse_input(click_label(&rows, "Studio", "Studio"));
        assert_eq!(screen.selected_index(), 1);
        assert!(!screen.detail_open);

        let rows = draw(&screen);
        screen.handle_mouse_input(click_label(&rows, "Studio", "Studio"));
        assert!(screen.detail_open);

        screen.edit_state = Some(NetworkEditState::from_network(&test_network()));
        let rows = draw(&screen);
        screen.handle_mouse_input(click_label(&rows, "Primary LAN", "Primary LAN"));
        assert_eq!(screen.selected_index(), 1, "the edit form holds the input");

        assert!(
            !rows.iter().any(|row| row.contains("toggle")),
            "Space would type into the Name field, so no toggle button"
        );

        let save = screen.handle_mouse_input(click_label(&rows, "save", "save"));
        assert!(
            matches!(save, Some(Action::PressKey(key)) if key.code == crossterm::event::KeyCode::Enter),
            "{save:?}"
        );
    }

    #[test]
    fn edit_state_build_request_tracks_toggles() {
        let mut edit = NetworkEditState::from_network(&test_network());
        edit.field_idx = 2;
        edit.toggle_bool();

        let request = edit
            .build_request()
            .expect("valid form")
            .expect("changed field");

        assert_eq!(request.name, None);
        assert_eq!(request.vlan_id, None);
        assert_eq!(request.enabled, Some(false));
        assert_eq!(request.dhcp_enabled, None);
        assert_eq!(request.ipv6_enabled, None);
        assert_eq!(
            serde_json::to_value(request).expect("request JSON"),
            json!({"enabled": false})
        );
    }

    #[test]
    fn network_form_keeps_partial_edits_and_validates_dhcp_ranges() {
        let mut edit = NetworkEditState::from_network(&test_network());
        assert!(edit.build_request().expect("unchanged").is_none());
        edit.field_idx = 8;
        for _ in 0.."192.168.100.10".len() {
            edit.handle_backspace();
        }
        for ch in "192.168.100.20".chars() {
            edit.handle_text_input(ch);
        }
        let request = edit.build_request().expect("valid range").expect("change");
        let dhcp = request.dhcp.expect("DHCP update");
        assert_eq!(dhcp.range_start.as_deref(), Some("192.168.100.20"));
        assert_eq!(dhcp.range_stop, None);
        assert_eq!(request.dhcp_enabled, None);
        assert_eq!(request.name, None);
        for _ in 0.."192.168.100.20".len() {
            edit.handle_backspace();
        }
        for ch in "192.168.100.251".chars() {
            edit.handle_text_input(ch);
        }
        assert!(edit.build_request().is_err());
    }

    #[test]
    fn enabling_dhcp_can_supply_a_new_range_without_resending_other_fields() {
        let mut network = test_network();
        network.dhcp = None;
        let mut edit = NetworkEditState::from_network(&network);
        edit.field_idx = 3;
        edit.toggle_bool();
        assert!(edit.build_request().is_err());
        for (index, address) in [(8, "192.168.100.10"), (9, "192.168.100.200")] {
            edit.field_idx = index;
            for ch in address.chars() {
                edit.handle_text_input(ch);
            }
        }
        let request = edit.build_request().expect("valid range").expect("changed");
        assert_eq!(
            serde_json::to_value(request).expect("request JSON"),
            json!({
                "dhcp_enabled": true,
                "dhcp": { "range_start": "192.168.100.10", "range_stop": "192.168.100.200" }
            })
        );
    }

    #[test]
    fn invalid_network_edit_stays_open_and_short_form_keeps_focus_visible() {
        use crossterm::event::{KeyCode, KeyEvent};
        let mut screen = NetworksScreen::new();
        screen.apply_action(&Action::NetworksUpdated(Arc::new(vec![Arc::new(
            test_network(),
        )])));
        screen.handle_key_input(KeyEvent::from(KeyCode::Char('e')));
        let edit = screen.edit_state.as_mut().expect("form");
        edit.field_idx = 9;
        edit.handle_text_input('9');
        assert!(matches!(
            screen.handle_key_input(KeyEvent::from(KeyCode::Enter)),
            Some(Action::Notify(_))
        ));
        assert!(screen.edit_state.is_some());
        let rows = crate::tui::table::render_lines(60, 12, |frame, area| {
            screen.render_screen(frame, area);
        });
        assert!(
            rows.iter().any(|row| row.contains("DHCP stop")),
            "{}",
            rows.join("\n")
        );
        assert!(
            rows.iter().any(|row| row.contains("save")),
            "{}",
            rows.join("\n")
        );
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
    fn edits_save_to_the_network_they_were_opened_on() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let network = |id: u128, name: &str, vlan: u16| -> Arc<Network> {
            let mut network = test_network();
            network.id = EntityId::Uuid(Uuid::from_u128(id));
            network.name = name.to_owned();
            network.vlan_id = Some(vlan);
            Arc::new(network)
        };
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);

        let mut screen = NetworksScreen::new();
        screen.apply_action(&Action::NetworksUpdated(Arc::new(vec![
            network(1, "Guest", 30),
            network(2, "IoT", 40),
        ])));
        screen.select(1);
        let _ = screen.handle_key_input(key(KeyCode::Char('e')));

        // IoT disappears while its form is open; the row now holds Guest.
        screen.apply_action(&Action::NetworksUpdated(Arc::new(vec![network(
            1, "Guest", 30,
        )])));
        assert!(screen.handle_key_input(key(KeyCode::Enter)).is_none());

        screen.apply_action(&Action::NetworksUpdated(Arc::new(vec![
            network(1, "Guest", 30),
            network(3, "Cameras", 50),
        ])));
        screen.select(0);
        let _ = screen.handle_key_input(key(KeyCode::Char('e')));
        let _ = screen.handle_key_input(key(KeyCode::Char('!')));
        screen.apply_action(&Action::NetworksUpdated(Arc::new(vec![
            network(4, "Admin", 5),
            network(1, "Guest", 30),
            network(3, "Cameras", 50),
        ])));
        assert!(matches!(
            screen.handle_key_input(key(KeyCode::Enter)),
            Some(Action::NetworkSave(id, _)) if id == EntityId::Uuid(Uuid::from_u128(3))
        ));
    }

    #[test]
    fn lease_time_formats_common_units() {
        assert_eq!(format_lease_time(3600), "1h");
        assert_eq!(format_lease_time(120), "2m");
        assert_eq!(format_lease_time(90), "90s");
    }
}
