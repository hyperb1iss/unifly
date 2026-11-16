use super::ClientsScreen;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};

use crate::tui::action::{Action, ClientTypeFilter};
use crate::tui::widgets::click::{self, Click};

/// Rows a page key moves.
const PAGE: isize = 10;

impl ClientsScreen {
    pub(super) fn handle_key_input(&mut self, key: KeyEvent) -> Option<Action> {
        if let Some(action) = self.handle_command_key(key) {
            return Some(action);
        }
        if self.handle_movement(key) {
            // With a detail open, the pane follows the cursor.
            if self.detail_open {
                self.open_selected_detail();
            }
            return None;
        }
        match key.code {
            // The filter changes what the list holds, so a detail closes.
            KeyCode::Char('l') | KeyCode::Right => {
                return self.apply_filter(self.filter_after(1));
            }
            KeyCode::Char('h') | KeyCode::Left => {
                return self.apply_filter(self.filter_after(-1));
            }
            KeyCode::Esc if self.detail_open => {
                self.close_detail();
                return Some(Action::CloseDetail);
            }
            KeyCode::Char('s') if !self.detail_open => self.set_sort(self.sort.next()),
            KeyCode::Char('r') if !self.detail_open => self.set_sort(self.sort.reversed()),
            KeyCode::Enter if !self.detail_open => self.open_selected_detail(),
            _ => {}
        }
        None
    }

    /// Commands that act on a client, in the table and in the detail.
    fn handle_command_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Char('b') => self.request_block_toggle(),
            KeyCode::Char('x') => self.request_kick(),
            KeyCode::Char('F') => self.request_forget(),
            KeyCode::Char('A') => self.request_guest_access(),
            KeyCode::Char('I') => self.request_fixed_ip(),
            _ => None,
        }
    }

    /// Move the cursor for a list movement key: `j`/`k` and the arrows,
    /// `g`/`G`, and the page keys, which arrive as `Ctrl+d`/`Ctrl+u`.
    /// Returns whether `key` was one.
    fn handle_movement(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('d') if ctrl => self.move_selection(PAGE),
            KeyCode::Char('u') if ctrl => self.move_selection(-PAGE),
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
            KeyCode::Char('g') => self.select(0),
            KeyCode::Char('G') => self.select(usize::MAX),
            _ => return false,
        }
        true
    }

    /// Mouse and touch input: filters and rows by click, the list by wheel.
    pub(super) fn handle_mouse_input(&mut self, mouse: MouseEvent) -> Option<Action> {
        if let Some(delta) = click::wheel_delta(mouse) {
            self.move_selection(delta);
            if self.detail_open {
                self.open_selected_detail();
            }
            return None;
        }
        let target = self.hits.borrow().clicked(mouse)?;
        match target {
            Click::Tab(index) => ClientTypeFilter::ALL
                .get(index)
                .and_then(|filter| self.apply_filter(*filter)),
            Click::Row(index) => {
                self.click_row(index);
                None
            }
            Click::Key(key) => Some(Action::PressKey(key)),
            _ => None,
        }
    }

    /// Select a type filter. An open detail closes, since its client may
    /// not pass the new filter.
    fn apply_filter(&mut self, filter: ClientTypeFilter) -> Option<Action> {
        if filter == self.filter {
            return None;
        }
        self.close_detail();
        self.set_filter(filter)
    }

    /// A row selects its client, and a second tap opens it, like `Enter`.
    /// With a detail open, the detail follows the tapped client.
    fn click_row(&mut self, index: usize) {
        let again = self.selected_index() == index;
        self.select(index);
        if self.detail_open || again {
            self.open_selected_detail();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEventKind};
    use serde_json::json;

    use crate::tui::action::{Action, ClientTypeFilter};
    use crate::tui::component::Component;
    use crate::tui::screens::clients::ClientsScreen;
    use crate::tui::screens::devices::fixtures::{client, wireless};
    use crate::tui::table::render_lines;
    use crate::tui::widgets::click::{click_label, mouse};

    fn render(screen: &ClientsScreen) -> Vec<String> {
        render_lines(100, 12, |frame, area| screen.render(frame, area))
    }

    /// alpha (wired), bravo (wireless), charlie (wired), in that order.
    fn screen() -> ClientsScreen {
        let mut screen = ClientsScreen::new();
        screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![
            client(1, "alpha", &json!({})),
            client(2, "bravo", &wireless(-55, "aa:bb:cc:dd:ee:01")),
            client(3, "charlie", &json!({})),
        ])));
        screen
    }

    fn press(screen: &mut ClientsScreen, code: KeyCode) -> Option<Action> {
        screen.handle_key_input(KeyEvent::from(code))
    }

    fn shown(screen: &ClientsScreen) -> Option<String> {
        let filtered = screen.filtered_clients();
        screen
            .detail_client(&filtered)
            .and_then(|client| client.name.clone())
    }

    #[test]
    fn clicking_a_type_filter_selects_it() {
        let mut screen = ClientsScreen::new();
        for (label, filter) in [
            ("Wireless", ClientTypeFilter::Wireless),
            ("Wired", ClientTypeFilter::Wired),
            ("VPN", ClientTypeFilter::Vpn),
            ("Guest", ClientTypeFilter::Guest),
            ("Blocked", ClientTypeFilter::Blocked),
            ("All", ClientTypeFilter::All),
        ] {
            let rows = render(&screen);
            screen.handle_mouse_input(click_label(&rows, "Wireless", label));
            assert_eq!(screen.filter, filter, "{label}");
        }
    }

    #[test]
    fn changing_the_filter_in_a_detail_closes_the_detail() {
        let mut screen = screen();
        press(&mut screen, KeyCode::Enter);
        assert!(screen.detail_open);

        press(&mut screen, KeyCode::Char('l'));

        assert_eq!(screen.filter, ClientTypeFilter::Wireless);
        assert!(!screen.detail_open);
    }

    #[test]
    fn the_filter_steps_both_ways_and_leaves_tab_to_the_app() {
        let mut screen = ClientsScreen::new();
        assert!(matches!(
            press(&mut screen, KeyCode::Left),
            Some(Action::RequestBlockedClients)
        ));
        assert_eq!(screen.filter, ClientTypeFilter::Blocked);
        press(&mut screen, KeyCode::Char('l'));
        assert_eq!(screen.filter, ClientTypeFilter::All);
        assert!(
            press(&mut screen, KeyCode::Tab).is_none(),
            "Tab switches screens everywhere"
        );
        assert_eq!(screen.filter, ClientTypeFilter::All);
    }

    #[test]
    fn a_filter_click_closes_an_open_detail() {
        let mut screen = screen();
        let rows = render(&screen);
        press(&mut screen, KeyCode::Enter);

        screen.handle_mouse_input(click_label(&rows, "Wireless", "Wired"));

        assert_eq!(screen.filter, ClientTypeFilter::Wired);
        assert!(!screen.detail_open);
    }

    #[test]
    fn with_a_detail_open_movement_steps_to_the_next_client() {
        let mut screen = screen();
        press(&mut screen, KeyCode::Enter);
        assert_eq!(shown(&screen).as_deref(), Some("alpha"));

        press(&mut screen, KeyCode::Char('j'));
        assert_eq!(shown(&screen).as_deref(), Some("bravo"));
        press(&mut screen, KeyCode::Char('G'));
        assert_eq!(shown(&screen).as_deref(), Some("charlie"));
        press(&mut screen, KeyCode::Up);
        assert_eq!(shown(&screen).as_deref(), Some("bravo"));
        screen.handle_key_input(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(shown(&screen).as_deref(), Some("alpha"));

        // The wheel does the same.
        screen.handle_mouse_input(mouse(MouseEventKind::ScrollDown, 10, 5));
        assert_eq!(shown(&screen).as_deref(), Some("bravo"));

        // Esc closes the detail and is used up; the next one is the app's.
        assert!(matches!(
            press(&mut screen, KeyCode::Esc),
            Some(Action::CloseDetail)
        ));
        assert!(!screen.detail_open);
        assert!(press(&mut screen, KeyCode::Esc).is_none());
    }

    #[test]
    fn s_cycles_the_sort_and_r_reverses_it_keeping_the_selected_client() {
        let mut screen = ClientsScreen::new();
        screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![
            client(1, "alpha", &json!({ "tx_bytes": 10, "rx_bytes": 10 })),
            client(2, "bravo", &wireless(-70, "aa:bb:cc:dd:ee:01")),
            client(3, "charlie", &wireless(-48, "aa:bb:cc:dd:ee:01")),
        ])));
        let names = |screen: &ClientsScreen| -> Vec<String> {
            screen
                .filtered_clients()
                .iter()
                .map(|client| client.name.clone().unwrap_or_default())
                .collect()
        };
        screen.select(1);

        press(&mut screen, KeyCode::Char('s'));
        assert_eq!(screen.sort.label(), "signal \u{2193}");
        // Strongest first; the wired client has no signal and goes last.
        assert_eq!(names(&screen), ["charlie", "bravo", "alpha"]);
        assert_eq!(screen.selected_client_id(), Some("client-2".into()));

        press(&mut screen, KeyCode::Char('r'));
        assert_eq!(names(&screen), ["bravo", "charlie", "alpha"]);

        press(&mut screen, KeyCode::Char('s'));
        assert_eq!(screen.sort.label(), "traffic \u{2193}");
        assert_eq!(names(&screen)[0], "alpha");
        assert_eq!(screen.selected_client_id(), Some("client-2".into()));
    }
}
