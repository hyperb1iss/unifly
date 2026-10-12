use super::NetworksScreen;
use super::state::NetworkEditState;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::action::Action;

impl NetworksScreen {
    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    pub(super) fn handle_key_input(&mut self, key: KeyEvent) -> Option<Action> {
        if self.edit_state.is_some() {
            return self.handle_edit_key(key);
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
                if !self.networks.is_empty() {
                    self.select(self.networks.len() - 1);
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
            KeyCode::Enter => {
                self.detail_open = !self.detail_open;
                None
            }
            KeyCode::Esc if self.detail_open => {
                self.detail_open = false;
                None
            }
            KeyCode::Char('e') => {
                if let Some(network) = self.selected_network().cloned() {
                    self.edit_state = Some(NetworkEditState::from_network(&network));
                    self.detail_open = true;
                }
                None
            }
            _ => None,
        }
    }

    fn handle_edit_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc => {
                self.edit_state = None;
                return None;
            }
            KeyCode::Enter => {
                // Save to the network the form was opened on, never to
                // whatever row the selection sits on after a refresh.
                if let Some(edit) = self.edit_state.take()
                    && self
                        .networks
                        .iter()
                        .any(|network| network.id == edit.network_id)
                {
                    let request = edit.build_request();
                    return Some(Action::NetworkSave(edit.network_id, Box::new(request)));
                }
                return None;
            }
            _ => {}
        }

        if let Some(edit) = self.edit_state.as_mut() {
            match key.code {
                KeyCode::Tab | KeyCode::Down => {
                    edit.field_idx = (edit.field_idx + 1) % NetworkEditState::FIELD_COUNT;
                }
                KeyCode::BackTab | KeyCode::Up => {
                    edit.field_idx = if edit.field_idx == 0 {
                        NetworkEditState::FIELD_COUNT - 1
                    } else {
                        edit.field_idx - 1
                    };
                }
                KeyCode::Char(' ') if NetworkEditState::is_bool_field(edit.field_idx) => {
                    edit.toggle_bool();
                }
                KeyCode::Char(ch) if !NetworkEditState::is_bool_field(edit.field_idx) => {
                    edit.handle_text_input(ch);
                }
                KeyCode::Backspace if !NetworkEditState::is_bool_field(edit.field_idx) => {
                    edit.handle_backspace();
                }
                _ => {}
            }
        }

        None
    }
}
