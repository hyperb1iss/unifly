use super::ClientsScreen;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};

use crate::tui::action::{Action, ClientTypeFilter};
use crate::tui::widgets::click::{self, Click};

impl ClientsScreen {
    pub(super) fn handle_key_input(&mut self, key: KeyEvent) -> Option<Action> {
        if self.detail_open {
            return self.handle_detail_key(key);
        }

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1);
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1);
                None
            }
            KeyCode::Char('g') => {
                self.select(0);
                Some(Action::ScrollToTop)
            }
            KeyCode::Char('G') => {
                let len = self.filtered_clients().len();
                if len > 0 {
                    self.select(len - 1);
                }
                Some(Action::ScrollToBottom)
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(10);
                Some(Action::PageDown)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(-10);
                Some(Action::PageUp)
            }
            KeyCode::Tab => {
                self.cycle_filter();
                Some(Action::FilterClientType(self.filter))
            }
            KeyCode::Enter => self.open_selected_detail(),
            KeyCode::Char('b') => self.selected_client_id().map(Action::RequestBlockClient),
            KeyCode::Char('B') => self.selected_client_id().map(Action::RequestUnblockClient),
            KeyCode::Char('x') => self.selected_client_id().map(Action::RequestKickClient),
            _ => None,
        }
    }

    pub(super) fn apply_action(&mut self, action: &Action) {
        match action {
            Action::ClientsUpdated(clients) => {
                self.apply_clients_update(clients);
            }
            Action::DevicesUpdated(devices) => {
                self.links.set_devices(devices);
            }
            Action::NetworksUpdated(networks) => {
                self.links.set_networks(networks);
            }
            Action::FilterClientType(filter) => {
                self.filter = *filter;
                self.reconcile_selection_after_view_change(true);
            }
            Action::SearchInput(query) => {
                self.search_query.clone_from(query);
                self.reconcile_selection_after_view_change(true);
            }
            Action::CloseSearch => {
                self.search_query.clear();
                self.reconcile_selection_after_view_change(true);
            }
            Action::CloseDetail => {
                self.detail_open = false;
                self.detail_client_id = None;
            }
            _ => {}
        }
    }

    /// Mouse and touch input: filters and rows by click, the list by wheel.
    pub(super) fn handle_mouse_input(&mut self, mouse: MouseEvent) -> Option<Action> {
        if let Some(delta) = click::wheel_delta(mouse) {
            self.move_selection(delta);
            return self
                .detail_open
                .then(|| self.open_selected_detail())
                .flatten();
        }
        let target = self.hits.borrow().clicked(mouse)?;
        match target {
            Click::Tab(index) => self.click_filter(index),
            Click::Row(index) => self.click_row(index),
            Click::Key(key) => Some(Action::PressKey(key)),
            _ => None,
        }
    }

    /// A type filter selects it, like `Tab`. An open detail closes, since
    /// its client may not pass the new filter.
    fn click_filter(&mut self, index: usize) -> Option<Action> {
        let filter = *ClientTypeFilter::ALL.get(index)?;
        if filter == self.filter {
            return None;
        }
        Some(self.apply_filter(filter))
    }

    fn apply_filter(&mut self, filter: ClientTypeFilter) -> Action {
        self.detail_open = false;
        self.detail_client_id = None;
        self.set_filter(filter);
        Action::FilterClientType(filter)
    }

    /// A row selects its client, and a second tap opens it, like `Enter`.
    /// With a detail open, the detail follows the tapped client.
    fn click_row(&mut self, index: usize) -> Option<Action> {
        let again = self.selected_index() == index;
        self.select(index);
        (self.detail_open || again)
            .then(|| self.open_selected_detail())
            .flatten()
    }

    fn open_selected_detail(&mut self) -> Option<Action> {
        let id = self.selected_client_id()?;
        self.detail_open = true;
        self.detail_client_id = Some(id.clone());
        Some(Action::OpenClientDetail(id))
    }

    fn handle_detail_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            // The filter changes what the list holds, so the detail closes.
            KeyCode::Tab => {
                self.cycle_filter();
                Some(self.apply_filter(self.filter))
            }
            KeyCode::Esc => {
                self.detail_open = false;
                self.detail_client_id = None;
                Some(Action::CloseDetail)
            }
            KeyCode::Char('b') => self
                .detail_action_client_id()
                .map(Action::RequestBlockClient),
            KeyCode::Char('B') => self
                .detail_action_client_id()
                .map(Action::RequestUnblockClient),
            KeyCode::Char('x') => self
                .detail_action_client_id()
                .map(Action::RequestKickClient),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::action::{Action, ClientTypeFilter};
    use crate::tui::component::Component;
    use crate::tui::screens::clients::ClientsScreen;
    use crate::tui::table::render_lines;
    use crate::tui::widgets::click::click_label;

    fn render(screen: &ClientsScreen) -> Vec<String> {
        render_lines(100, 12, |frame, area| screen.render(frame, area))
    }

    #[test]
    fn clicking_a_type_filter_selects_it() {
        let mut screen = ClientsScreen::new();
        for (label, filter) in [
            ("Wireless", ClientTypeFilter::Wireless),
            ("Wired", ClientTypeFilter::Wired),
            ("VPN", ClientTypeFilter::Vpn),
            ("Guest", ClientTypeFilter::Guest),
            ("All", ClientTypeFilter::All),
        ] {
            let rows = render(&screen);
            let action = screen.handle_mouse_input(click_label(&rows, "Wireless", label));
            assert!(
                matches!(action, Some(Action::FilterClientType(clicked)) if clicked == filter),
                "{label}: {action:?}"
            );
            assert_eq!(screen.filter, filter);
        }
    }

    #[test]
    fn tab_in_a_detail_cycles_the_filter_and_closes_the_detail() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut screen = ClientsScreen::new();
        screen.detail_open = true;

        let action = screen.handle_key_input(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        assert!(matches!(
            action,
            Some(Action::FilterClientType(ClientTypeFilter::Wireless))
        ));
        assert!(!screen.detail_open);
    }

    #[test]
    fn a_filter_click_closes_an_open_detail() {
        let mut screen = ClientsScreen::new();
        let rows = render(&screen);
        screen.detail_open = true;

        let action = screen.handle_mouse_input(click_label(&rows, "Wireless", "Wired"));

        assert!(matches!(
            action,
            Some(Action::FilterClientType(ClientTypeFilter::Wired))
        ));
        assert!(!screen.detail_open);
    }
}
