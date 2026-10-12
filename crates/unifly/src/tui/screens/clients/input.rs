use super::ClientsScreen;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::action::Action;

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
            KeyCode::Enter => self.selected_client_id().map(|id| {
                self.detail_open = true;
                self.detail_client_id = Some(id.clone());
                Action::OpenClientDetail(id)
            }),
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

    fn handle_detail_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
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
