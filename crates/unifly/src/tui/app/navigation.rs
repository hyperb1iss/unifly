use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use super::{App, ConnectionStatus};
use crate::tui::action::Action;
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

        if let Ok(Some(action)) = self.forward_key_to_active_screen(key) {
            return Ok(Some(action));
        }

        let quit = key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c');
        if !quit
            && self
                .screens
                .get(&self.active_screen)
                .is_some_and(|screen| screen.captures_input())
        {
            return Ok(None);
        }

        if let Some(action) = self.handle_global_key_event(key) {
            return Ok(Some(action));
        }

        Ok(None)
    }

    /// Handle mouse events: the sponsor button and the screen tabs first,
    /// then the active screen.
    pub(super) fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        // Presses, drags, and wheel turns can change screen state without
        // dispatching an invalidating action (a selection, a settings
        // toggle). Bare pointer motion changes nothing and stays out.
        if !matches!(mouse.kind, MouseEventKind::Moved | MouseEventKind::Up(_)) {
            self.needs_redraw = true;
        }
        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind
            && let Some(action) = self.check_donate_click(mouse.column, mouse.row)
        {
            return Ok(Some(action));
        }

        // Overlays hold the input while they are up, as they do for keys.
        if self.pending_confirm.is_some()
            || self.search_active
            || self.help_visible
            || self.about_visible
        {
            return Ok(None);
        }

        if let Some(Click::Action(action)) = self.hits.clicked(mouse) {
            return Ok(Some(action));
        }

        if let Some(screen) = self.screens.get_mut(&self.active_screen) {
            return screen.handle_mouse_event(mouse);
        }
        Ok(None)
    }

    /// Check if a click landed on the sponsor button in the status bar.
    fn check_donate_click(&self, col: u16, row: u16) -> Option<Action> {
        if !self.show_donate || self.search_active {
            return None;
        }
        if matches!(self.active_screen, ScreenId::Setup | ScreenId::Settings) {
            return None;
        }

        let (w, h) = self.terminal_size;
        if h == 0 || w == 0 {
            return None;
        }

        let button_width = 12u16;
        let donate_x = w.saturating_sub(button_width);
        let status_y = h.saturating_sub(1);

        if row == status_y && col >= donate_x {
            Some(Action::OpenDonate)
        } else {
            None
        }
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

        app.help_visible = true;
        assert!(app.handle_mouse_event(devices).expect("click").is_none());
        app.help_visible = false;

        app.search_active = true;
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
