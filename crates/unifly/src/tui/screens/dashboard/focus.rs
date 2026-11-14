//! Keyboard focus across the dashboard's panels, and what a panel opens.

use crossterm::event::{KeyCode, KeyEvent, MouseEvent};

use unifly_api::{Command, DeviceType};

use super::DashboardScreen;
use crate::tui::action::{Action, ConfirmRequest, Notice, Notification};
use crate::tui::screen::ScreenId;
use crate::tui::widgets::click::{self, Click};

/// One dashboard panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Panel {
    #[default]
    Traffic,
    Gateway,
    Connectivity,
    Capacity,
    Networks,
    Wifi,
    TopClients,
    Events,
}

/// The panels as they sit on screen, top row first.
const GRID: [&[Panel]; 4] = [
    &[Panel::Traffic],
    &[Panel::Gateway, Panel::Connectivity, Panel::Capacity],
    &[Panel::Networks, Panel::Wifi, Panel::TopClients],
    &[Panel::Events],
];

impl Panel {
    /// The screen that tells this panel's full story.
    pub(super) fn screen(self) -> ScreenId {
        match self {
            Self::Traffic => ScreenId::Stats,
            Self::Gateway | Self::Connectivity | Self::Capacity => ScreenId::Devices,
            Self::Networks => ScreenId::Networks,
            Self::Wifi => ScreenId::Wifi,
            Self::TopClients => ScreenId::Clients,
            Self::Events => ScreenId::Events,
        }
    }

    /// What `Enter` does here, for the hint bar.
    fn open_label(self) -> &'static str {
        match self {
            Self::Traffic => "open Stats",
            Self::Gateway => "open the gateway",
            Self::Connectivity | Self::Capacity => "open Devices",
            Self::Networks => "open Networks",
            Self::Wifi => "open WiFi",
            Self::TopClients => "open Clients",
            Self::Events => "open Events",
        }
    }

    fn position(self) -> (usize, usize) {
        GRID.iter()
            .enumerate()
            .find_map(|(row, panels)| {
                let column = panels.iter().position(|panel| *panel == self)?;
                Some((row, column))
            })
            .unwrap_or((0, 0))
    }
}

impl DashboardScreen {
    pub(super) fn handle_key_input(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.move_focus(0, -1),
            KeyCode::Right | KeyCode::Char('l') => self.move_focus(0, 1),
            KeyCode::Up | KeyCode::Char('k') => self.move_focus(-1, 0),
            KeyCode::Down | KeyCode::Char('j') => self.move_focus(1, 0),
            KeyCode::Char('g') => self.set_focus(Panel::Traffic),
            KeyCode::Char('G') => self.set_focus(Panel::Events),
            KeyCode::Enter => return Some(self.open_panel(self.focus)),
            KeyCode::Char('S') => return Some(self.request_speed_test()),
            _ => return None,
        }
        Some(Action::Invalidate)
    }

    /// Move focus one panel along a row or between rows, stopping at the
    /// edges. Moving between rows keeps the column last chosen, so going
    /// down and up through a one-panel row returns to the same panel.
    fn move_focus(&mut self, rows: isize, columns: isize) {
        let (row, column) = self.focus.position();
        if columns != 0 {
            let last = GRID[row].len() - 1;
            self.column = column.saturating_add_signed(columns).min(last);
            self.focus = GRID[row][self.column];
            return;
        }
        let row = row.saturating_add_signed(rows).min(GRID.len() - 1);
        let panels = GRID[row];
        self.focus = panels[self.column.min(panels.len() - 1)];
    }

    pub(super) fn set_focus(&mut self, panel: Panel) {
        self.focus = panel;
        let (_, column) = panel.position();
        if GRID[panel.position().0].len() > 1 {
            self.column = column;
        }
    }

    /// What opening `panel` does: its screen, or for the gateway panel the
    /// gateway's own detail.
    pub(super) fn open_panel(&self, panel: Panel) -> Action {
        if panel == Panel::Gateway
            && let Some(gateway) = self.gateway()
        {
            return Action::ShowDevice(gateway.id.clone());
        }
        Action::SwitchScreen(panel.screen())
    }

    pub(super) fn gateway(&self) -> Option<&std::sync::Arc<unifly_api::Device>> {
        self.devices
            .iter()
            .find(|device| device.device_type == DeviceType::Gateway)
    }

    /// Ask before starting a WAN speed test, or say why one cannot run.
    fn request_speed_test(&self) -> Action {
        if self.gateway().is_none() {
            return Action::Notify(Notification::info(
                "A speed test needs a UniFi gateway on this site",
            ));
        }
        if !self.access.is_some_and(|access| access.session) {
            return Action::Notify(Notification::info(
                "A speed test needs a direct controller session",
            ));
        }
        Action::ShowConfirm(
            ConfirmRequest::new(
                "Run a WAN speed test?",
                Action::Run {
                    command: Box::new(Command::SpeedtestDevice),
                    notice: Notice::new(
                        "Starting the speed test\u{2026}",
                        "Speed test started: the result arrives as an event",
                    ),
                },
            )
            .detail("It fills the WAN link in both directions for about a minute."),
        )
    }

    /// A tap focuses the panel under it and opens what was tapped: a row's
    /// device or client where rows are listed, else the panel's screen.
    pub(super) fn handle_mouse_input(&mut self, mouse: MouseEvent) -> Option<Action> {
        if !click::is_left_press(mouse) {
            return None;
        }
        let panel = self.panel_hits.borrow().at(mouse.column, mouse.row);
        if let Some(panel) = panel {
            self.set_focus(panel);
        }
        match self.hits.borrow().clicked(mouse) {
            Some(Click::Action(action)) => Some(action),
            Some(Click::Key(key)) => Some(Action::PressKey(key)),
            _ => panel.map(|panel| self.open_panel(panel)),
        }
    }

    pub(super) fn help_keys() -> Vec<(&'static str, &'static str)> {
        vec![
            (
                "hjkl \u{2190}\u{2193}\u{2191}\u{2192}",
                "Move between panels",
            ),
            ("g/G", "First / last panel"),
            ("Enter", "Open the focused panel"),
            ("S", "Run a WAN speed test"),
        ]
    }

    pub(super) fn hint_bar(&self) -> click::ClickLine<'static, Click> {
        click::key_hints(&[
            click::Hint::info("hjkl", "focus"),
            click::Hint::button("Enter", self.focus.open_label(), KeyCode::Enter),
            click::Hint::button("S", "speed test", click::char_key('S')),
        ])
    }
}

#[cfg(test)]
pub(super) mod tests {
    use std::sync::Arc;

    use crossterm::event::{KeyCode, KeyEvent};
    use serde_json::json;
    use unifly_api::{Client, Command, Device};

    use super::Panel;
    use crate::tui::action::{Access, Action};
    use crate::tui::component::Component;
    use crate::tui::screen::ScreenId;
    use crate::tui::screens::dashboard::DashboardScreen;
    use crate::tui::table::render_lines;
    use crate::tui::widgets::click::click_label;

    pub(in crate::tui::screens::dashboard) fn device(
        name: &str,
        mac: &str,
        device_type: &str,
    ) -> Arc<Device> {
        Arc::new(
            serde_json::from_value(json!({
                "id": format!("id-{name}"), "mac": mac, "ip": "10.0.0.2", "wan_ipv6": null,
                "name": name, "model": "UniFi", "device_type": device_type,
                "state": "Online", "firmware_version": null, "firmware_updatable": false,
                "adopted_at": null, "provisioned_at": null, "last_seen": null,
                "serial": null, "supported": true, "ports": [], "radios": [],
                "uplink_device_id": null, "uplink_device_mac": null,
                "has_switching": false, "has_access_point": false, "stats": {},
                "client_count": 3, "origin": null
            }))
            .expect("device fixture should deserialize"),
        )
    }

    /// A wireless client that has moved `bytes` in total.
    pub(in crate::tui::screens::dashboard) fn client(
        name: &str,
        mac: &str,
        bytes: u64,
    ) -> Arc<Client> {
        let mut client: Client = unifly_api::integration_types::ClientResponse {
            id: uuid::Uuid::new_v4(),
            name: name.to_owned(),
            client_type: "WIRELESS".to_owned(),
            ip_address: Some("192.168.1.10".to_owned()),
            connected_at: None,
            mac_address: Some(mac.to_owned()),
            access: json!({ "type": "DEFAULT" }),
        }
        .into();
        client.id = unifly_api::EntityId::Legacy(format!("id-{name}"));
        client.tx_bytes = Some(bytes / 2);
        client.rx_bytes = Some(bytes / 2);
        Arc::new(client)
    }

    fn press(screen: &mut DashboardScreen, code: KeyCode) -> Option<Action> {
        screen.handle_key_event(KeyEvent::from(code)).expect("key")
    }

    fn connected() -> DashboardScreen {
        let mut screen = DashboardScreen::new();
        screen.focused = true;
        screen.update(&Action::Connected).expect("connected");
        screen
            .update(&Action::AccessUpdated(Access {
                integration: true,
                session: true,
                live_events: true,
            }))
            .expect("access");
        screen
            .update(&Action::DevicesUpdated(Arc::new(vec![
                device("gw", "00:00:00:00:00:01", "Gateway"),
                device("ap-loft", "00:00:00:00:00:02", "AccessPoint"),
            ])))
            .expect("devices");
        screen
    }

    #[test]
    fn arrows_and_vim_keys_move_focus_across_the_panel_grid() {
        let mut screen = connected();
        assert_eq!(screen.focus, Panel::Traffic);

        press(&mut screen, KeyCode::Down);
        assert_eq!(screen.focus, Panel::Gateway);
        press(&mut screen, KeyCode::Char('l'));
        press(&mut screen, KeyCode::Right);
        assert_eq!(screen.focus, Panel::Capacity);
        press(&mut screen, KeyCode::Right);
        assert_eq!(screen.focus, Panel::Capacity, "focus stops at the edge");
        press(&mut screen, KeyCode::Char('j'));
        assert_eq!(screen.focus, Panel::TopClients);
        press(&mut screen, KeyCode::Char('h'));
        assert_eq!(screen.focus, Panel::Wifi);
        // Through the one-panel rows and back, the column is kept.
        press(&mut screen, KeyCode::Down);
        assert_eq!(screen.focus, Panel::Events);
        press(&mut screen, KeyCode::Char('k'));
        assert_eq!(screen.focus, Panel::Wifi);
        press(&mut screen, KeyCode::Char('g'));
        assert_eq!(screen.focus, Panel::Traffic);
        press(&mut screen, KeyCode::Down);
        assert_eq!(screen.focus, Panel::Connectivity);
    }

    #[test]
    fn enter_opens_the_focused_panels_screen_and_the_gateway_itself() {
        let mut screen = connected();
        let stats = press(&mut screen, KeyCode::Enter);
        assert!(matches!(stats, Some(Action::SwitchScreen(ScreenId::Stats))));

        press(&mut screen, KeyCode::Down);
        let gateway = press(&mut screen, KeyCode::Enter);
        assert!(
            matches!(&gateway, Some(Action::ShowDevice(id)) if id.to_string() == "id-gw"),
            "{gateway:?}"
        );

        for (key, target) in [
            (KeyCode::Right, ScreenId::Devices),
            (KeyCode::Down, ScreenId::Wifi),
            (KeyCode::Right, ScreenId::Clients),
            (KeyCode::Down, ScreenId::Events),
        ] {
            press(&mut screen, key);
            let opened = press(&mut screen, KeyCode::Enter);
            assert!(
                matches!(opened, Some(Action::SwitchScreen(screen)) if screen == target),
                "{target:?}: {opened:?}"
            );
        }
    }

    #[test]
    fn the_focused_panel_is_marked_and_the_hint_bar_names_what_enter_opens() {
        let mut screen = connected();
        let draw = |screen: &DashboardScreen| {
            render_lines(120, 40, |frame, area| screen.render(frame, area))
        };

        let rows = draw(&screen);
        assert!(rows.iter().any(|row| row.contains("\u{25B8} WAN Traffic")));
        let hints = &rows[rows.len() - 2];
        assert!(hints.contains("Enter open Stats"), "{hints}");
        assert!(hints.contains("S speed test"), "{hints}");

        press(&mut screen, KeyCode::Down);
        let rows = draw(&screen);
        assert!(rows.iter().any(|row| row.contains("\u{25B8} Gateway")));
        assert!(!rows.iter().any(|row| row.contains("\u{25B8} WAN Traffic")));
        assert!(rows[rows.len() - 2].contains("Enter open the gateway"));

        // An unfocused dashboard marks nothing.
        screen.focused = false;
        assert!(!draw(&screen).iter().any(|row| row.contains('\u{25B8}')));
    }

    #[test]
    fn a_tapped_row_opens_its_access_point_or_client() {
        let mut screen = connected();
        screen
            .update(&Action::ClientsUpdated(Arc::new(vec![
                client("nas-vault", "00:00:00:00:10:01", 9_000_000),
                client("render-03", "00:00:00:00:10:02", 5_000_000),
            ])))
            .expect("clients");
        let rows = render_lines(140, 40, |frame, area| screen.render(frame, area));

        let ap = screen
            .handle_mouse_event(click_label(&rows, "ap-loft", "ap-loft"))
            .expect("click");
        assert!(
            matches!(&ap, Some(Action::ShowDevice(id)) if id.to_string() == "id-ap-loft"),
            "{ap:?}"
        );
        assert_eq!(screen.focus, Panel::Wifi, "the tapped panel takes focus");

        let client = screen
            .handle_mouse_event(click_label(&rows, "render-03", "render-03"))
            .expect("click");
        assert!(
            matches!(&client, Some(Action::ShowClient(id)) if id.to_string() == "id-render-03"),
            "{client:?}"
        );

        let connectivity = screen
            .handle_mouse_event(click_label(&rows, "Connectivity", "Connectivity"))
            .expect("click");
        assert!(
            matches!(connectivity, Some(Action::SwitchScreen(ScreenId::Devices))),
            "{connectivity:?}"
        );
    }

    #[test]
    fn a_speed_test_asks_first_and_explains_when_it_cannot_run() {
        let mut screen = connected();
        let asked = press(&mut screen, KeyCode::Char('S'));
        let Some(Action::ShowConfirm(confirm)) = asked else {
            panic!("expected a confirmation, got {asked:?}");
        };
        assert!(confirm.prompt.contains("speed test"));
        assert!(confirm.detail.is_some(), "the cost is spelled out");
        assert!(matches!(
            *confirm.action,
            Action::Run { ref command, .. } if matches!(**command, Command::SpeedtestDevice)
        ));

        screen
            .update(&Action::AccessUpdated(Access {
                integration: true,
                session: false,
                live_events: false,
            }))
            .expect("access");
        let refused = press(&mut screen, KeyCode::Char('S'));
        assert!(matches!(refused, Some(Action::Notify(_))), "{refused:?}");

        let mut no_gateway = connected();
        no_gateway
            .update(&Action::DevicesUpdated(Arc::new(vec![device(
                "ap-loft",
                "00:00:00:00:00:02",
                "AccessPoint",
            )])))
            .expect("devices");
        let refused = press(&mut no_gateway, KeyCode::Char('S'));
        assert!(matches!(refused, Some(Action::Notify(_))), "{refused:?}");
    }
}
