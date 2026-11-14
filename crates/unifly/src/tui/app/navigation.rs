use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use super::App;
use crate::tui::action::{Action, Notification};
use crate::tui::screen::ScreenId;
use crate::tui::widgets::click::Click;
use crate::tui::widgets::line_input::Edit;

/// Whether a key is a Ctrl or Alt chord. AltGr reports as both and types
/// an ordinary character, so it is not one.
fn is_chord(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) != key.modifiers.contains(KeyModifiers::ALT)
}

fn is_ctrl(key: KeyEvent, ch: char) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL)
        && !key.modifiers.contains(KeyModifiers::ALT)
        && key.code == KeyCode::Char(ch)
}

/// The key a list already answers for a paging key: screens bind the vim
/// keys, so PgUp, PgDn, Home, and End reach them as those.
fn list_alias(key: KeyEvent) -> KeyEvent {
    match key.code {
        KeyCode::PageDown => KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
        KeyCode::PageUp => KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        KeyCode::Home => KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        KeyCode::End => KeyEvent::new(KeyCode::Char('G'), KeyModifiers::NONE),
        _ => key,
    }
}

impl App {
    /// Map a key event to an action. Global keys are handled here;
    /// screen-specific keys are delegated to the active screen component.
    pub(super) fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        self.needs_redraw = true;

        // Raw mode leaves no SIGINT, so Ctrl+C has to quit from every state,
        // ahead of any overlay, field, or screen that would read a plain `c`.
        if is_ctrl(key, 'c') {
            return Ok(Some(Action::Quit));
        }
        if is_ctrl(key, 'l') {
            return Ok(Some(Action::Repaint));
        }

        // F1 is help where `?` cannot be: a form would type it.
        let modal = self.pending_confirm.is_some() || self.prompt.is_some();
        if key.code == KeyCode::F(1) && !modal {
            return Ok(Some(Action::ToggleHelp));
        }

        if self.pending_confirm.is_some() {
            if is_chord(key) {
                return Ok(None);
            }
            return Ok(match key.code {
                KeyCode::Char('y' | 'Y') => Some(Action::ConfirmYes),
                KeyCode::Char('n' | 'N') | KeyCode::Esc => Some(Action::ConfirmNo),
                _ => None,
            });
        }

        if self.prompt.is_some() {
            return Ok(self.handle_prompt_key_event(key));
        }

        if self.help_visible {
            return Ok(match key.code {
                KeyCode::Esc | KeyCode::Char('?' | 'q') if !is_chord(key) => {
                    Some(Action::ToggleHelp)
                }
                _ => None,
            });
        }

        if self.notices_visible {
            return Ok(match key.code {
                KeyCode::Esc | KeyCode::Char('n' | 'q') if !is_chord(key) => {
                    Some(Action::ToggleNotices)
                }
                _ => None,
            });
        }

        // Setup and Settings take every remaining key: no global shortcut
        // may fire while their fields have the keyboard. The dialogs and
        // overlays above come first, so a question one of them asks can
        // be answered.
        if self.fills_frame() {
            return self.forward_key_to_active_screen(key);
        }

        if self.search_active {
            return self.handle_search_key_event(key);
        }

        if self.about_visible {
            if is_chord(key) {
                return Ok(None);
            }
            return Ok(match key.code {
                KeyCode::Esc | KeyCode::Char('a' | 'q') => Some(Action::ToggleAbout),
                KeyCode::Char('d') => Some(Action::OpenDonate),
                KeyCode::Char('g') => {
                    super::dispatch::open_url("https://github.com/hyperb1iss/unifly");
                    None
                }
                _ => None,
            });
        }

        // Read before forwarding: a key that closes the form (Esc) must not
        // go on to a global shortcut either.
        let typing = self.active_screen_captures_input();
        // A refresh under a form could swap the record being edited.
        if is_ctrl(key, 'r') && !typing {
            return Ok(Some(Action::Refresh));
        }
        // A chord is never a screen's plain letter: Ctrl+b must not block a
        // client because `b` does. A form reads its own editing chords, and
        // the lists page on Ctrl+d and Ctrl+u.
        let paging = is_ctrl(key, 'd') || is_ctrl(key, 'u');
        if (typing || paging || !is_chord(key))
            && let Ok(Some(action)) = self.forward_key_to_active_screen(key)
        {
            return Ok(Some(action));
        }

        if typing {
            return Ok(None);
        }

        Ok(self.handle_global_key_event(key))
    }

    /// Route pasted text to whatever is taking typed input. Anywhere else
    /// it is dropped: a paste must never run as a burst of shortcut keys.
    pub(super) fn handle_paste(&mut self, text: &str) -> Option<Action> {
        self.needs_redraw = true;
        if self.pending_confirm.is_some() {
            return None;
        }
        if let Some((_, input)) = &mut self.prompt {
            input.paste(text, |_| true);
            return None;
        }
        if self.search_active {
            return self
                .search_input
                .paste(text, |_| true)
                .consumed()
                .then(|| Action::SearchInput(self.search_input.text().to_owned()));
        }
        if self.help_visible || self.about_visible || self.notices_visible {
            return None;
        }
        let screen = self.screens.get_mut(&self.active_screen)?;
        if !screen.captures_input() {
            return None;
        }
        screen.handle_paste(text)
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
            || self.prompt.is_some()
            || self.search_active
            || self.help_visible
            || self.about_visible
            || self.notices_visible
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
                self.search_input.set(query);
            }
            None => self.show_notification(Notification::info(
                "Search filters the Devices, Clients, Events, and WiFi lists",
            )),
        }
    }

    /// Whether search filters anything on the active screen.
    pub(super) fn active_screen_searches(&self) -> bool {
        self.screens
            .get(&self.active_screen)
            .is_some_and(|screen| screen.search_query().is_some())
    }

    /// The filter the active screen is keeping, when it has one.
    fn active_filter(&self) -> Option<&str> {
        self.screens
            .get(&self.active_screen)
            .and_then(|screen| screen.search_query())
            .filter(|query| !query.is_empty())
    }

    /// Whether the active screen is taking typed input, such as a form.
    pub(super) fn active_screen_captures_input(&self) -> bool {
        self.screens
            .get(&self.active_screen)
            .is_some_and(|screen| screen.captures_input())
    }

    /// Keys while a prompt is open: Enter hands its text to the active
    /// screen, Esc drops it, and everything else edits the line.
    fn handle_prompt_key_event(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc => {
                self.prompt = None;
                None
            }
            // A secret is taken exactly as typed: a passphrase may begin
            // or end with a space.
            KeyCode::Enter => self.prompt.take().map(|(request, input)| {
                let text = if request.secret {
                    input.text()
                } else {
                    input.text().trim()
                };
                Action::PromptSubmitted(text.to_owned())
            }),
            // A masked value typed blind can lock a network out: Ctrl+T
            // shows it, and hides it again.
            KeyCode::Char('t') if is_ctrl(key, 't') => {
                if self
                    .prompt
                    .as_ref()
                    .is_some_and(|(request, _)| request.secret)
                {
                    self.prompt_revealed = !self.prompt_revealed;
                }
                None
            }
            _ => {
                if let Some((_, input)) = &mut self.prompt {
                    input.handle_key(key);
                }
                None
            }
        }
    }

    fn handle_search_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match key.code {
            KeyCode::Esc => {
                self.search_input.clear();
                return Ok(Some(Action::CloseSearch));
            }
            KeyCode::Enter => return Ok(Some(Action::SearchSubmit)),
            // The filtered list stays movable while the bar is open.
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
                return self.forward_key_to_active_screen(key);
            }
            _ => {}
        }
        Ok(match self.search_input.handle_key(key) {
            Edit::Changed => Some(Action::SearchInput(self.search_input.text().to_owned())),
            Edit::Moved | Edit::Ignored => None,
        })
    }

    /// What `Esc` does once the screen has nothing of its own to close: it
    /// dismisses a warning or error toast, then clears a kept search
    /// filter, then goes back a screen.
    fn escape_action(&self) -> Action {
        if self.has_sticky_notification() {
            Action::DismissNotification
        } else if self.active_filter().is_some() {
            Action::CloseSearch
        } else {
            Action::GoBack
        }
    }

    fn handle_global_key_event(&self, key: KeyEvent) -> Option<Action> {
        // Terminals differ on whether Shift is reported with a shifted
        // character (`?` on Windows, digits on AZERTY), so only the key
        // itself is matched.
        if is_chord(key) {
            return None;
        }
        match key.code {
            KeyCode::Char('q') => Some(Action::Quit),
            KeyCode::Char('?') => Some(Action::ToggleHelp),
            KeyCode::Char('a') => Some(Action::ToggleAbout),
            KeyCode::Char('n') => Some(Action::ToggleNotices),
            KeyCode::Char('/') => Some(Action::OpenSearch),
            KeyCode::Char(',') => Some(Action::OpenSettings),
            KeyCode::Char(c @ '1'..='9') => {
                #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
                let n = c.to_digit(10).unwrap_or(0) as u8;
                ScreenId::from_number(n).map(Action::SwitchScreen)
            }
            KeyCode::Tab => Some(Action::SwitchScreen(self.active_screen.next())),
            KeyCode::BackTab => Some(Action::SwitchScreen(self.active_screen.prev())),
            KeyCode::Esc => Some(self.escape_action()),
            _ => None,
        }
    }

    fn forward_key_to_active_screen(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        if let Some(screen) = self.screens.get_mut(&self.active_screen) {
            let key = if screen.captures_input() {
                key
            } else {
                list_alias(key)
            };
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
            .handle_mouse_event(click_label(&rows, "v alarms", "alarms"))
            .expect("click handling");
        let Some(Action::PressKey(key)) = action else {
            panic!("expected a key press, got {action:?}");
        };
        assert_eq!(key, crate::tui::widgets::click::char_key('v'));

        // The screen takes the key as if it had been typed: `v` opens
        // its alarms view.
        app.process_action(&Action::PressKey(key))
            .expect("key press handling");
        let queued = app.action_rx.try_recv().expect("the press dispatches");
        assert!(matches!(queued, Action::Invalidate), "{queued:?}");
        assert!(render(&mut app, 120)[0].contains("Alarms (0)"));
        app.process_action(&Action::PressKey(key))
            .expect("key press handling");
        app.action_rx.try_recv().expect("the press dispatches");

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
            app.handle_mouse_event(click_label(&rows, "Everywhere", "Everywhere"))
                .expect("click")
                .is_none(),
            "a tap inside the overlay does nothing"
        );
        let action = tap(&mut app, "to close", "Esc to close");
        assert!(matches!(action, Some(Action::ToggleHelp)), "{action:?}");
        app.help_visible = false;

        app.about_visible = true;
        let action = tap(&mut app, "onate", "onate");
        assert!(matches!(action, Some(Action::OpenDonate)), "{action:?}");
        let action = tap(&mut app, "to close", "Esc or a to close");
        assert!(matches!(action, Some(Action::ToggleAbout)), "{action:?}");
        app.about_visible = false;

        app.pending_confirm = Some(crate::tui::action::ConfirmRequest::new(
            "Restart sw-core?",
            Action::Refresh,
        ));
        let action = tap(&mut app, "confirm", "confirm");
        assert!(matches!(action, Some(Action::ConfirmYes)), "{action:?}");
        let action = tap(&mut app, "6 Topo", "2 Devices");
        assert!(
            matches!(action, Some(Action::ConfirmNo)),
            "a tap outside answers no: {action:?}"
        );
    }

    #[test]
    fn the_sponsor_button_gives_its_corner_to_screen_actions() {
        let mut app = dashboard_app();
        app.process_action(&Action::DevicesUpdated(std::sync::Arc::new(vec![device(
            "01", "sw-alpha",
        )])))
        .expect("devices update");
        app.switch_screen(ScreenId::Devices).expect("switch");
        let status =
            |app: &mut App, width: u16| render(app, width).last().cloned().unwrap_or_default();

        // With room for everything, the button keeps the corner.
        let wide = status(&mut app, 120);
        assert!(
            wide.contains("Enter detail") && wide.contains("Sponsor"),
            "{wide}"
        );

        // Where it would cost an action, the action wins, and the way to
        // help stays.
        let narrow = status(&mut app, 64);
        assert!(narrow.contains("Enter detail"), "{narrow}");
        assert!(!narrow.contains("Sponsor"), "{narrow}");
        assert!(narrow.contains("? help"), "{narrow}");
    }

    #[test]
    fn the_status_bar_and_toasts_take_taps() {
        let mut app = dashboard_app();
        let rows = render(&mut app, 120);
        let action = app
            .handle_mouse_event(click_label(&rows, "? help", "help"))
            .expect("click");
        assert!(matches!(action, Some(Action::ToggleHelp)), "{action:?}");
        let action = app
            .handle_mouse_event(click_label(&rows, "? help", "Sponsor"))
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
            matches!(action, Some(Action::DismissToast(_))),
            "{action:?}"
        );

        app.notices.dismiss_sticky();
        app.search_active = true;
        app.search_input.set("ap");
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

        // The bar offers only the keys a form leaves working.
        let bar = rows.last().cloned().unwrap_or_default();
        assert!(bar.contains("F1 help") && bar.contains("^c quit"), "{bar}");
        assert!(
            !bar.contains("settings") && !bar.contains("refresh"),
            "{bar}"
        );

        for (marker, label) in [
            ("F1 help", "quit"),
            ("F1 help", "help"),
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
        assert_eq!(app.search_input.text(), "alp");

        press(&mut app, KeyCode::Esc);
        assert!(!app.search_active);
        let rows = shown(&mut app);
        assert!(
            rows.contains("sw-alpha") && rows.contains("sw-beta"),
            "{rows}"
        );
        press(&mut app, KeyCode::Char('/'));
        assert_eq!(app.search_input.text(), "", "Esc cleared the filter");
    }

    #[test]
    fn search_is_offered_only_where_it_filters() {
        let mut app = dashboard_app();
        let status_bar = |app: &mut App| render(app, 120).last().cloned().unwrap_or_default();
        assert!(!status_bar(&mut app).contains("search"));

        press(&mut app, KeyCode::Char('/'));
        assert!(!app.search_active, "search opened on the Dashboard");
        assert!(app.notices.latest().is_some(), "the refusal says why");

        for screen in [ScreenId::Devices, ScreenId::Clients, ScreenId::Wifi] {
            app.switch_screen(screen).expect("switch");
            assert!(status_bar(&mut app).contains("search"), "{screen}");
        }
    }

    #[test]
    fn the_help_overlay_fits_a_24_row_terminal_whole() {
        let mut app = dashboard_app();
        app.help_visible = true;
        let text = render_lines(80, 24, |frame, _| app.render(frame)).join("\n");

        for needed in [
            "Dashboard screen",
            "Everywhere",
            "Notifications",
            "Refresh now, or retry",
            "Select text",
            "Esc to close",
        ] {
            assert!(text.contains(needed), "{needed}:\n{text}");
        }
    }

    #[test]
    fn the_help_overlay_lists_search_only_where_it_filters_and_wraps_long_rows() {
        let mut app = dashboard_app();
        app.help_visible = true;
        let text = render_lines(80, 24, |frame, _| app.render(frame)).join("\n");
        assert!(!text.contains("Search the list"), "{text}");

        app.active_screen = ScreenId::Devices;
        let text = render_lines(80, 24, |frame, _| app.render(frame)).join("\n");
        assert!(text.contains("Search the list"), "{text}");

        // A description longer than its column continues on the next row.
        let lines = super::super::render::help_column_text(
            &[("x", "Archive the selected alarm and reload the list")],
            38,
        );
        assert_eq!(
            lines,
            [
                "  x           Archive the selected",
                "              alarm and reload the",
                "              list"
            ]
        );
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn ctrl_c_quits_from_every_state() {
        let ctrl_c = key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let quits = |app: &mut App, state: &str| {
            let action = app.handle_key_event(ctrl_c).expect("key");
            assert!(matches!(action, Some(Action::Quit)), "{state}: {action:?}");
        };

        let mut app = dashboard_app();
        // WiFi Overview binds a plain `c`.
        app.active_screen = ScreenId::Wifi;
        quits(&mut app, "WiFi overview");
        app.search_active = true;
        quits(&mut app, "search");
        assert_eq!(app.search_input.text(), "", "nothing was typed");
        app.search_active = false;
        app.help_visible = true;
        quits(&mut app, "help");
        app.help_visible = false;
        app.about_visible = true;
        quits(&mut app, "about");
        app.about_visible = false;
        app.pending_confirm = Some(crate::tui::action::ConfirmRequest::new(
            "Restart sw-core?",
            Action::Refresh,
        ));
        quits(&mut app, "confirm");
    }

    #[test]
    fn shifted_keys_work_whether_or_not_the_terminal_reports_shift() {
        for modifiers in [KeyModifiers::NONE, KeyModifiers::SHIFT] {
            let mut app = dashboard_app();
            let action = app
                .handle_key_event(key(KeyCode::Char('?'), modifiers))
                .expect("key");
            assert!(matches!(action, Some(Action::ToggleHelp)), "{modifiers:?}");
            let action = app
                .handle_key_event(key(KeyCode::Char('2'), modifiers))
                .expect("key");
            assert!(
                matches!(action, Some(Action::SwitchScreen(ScreenId::Devices))),
                "{modifiers:?}"
            );
        }
    }

    #[test]
    fn a_chord_never_acts_as_its_plain_letter() {
        // `b` blocks a client and `x` kicks one; their chords must not.
        let mut app = dashboard_app();
        app.active_screen = ScreenId::Clients;
        for (ch, modifiers) in [
            ('b', KeyModifiers::CONTROL),
            ('x', KeyModifiers::CONTROL),
            ('q', KeyModifiers::ALT),
            ('a', KeyModifiers::CONTROL),
        ] {
            let action = app
                .handle_key_event(key(KeyCode::Char(ch), modifiers))
                .expect("key");
            assert!(action.is_none(), "{modifiers:?}+{ch}: {action:?}");
        }
        assert!(app.running);

        // A confirmation is not answered by a chord either.
        app.pending_confirm = Some(crate::tui::action::ConfirmRequest::new(
            "Restart sw-core?",
            Action::Refresh,
        ));
        let action = app
            .handle_key_event(key(KeyCode::Char('y'), KeyModifiers::CONTROL))
            .expect("key");
        assert!(action.is_none(), "{action:?}");
    }

    #[test]
    fn a_paste_types_into_search_and_is_dropped_elsewhere() {
        let mut app = dashboard_app();
        app.active_screen = ScreenId::Firewall;
        // `J` and `K` reorder policies; a paste must never press them.
        assert!(app.handle_paste("JKR\ny").is_none());
        assert!(app.action_rx.try_recv().is_err());

        app.active_screen = ScreenId::Devices;
        app.search_active = true;
        let action = app.handle_paste("sw-\ncore\r");
        assert!(
            matches!(action, Some(Action::SearchInput(ref query)) if query == "sw-core"),
            "{action:?}"
        );
        assert!(app.search_active, "a pasted newline does not submit");
    }

    #[test]
    fn the_search_bar_edits_at_a_cursor() {
        let mut app = dashboard_app();
        app.active_screen = ScreenId::Devices;
        app.search_active = true;
        for code in [
            KeyCode::Char('a'),
            KeyCode::Char('c'),
            KeyCode::Left,
            KeyCode::Char('b'),
        ] {
            app.handle_key_event(key(code, KeyModifiers::NONE))
                .expect("key");
        }
        assert_eq!(app.search_input.text(), "abc");
        let action = app
            .handle_key_event(key(KeyCode::Char('u'), KeyModifiers::CONTROL))
            .expect("key");
        assert!(
            matches!(action, Some(Action::SearchInput(ref query)) if query == "c"),
            "{action:?}"
        );
    }

    #[test]
    fn esc_dismisses_an_error_then_clears_the_filter_then_goes_back_once() {
        let mut app = dashboard_app();
        app.process_action(&Action::DevicesUpdated(std::sync::Arc::new(vec![device(
            "01", "sw-alpha",
        )])))
        .expect("devices");
        app.switch_screen(ScreenId::Devices).expect("switch");
        press(&mut app, KeyCode::Char('/'));
        press(&mut app, KeyCode::Char('s'));
        press(&mut app, KeyCode::Enter);
        app.show_notification(Notification::error("the controller said no"));

        let esc = |app: &mut App| {
            app.handle_key_event(key(KeyCode::Esc, KeyModifiers::NONE))
                .expect("key")
        };
        let first = esc(&mut app);
        assert!(
            matches!(first, Some(Action::DismissNotification)),
            "{first:?}"
        );
        app.process_action(&Action::DismissNotification)
            .expect("dismiss");
        let second = esc(&mut app);
        assert!(matches!(second, Some(Action::CloseSearch)), "{second:?}");
        app.process_action(&Action::CloseSearch).expect("clear");
        let third = esc(&mut app);
        assert!(matches!(third, Some(Action::GoBack)), "{third:?}");
        app.process_action(&Action::GoBack).expect("back");
        assert_eq!(app.active_screen, ScreenId::Dashboard);

        // Back does not arm another back: Esc would bounce otherwise.
        app.process_action(&Action::GoBack).expect("back again");
        assert_eq!(app.active_screen, ScreenId::Dashboard);
    }

    #[test]
    fn a_prompt_takes_typed_and_pasted_text_and_hands_it_to_the_screen() {
        let mut app = dashboard_app();
        app.process_action(&Action::OpenPrompt(crate::tui::action::PromptRequest {
            title: "Fixed IP for nas-vault".into(),
            label: "IPv4 address".into(),
            initial: "10.40.20.".into(),
            ..Default::default()
        }))
        .expect("open");
        let rows = render(&mut app, 100);
        let text = rows.join("\n");
        assert!(text.contains("Fixed IP for nas-vault"), "{text}");
        assert!(text.contains("10.40.20."), "{text}");

        // `q` types; it does not quit. A paste goes into the field.
        let typed = app
            .handle_key_event(key(KeyCode::Char('q'), KeyModifiers::NONE))
            .expect("key");
        assert!(typed.is_none());
        app.handle_key_event(key(KeyCode::Backspace, KeyModifiers::NONE))
            .expect("key");
        assert!(app.handle_paste("5\n").is_none());
        let submitted = app
            .handle_key_event(key(KeyCode::Enter, KeyModifiers::NONE))
            .expect("key");
        assert!(
            matches!(submitted, Some(Action::PromptSubmitted(ref text)) if text == "10.40.20.5"),
            "{submitted:?}"
        );
        assert!(app.prompt.is_none());

        // A secret is masked until Ctrl+T shows it.
        app.process_action(&Action::OpenPrompt(crate::tui::action::PromptRequest {
            title: "Passphrase for Guest".into(),
            initial: "hunter22".into(),
            secret: true,
            ..Default::default()
        }))
        .expect("open");
        let masked = render(&mut app, 100).join("\n");
        assert!(!masked.contains("hunter22"), "{masked}");
        assert!(masked.contains("^t show"), "{masked}");
        app.handle_key_event(key(KeyCode::Char('t'), KeyModifiers::CONTROL))
            .expect("key");
        let shown = render(&mut app, 100).join("\n");
        assert!(shown.contains("hunter22"), "{shown}");
        app.handle_key_event(key(KeyCode::Esc, KeyModifiers::NONE))
            .expect("key");

        // A secret keeps the spaces around it; anything else is trimmed.
        for (secret, expected) in [(true, " pass phrase "), (false, "pass phrase")] {
            app.process_action(&Action::OpenPrompt(crate::tui::action::PromptRequest {
                initial: " pass phrase ".into(),
                secret,
                ..Default::default()
            }))
            .expect("open");
            let submitted = app
                .handle_key_event(key(KeyCode::Enter, KeyModifiers::NONE))
                .expect("key");
            assert!(
                matches!(submitted, Some(Action::PromptSubmitted(ref text)) if text == expected),
                "secret {secret}: {submitted:?}"
            );
        }

        app.process_action(&Action::OpenPrompt(
            crate::tui::action::PromptRequest::default(),
        ))
        .expect("open");
        let cancelled = app
            .handle_key_event(key(KeyCode::Esc, KeyModifiers::NONE))
            .expect("key");
        assert!(cancelled.is_none() && app.prompt.is_none());
    }

    #[test]
    fn dialogs_and_f1_help_work_over_the_setup_wizard() {
        // With no controller the app opens on the wizard, which otherwise
        // takes every key for its fields.
        let mut app = App::new(None, None, false, true);
        assert_eq!(app.active_screen, ScreenId::Setup);

        let help = app
            .handle_key_event(key(KeyCode::F(1), KeyModifiers::NONE))
            .expect("key");
        assert!(matches!(help, Some(Action::ToggleHelp)), "{help:?}");
        app.process_action(&Action::ToggleHelp).expect("open help");
        let text = render_lines(100, 30, |frame, _| app.render(frame)).join("\n");
        assert!(text.contains("Keyboard Shortcuts"), "{text}");
        assert!(text.contains("Setup screen"), "{text}");
        assert!(!text.contains("Jump to screen"), "{text}");
        let close = app
            .handle_key_event(key(KeyCode::Esc, KeyModifiers::NONE))
            .expect("key");
        assert!(matches!(close, Some(Action::ToggleHelp)), "{close:?}");
        app.process_action(&Action::ToggleHelp).expect("close help");

        // A question asked over the wizard is drawn and can be answered.
        app.pending_confirm = Some(crate::tui::action::ConfirmRequest::new(
            "Discard your changes?",
            Action::Refresh,
        ));
        let text = render_lines(100, 30, |frame, _| app.render(frame)).join("\n");
        assert!(text.contains("Discard your changes?"), "{text}");
        let yes = app
            .handle_key_event(key(KeyCode::Char('y'), KeyModifiers::NONE))
            .expect("key");
        assert!(matches!(yes, Some(Action::ConfirmYes)), "{yes:?}");
    }

    #[test]
    fn paging_keys_reach_lists_as_their_vim_keys() {
        let page_down = list_alias(key(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(page_down, key(KeyCode::Char('d'), KeyModifiers::CONTROL));
        let end = list_alias(key(KeyCode::End, KeyModifiers::NONE));
        assert_eq!(end.code, KeyCode::Char('G'));
        let plain = key(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(list_alias(plain), plain);
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
        assert_eq!(app.search_input.text(), "u");

        let action = app
            .handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .expect("search close should succeed");
        assert!(matches!(action, Some(Action::CloseSearch)));
        assert_eq!(app.search_input.text(), "");
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
