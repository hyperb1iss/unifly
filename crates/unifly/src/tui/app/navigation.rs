use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use super::{App, ConnectionStatus};
use crate::tui::action::{Action, Notification};
use crate::tui::screen::ScreenId;
use crate::tui::widgets::click::Click;

impl App {
    /// Map a key event to an action. Global keys are handled here;
    /// screen-specific keys are delegated to the active screen component.
    pub(super) fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        self.needs_redraw = true;

        if let Some(action) = self.handle_special_screen_key_event(key)? {
            return Ok(Some(action));
        }

        if self.active_screen == ScreenId::Setup || self.active_screen == ScreenId::Settings {
            return Ok(None);
        }

        if self.pending_confirm.is_some() {
            return Ok(match key.code {
                KeyCode::Char('y' | 'Y') => Some(Action::ConfirmYes),
                KeyCode::Char('n' | 'N') | KeyCode::Esc => Some(Action::ConfirmNo),
                _ => None,
            });
        }

        if self.search_active {
            return Ok(self.handle_search_key_event(key));
        }

        if self.help_visible {
            return Ok(match key.code {
                KeyCode::Esc | KeyCode::Char('?') => Some(Action::ToggleHelp),
                _ => None,
            });
        }

        if self.about_visible {
            return Ok(match key.code {
                KeyCode::Esc | KeyCode::Char('a') => Some(Action::ToggleAbout),
                KeyCode::Char('d') => Some(Action::OpenDonate),
                KeyCode::Char('g') => {
                    super::dispatch::open_url("https://github.com/hyperb1iss/unifly");
                    None
                }
                _ => None,
            });
        }

        // Intercepted ahead of screen forwarding: screen key handlers match
        // on the bare key code and would swallow Ctrl+r as a plain 'r'.
        if key.modifiers == KeyModifiers::CONTROL
            && key.code == KeyCode::Char('r')
            && self.connection_status == ConnectionStatus::Disconnected
        {
            return Ok(Some(Action::RetryConnect));
        }

        // Read before forwarding: a key that closes the form (Esc) must not
        // go on to a global shortcut either.
        let typing = self.active_screen_captures_input();
        if let Ok(Some(action)) = self.forward_key_to_active_screen(key) {
            return Ok(Some(action));
        }

        let quit = key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c');
        if typing && !quit {
            return Ok(None);
        }

        if let Some(action) = self.handle_global_key_event(key) {
            return Ok(Some(action));
        }

        Ok(None)
    }

    /// Handle mouse events: the App's own targets first, then the active
    /// screen.
    pub(super) fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        // Presses, drags, and wheel turns can change screen state without
        // dispatching an invalidating action (a selection, a settings
        // toggle). Bare pointer motion changes nothing and stays out.
        if !matches!(mouse.kind, MouseEventKind::Moved | MouseEventKind::Up(_)) {
            self.needs_redraw = true;
        }
        // App chrome sits on top: the tab and status bars, and any overlay,
        // whose backdrop covers the screen beneath it.
        if let Some(target) = self.hits.clicked(mouse) {
            return match target {
                Click::Key(key) => self.handle_key_event(key),
                Click::Action(action) => Ok(Some(action)),
                Click::Tab(_) | Click::Row(_) | Click::Absorb => Ok(None),
            };
        }

        // Overlays hold wheel turns and drags too, as they do for keys.
        if self.pending_confirm.is_some()
            || self.search_active
            || self.help_visible
            || self.about_visible
        {
            return Ok(None);
        }

        if let Some(screen) = self.screens.get_mut(&self.active_screen) {
            return screen.handle_mouse_event(mouse);
        }
        Ok(None)
    }

    /// Open the search bar on the active screen's filter, so a filter kept
    /// with `Enter` can be refined or cleared. Where search filters
    /// nothing, a toast says where it does instead.
    pub(super) fn open_search(&mut self) {
        let query = self
            .screens
            .get(&self.active_screen)
            .and_then(|screen| screen.search_query())
            .map(str::to_owned);
        match query {
            Some(query) => {
                self.search_active = true;
                self.search_query = query;
            }
            None => self.show_notification(Notification::info(
                "Search filters the Devices, Clients, and WiFi lists",
            )),
        }
    }

    /// Whether search filters anything on the active screen.
    pub(super) fn active_screen_searches(&self) -> bool {
        self.screens
            .get(&self.active_screen)
            .is_some_and(|screen| screen.search_query().is_some())
    }

    /// Whether the active screen is taking typed input, such as a form.
    pub(super) fn active_screen_captures_input(&self) -> bool {
        self.screens
            .get(&self.active_screen)
            .is_some_and(|screen| screen.captures_input())
    }

    fn handle_special_screen_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        if self.active_screen == ScreenId::Setup || self.active_screen == ScreenId::Settings {
            if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
                return Ok(Some(Action::Quit));
            }

            if let Some(screen) = self.screens.get_mut(&self.active_screen) {
                return screen.handle_key_event(key);
            }
        }

        Ok(None)
    }

    fn handle_search_key_event(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc => {
                self.search_query.clear();
                Some(Action::CloseSearch)
            }
            KeyCode::Enter => Some(Action::SearchSubmit),
            KeyCode::Backspace => {
                self.search_query.pop();
                Some(Action::SearchInput(self.search_query.clone()))
            }
            KeyCode::Char(c) => {
                self.search_query.push(c);
                Some(Action::SearchInput(self.search_query.clone()))
            }
            _ => None,
        }
    }

    fn handle_global_key_event(&self, key: KeyEvent) -> Option<Action> {
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c'))
            | (KeyModifiers::NONE, KeyCode::Char('q')) => Some(Action::Quit),
            (KeyModifiers::NONE, KeyCode::Char('?')) => Some(Action::ToggleHelp),
            (KeyModifiers::NONE, KeyCode::Char('a')) => Some(Action::ToggleAbout),
            (KeyModifiers::NONE, KeyCode::Char('/')) => Some(Action::OpenSearch),
            (KeyModifiers::NONE, KeyCode::Char(',')) => Some(Action::OpenSettings),
            (KeyModifiers::NONE, KeyCode::Char(c @ '1'..='9')) => {
                #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
                let n = c.to_digit(10).unwrap_or(0) as u8;
                ScreenId::from_number(n).map(Action::SwitchScreen)
            }
            (KeyModifiers::NONE, KeyCode::Tab) => {
                Some(Action::SwitchScreen(self.active_screen.next()))
            }
            (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                Some(Action::SwitchScreen(self.active_screen.prev()))
            }
            (KeyModifiers::NONE, KeyCode::Esc) => Some(Action::GoBack),
            _ => None,
        }
    }

    fn forward_key_to_active_screen(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        if let Some(screen) = self.screens.get_mut(&self.active_screen) {
            return screen.handle_key_event(key);
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::tui::table::render_lines;
    use crate::tui::widgets::click::{click_label, left_click};

    fn dashboard_app() -> App {
        let mut app = App::new(None, None, false, true);
        app.active_screen = ScreenId::Dashboard;
        app
    }

    fn render(app: &mut App, width: u16) -> Vec<String> {
        render_lines(width, 24, |frame, _| app.render(frame))
    }

    /// The tab bar row of a rendered frame.
    fn tab_row(rows: &[String]) -> &str {
        rows.iter()
            .find(|row| row.contains("6 Topo"))
            .expect("tab bar row")
    }

    /// The label a tab bar row shows for `id`, full or short.
    fn shown_label(row: &str, id: ScreenId) -> String {
        [id.label(), id.label_short()]
            .into_iter()
            .map(|label| format!("{} {label}", id.number()))
            .find(|label| row.contains(label.as_str()))
            .unwrap_or_else(|| panic!("{id} tab not drawn in {row:?}"))
    }

    #[test]
    fn clicking_a_tab_switches_to_its_screen() {
        for width in [130, 110, 95, 80] {
            let mut app = dashboard_app();
            let rows = render(&mut app, width);

            for id in ScreenId::ALL {
                let label = shown_label(tab_row(&rows), id);
                let click = click_label(&rows, "6 Topo", &label);
                let action = app.handle_mouse_event(click).expect("click handling");
                assert!(
                    matches!(action, Some(Action::SwitchScreen(target)) if target == id),
                    "{width} cols, {label}: {action:?}"
                );

                // The space before the number belongs to the tab as well.
                let action = app
                    .handle_mouse_event(left_click(click.column - 1, click.row))
                    .expect("click handling");
                assert!(
                    matches!(action, Some(Action::SwitchScreen(target)) if target == id),
                    "{width} cols, left edge of {label}: {action:?}"
                );
            }
        }
    }

    #[test]
    fn every_tab_is_drawn_from_74_columns_up() {
        for width in 74..=130 {
            let mut app = dashboard_app();
            let rows = render(&mut app, width);
            let row = tab_row(&rows);
            for id in ScreenId::ALL {
                shown_label(row, id);
            }
            let full = row.contains("1 Dashboard");
            assert_eq!(full, width >= 102, "{width} cols: {row:?}");
        }
    }

    #[test]
    fn overlays_and_full_screen_views_swallow_tab_clicks() {
        let mut app = dashboard_app();
        let rows = render(&mut app, 120);
        let devices = click_label(&rows, "6 Topo", "2 Devices");

        // A tap outside the help overlay closes it rather than switching.
        app.help_visible = true;
        render(&mut app, 120);
        let action = app.handle_mouse_event(devices).expect("click");
        assert!(matches!(action, Some(Action::ToggleHelp)), "{action:?}");
        app.help_visible = false;

        app.search_active = true;
        render(&mut app, 120);
        assert!(app.handle_mouse_event(devices).expect("click").is_none());
        app.search_active = false;

        // Settings draws over the tab bar, so it leaves no tabs to click.
        app.active_screen = ScreenId::Settings;
        render(&mut app, 120);
        assert!(app.handle_mouse_event(devices).expect("click").is_none());
    }

    #[test]
    fn clicks_drags_and_wheel_turns_reopen_the_render_gate() {
        use crossterm::event::{MouseButton, MouseEventKind};

        use crate::tui::widgets::click::mouse;

        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::ScrollDown,
        ] {
            let mut app = dashboard_app();
            app.needs_redraw = false;
            app.handle_mouse_event(mouse(kind, 0, 0))
                .expect("mouse handling");
            assert!(app.should_draw(), "{kind:?}");
        }

        let mut app = dashboard_app();
        app.needs_redraw = false;
        app.handle_mouse_event(mouse(MouseEventKind::Moved, 0, 0))
            .expect("mouse handling");
        assert!(!app.should_draw(), "pointer motion");
    }

    #[test]
    fn tab_navigation_cycles_between_primary_screens() {
        let mut app = App::new(None, None, false, true);
        app.active_screen = ScreenId::Dashboard;

        let action = app
            .handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .expect("key handling should succeed");
        assert!(matches!(
            action,
            Some(Action::SwitchScreen(ScreenId::Devices))
        ));

        let action = app
            .handle_key_event(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT))
            .expect("key handling should succeed");
        assert!(matches!(action, Some(Action::SwitchScreen(ScreenId::Wifi))));
    }

    #[test]
    fn a_screen_taking_typed_input_keeps_keys_from_global_shortcuts() {
        let mut app = dashboard_app();
        app.active_screen = ScreenId::Networks;
        let press = |code| KeyEvent::new(code, KeyModifiers::NONE);

        app.process_action(&Action::NetworksUpdated(std::sync::Arc::new(vec![
            std::sync::Arc::new(
                serde_json::from_value(serde_json::json!({
                    "id": "11111111-1111-1111-1111-111111111111", "name": "Studio",
                    "vlan_id": 10, "enabled": true, "management": "Gateway",
                    "purpose": "Corporate", "is_default": false, "subnet": null,
                    "gateway_ip": null, "isolation_enabled": false,
                    "internet_access_enabled": true, "mdns_forwarding_enabled": false,
                    "ipv6_enabled": false, "ipv6_mode": null, "ipv6_prefix": null,
                    "dhcpv6_enabled": false, "slaac_enabled": false, "ntp_server": null,
                    "pxe_enabled": false, "tftp_server": null, "firewall_zone_id": null,
                    "cellular_backup_enabled": false, "origin": null, "dhcp": null
                }))
                .expect("network fixture"),
            ),
        ])))
        .expect("networks update");
        app.handle_key_event(press(KeyCode::Char('e')))
            .expect("open the edit form");

        for code in [KeyCode::Tab, KeyCode::Char('1'), KeyCode::Char('q')] {
            let action = app.handle_key_event(press(code)).expect("typed key");
            assert!(
                action.is_none(),
                "{code:?} leaked to a global shortcut: {action:?}"
            );
        }
        assert!(app.running);

        let quit = app
            .handle_key_event(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .expect("ctrl+c");
        assert!(matches!(quit, Some(Action::Quit)));
    }

    #[test]
    fn a_tapped_key_hint_presses_its_key_through_the_key_path() {
        let mut app = dashboard_app();
        app.active_screen = ScreenId::Events;
        let rows = render(&mut app, 120);

        let action = app
            .handle_mouse_event(click_label(&rows, "pause/resume", "pause/resume"))
            .expect("click handling");
        let Some(Action::PressKey(key)) = action else {
            panic!("expected a key press, got {action:?}");
        };
        assert_eq!(key, crate::tui::widgets::click::char_key(' '));

        app.process_action(&Action::PressKey(key))
            .expect("key press handling");
        let queued = app.action_rx.try_recv().expect("the press dispatches");
        assert!(matches!(queued, Action::ToggleEventPause), "{queued:?}");

        // A pressed key the screen leaves alone reaches the global keys:
        // Events leaves `?` to the App, which opens help.
        app.process_action(&Action::PressKey(crate::tui::widgets::click::char_key('?')))
            .expect("key press handling");
        let queued = app.action_rx.try_recv().expect("the press dispatches");
        assert!(matches!(queued, Action::ToggleHelp), "{queued:?}");
    }

    #[test]
    fn overlays_close_on_an_outside_tap_and_take_taps_on_their_buttons() {
        // Tall enough for the whole help overlay, close hint included.
        let draw = |app: &mut App| render_lines(120, 34, |frame, _| app.render(frame));
        let mut app = dashboard_app();
        let tap = |app: &mut App, marker: &str, label: &str| {
            let rows = draw(app);
            app.handle_mouse_event(click_label(&rows, marker, label))
                .expect("click handling")
        };

        app.help_visible = true;
        let rows = draw(&mut app);
        assert!(
            app.handle_mouse_event(click_label(&rows, "Navigation", "Navigation"))
                .expect("click")
                .is_none(),
            "a tap inside the overlay does nothing"
        );
        let action = tap(&mut app, "to close", "Esc or ? to close");
        assert!(matches!(action, Some(Action::ToggleHelp)), "{action:?}");
        app.help_visible = false;

        app.about_visible = true;
        let action = tap(&mut app, "onate", "onate");
        assert!(matches!(action, Some(Action::OpenDonate)), "{action:?}");
        let action = tap(&mut app, "to close", "Esc or a to close");
        assert!(matches!(action, Some(Action::ToggleAbout)), "{action:?}");
        app.about_visible = false;

        app.pending_confirm = Some(crate::tui::action::ConfirmAction::RestartDevice {
            id: unifly_api::EntityId::Legacy("device-1".into()),
            name: "sw-core".into(),
        });
        let action = tap(&mut app, "confirm", "confirm");
        assert!(matches!(action, Some(Action::ConfirmYes)), "{action:?}");
        let action = tap(&mut app, "6 Topo", "2 Devices");
        assert!(
            matches!(action, Some(Action::ConfirmNo)),
            "a tap outside answers no: {action:?}"
        );
    }

    #[test]
    fn the_status_bar_and_toasts_take_taps() {
        let mut app = dashboard_app();
        let rows = render(&mut app, 120);
        let action = app
            .handle_mouse_event(click_label(&rows, "settings", "help"))
            .expect("click");
        assert!(matches!(action, Some(Action::ToggleHelp)), "{action:?}");
        let action = app
            .handle_mouse_event(click_label(&rows, "settings", "Sponsor"))
            .expect("click");
        assert!(matches!(action, Some(Action::OpenDonate)), "{action:?}");

        app.show_notification(crate::tui::action::Notification::info(
            "restart requested for sw-core",
        ));
        let rows = render(&mut app, 120);
        let action = app
            .handle_mouse_event(click_label(&rows, "restart requested", "restart"))
            .expect("click");
        assert!(
            matches!(action, Some(Action::DismissNotification)),
            "{action:?}"
        );

        app.notification = None;
        app.search_active = true;
        app.search_query = "ap".into();
        let rows = render(&mut app, 120);
        let action = app
            .handle_mouse_event(click_label(&rows, "Enter done", "clear"))
            .expect("click");
        assert!(matches!(action, Some(Action::CloseSearch)), "{action:?}");
    }

    fn networks_app() -> App {
        let mut app = dashboard_app();
        app.process_action(&Action::NetworksUpdated(std::sync::Arc::new(vec![
            std::sync::Arc::new(
                serde_json::from_value(serde_json::json!({
                    "id": "11111111-1111-1111-1111-111111111111", "name": "Studio",
                    "vlan_id": 10, "enabled": true, "management": "Gateway",
                    "purpose": "Corporate", "is_default": false, "subnet": null,
                    "gateway_ip": null, "isolation_enabled": false,
                    "internet_access_enabled": true, "mdns_forwarding_enabled": false,
                    "ipv6_enabled": false, "ipv6_mode": null, "ipv6_prefix": null,
                    "dhcpv6_enabled": false, "slaac_enabled": false, "ntp_server": null,
                    "pxe_enabled": false, "tftp_server": null, "firewall_zone_id": null,
                    "cellular_backup_enabled": false, "origin": null, "dhcp": null
                }))
                .expect("network fixture"),
            ),
        ])))
        .expect("networks update");
        app.switch_screen(ScreenId::Networks).expect("switch");
        app
    }

    #[test]
    fn an_esc_a_screen_uses_does_not_also_go_back() {
        let press = |code| KeyEvent::new(code, KeyModifiers::NONE);

        // Collapsing a network detail.
        let mut app = networks_app();
        app.handle_key_event(press(KeyCode::Enter)).expect("expand");
        let action = app.handle_key_event(press(KeyCode::Esc)).expect("collapse");
        assert!(!matches!(action, Some(Action::GoBack)), "{action:?}");

        // Cancelling the edit form.
        let mut app = networks_app();
        app.handle_key_event(press(KeyCode::Char('e')))
            .expect("edit");
        let action = app.handle_key_event(press(KeyCode::Esc)).expect("cancel");
        assert!(action.is_none(), "{action:?}");

        // Editing opened the detail too; the next Esc collapses it, and with
        // nothing left to close, Esc goes back as before.
        let action = app.handle_key_event(press(KeyCode::Esc)).expect("collapse");
        assert!(matches!(action, Some(Action::Invalidate)), "{action:?}");
        let action = app.handle_key_event(press(KeyCode::Esc)).expect("back");
        assert!(matches!(action, Some(Action::GoBack)), "{action:?}");
    }

    #[test]
    fn the_chrome_holds_still_while_a_form_takes_typed_input() {
        let mut app = networks_app();
        app.handle_key_event(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE))
            .expect("edit");
        let rows = render(&mut app, 120);

        for (marker, label) in [
            ("settings", "quit"),
            ("settings", "help"),
            ("6 Topo", "2 Devices"),
        ] {
            let action = app
                .handle_mouse_event(click_label(&rows, marker, label))
                .expect("click");
            assert!(action.is_none(), "{label}: {action:?}");
        }
        assert!(app.running);
    }

    fn device(id: &str, name: &str) -> std::sync::Arc<unifly_api::Device> {
        std::sync::Arc::new(
            serde_json::from_value(serde_json::json!({
                "id": id, "mac": format!("aa:bb:cc:dd:ee:{id}"), "ip": null,
                "wan_ipv6": null, "name": name, "model": "USW Pro 24",
                "device_type": "Switch", "state": "Online", "firmware_version": null,
                "firmware_updatable": false, "adopted_at": null, "provisioned_at": null,
                "last_seen": null, "serial": null, "supported": true, "ports": [],
                "radios": [], "uplink_device_id": null, "uplink_device_mac": null,
                "uplink_port_idx": null, "has_switching": true,
                "has_access_point": false, "stats": {}, "client_count": null,
                "origin": null
            }))
            .expect("device fixture"),
        )
    }

    /// Press a key and process the action it yields, as the event loop does.
    fn press(app: &mut App, code: KeyCode) {
        if let Some(action) = app
            .handle_key_event(KeyEvent::new(code, KeyModifiers::NONE))
            .expect("key handling")
        {
            app.process_action(&action).expect("action handling");
        }
    }

    #[test]
    fn enter_keeps_the_search_filter_and_esc_clears_it() {
        let mut app = dashboard_app();
        app.process_action(&Action::DevicesUpdated(std::sync::Arc::new(vec![
            device("01", "sw-alpha"),
            device("02", "sw-beta"),
        ])))
        .expect("devices update");
        app.switch_screen(ScreenId::Devices).expect("switch");
        let shown = |app: &mut App| render(app, 120).join("\n");

        press(&mut app, KeyCode::Char('/'));
        for c in "alp".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert!(!app.search_active, "Enter closes the search bar");
        let rows = shown(&mut app);
        assert!(
            rows.contains("sw-alpha") && !rows.contains("sw-beta"),
            "{rows}"
        );

        // Reopening search picks up the filter in place, ready to refine.
        press(&mut app, KeyCode::Char('/'));
        assert!(app.search_active);
        assert_eq!(app.search_query, "alp");

        press(&mut app, KeyCode::Esc);
        assert!(!app.search_active);
        let rows = shown(&mut app);
        assert!(
            rows.contains("sw-alpha") && rows.contains("sw-beta"),
            "{rows}"
        );
        press(&mut app, KeyCode::Char('/'));
        assert_eq!(app.search_query, "", "Esc cleared the filter");
    }

    #[test]
    fn search_is_offered_only_where_it_filters() {
        let mut app = dashboard_app();
        let status_bar = |app: &mut App| render(app, 120).last().cloned().unwrap_or_default();
        assert!(!status_bar(&mut app).contains("search"));

        press(&mut app, KeyCode::Char('/'));
        assert!(!app.search_active, "search opened on the Dashboard");
        assert!(app.notification.is_some(), "the refusal says why");

        for screen in [ScreenId::Devices, ScreenId::Clients, ScreenId::Wifi] {
            app.switch_screen(screen).expect("switch");
            assert!(status_bar(&mut app).contains("search"), "{screen}");
        }
    }

    #[test]
    fn the_help_overlay_lists_only_keys_that_work_everywhere() {
        let mut app = dashboard_app();
        app.help_visible = true;
        let text = render_lines(120, 34, |frame, _| app.render(frame)).join("\n");

        for screen_key in ["Sort column", "Filter"] {
            assert!(!text.contains(screen_key), "{screen_key}:\n{text}");
        }
        assert!(text.contains("Search lists"), "{text}");
        assert!(text.contains("own keys"), "{text}");
    }

    #[test]
    fn search_input_updates_query_and_can_close() {
        let mut app = App::new(None, None, false, true);
        app.active_screen = ScreenId::Dashboard;
        app.search_active = true;

        let action = app
            .handle_key_event(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE))
            .expect("search key handling should succeed");
        assert!(matches!(action, Some(Action::SearchInput(ref q)) if q == "u"));
        assert_eq!(app.search_query, "u");

        let action = app
            .handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .expect("search close should succeed");
        assert!(matches!(action, Some(Action::CloseSearch)));
        assert_eq!(app.search_query, "");
    }

    #[test]
    fn onboarding_input_reopens_render_gate() {
        let mut app = App::new(None, None, false, true);
        app.needs_redraw = false;

        let action = app
            .handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .expect("onboarding key handling should succeed");

        assert!(action.is_none());
        assert!(app.should_draw());
    }
}
