use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::WifiScreen;
use crate::tui::action::{Action, WifiSortField, WifiSubTab};

impl WifiScreen {
    pub(super) fn handle_key_input(&mut self, key: KeyEvent) -> Option<Action> {
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
                self.select_first();
                Some(Action::ScrollToTop)
            }
            KeyCode::Char('G') => {
                self.select_last();
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
                self.sub_tab = self.sub_tab.next();
                Some(Action::WifiSubTab(self.sub_tab))
            }
            KeyCode::BackTab => {
                self.sub_tab = self.sub_tab.prev();
                Some(Action::WifiSubTab(self.sub_tab))
            }
            KeyCode::Enter => self.handle_enter(),
            KeyCode::Esc => self.handle_escape(),
            // The sort field changes in the action handler, after it has
            // captured which rows are selected under the current order.
            KeyCode::Char('s') => Some(Action::WifiSortColumn(next_sort_field(
                self.sub_tab,
                self.sort_field,
            ))),
            KeyCode::Char('c') if self.sub_tab == crate::tui::action::WifiSubTab::Overview => {
                Some(Action::WifiToggleChannelMap)
            }
            KeyCode::Char('f') if self.sub_tab == WifiSubTab::Neighbors => {
                self.selected_band = self.selected_band.next();
                Some(Action::WifiBandSelect(self.selected_band))
            }
            KeyCode::Char('[') => {
                self.selected_band = self.selected_band.prev();
                Some(Action::WifiBandSelect(self.selected_band))
            }
            KeyCode::Char(']') => {
                self.selected_band = self.selected_band.next();
                Some(Action::WifiBandSelect(self.selected_band))
            }
            KeyCode::Char('R') if self.sub_tab == crate::tui::action::WifiSubTab::Overview => self
                .selected_ap()
                .map(|ap| Action::RequestRestart(ap.id.clone())),
            KeyCode::Char('L') if self.sub_tab == crate::tui::action::WifiSubTab::Overview => self
                .selected_ap()
                .map(|ap| Action::RequestLocate(ap.id.clone())),
            KeyCode::Char('b') if self.sub_tab == crate::tui::action::WifiSubTab::Clients => self
                .selected_client()
                .map(|client| Action::RequestBlockClient(client.id.clone())),
            KeyCode::Char('B' | 'u') if self.sub_tab == crate::tui::action::WifiSubTab::Clients => {
                self.selected_client()
                    .map(|client| Action::RequestUnblockClient(client.id.clone()))
            }
            KeyCode::Char('x') if self.sub_tab == crate::tui::action::WifiSubTab::Clients => self
                .selected_client()
                .map(|client| Action::RequestKickClient(client.id.clone())),
            KeyCode::Char('r') if self.sub_tab == crate::tui::action::WifiSubTab::Roaming => self
                .focused_client_mac()
                .map(|mac| Action::RequestWifiRoamHistory {
                    mac,
                    limit: Some(100),
                }),
            _ => None,
        }
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        let len = self.active_len();
        if len == 0 {
            return;
        }

        let current = self.active_table_state().selected().unwrap_or(0);
        let next = if delta.is_negative() {
            current.saturating_sub(delta.unsigned_abs())
        } else {
            current.saturating_add(delta.unsigned_abs()).min(len - 1)
        };
        self.active_table_state().select(Some(next));
        self.update_focus_from_selection();
    }

    fn select_first(&mut self) {
        if self.active_len() > 0 {
            self.active_table_state().select(Some(0));
            self.update_focus_from_selection();
        }
    }

    fn select_last(&mut self) {
        let len = self.active_len();
        if len > 0 {
            self.active_table_state().select(Some(len - 1));
            self.update_focus_from_selection();
        }
    }

    fn handle_enter(&mut self) -> Option<Action> {
        match self.sub_tab {
            crate::tui::action::WifiSubTab::Overview => {
                self.detail_open = !self.detail_open;
                None
            }
            crate::tui::action::WifiSubTab::Clients => {
                self.detail_open = !self.detail_open;
                if self.detail_open {
                    self.focus_client_from_selection();
                    self.selected_client_ip()
                        .map(Action::RequestWifiClientDetail)
                } else {
                    Some(Action::CloseDetail)
                }
            }
            crate::tui::action::WifiSubTab::Roaming => {
                self.focused_client_mac()
                    .map(|mac| Action::RequestWifiRoamHistory {
                        mac,
                        limit: Some(100),
                    })
            }
            crate::tui::action::WifiSubTab::Neighbors => None,
        }
    }

    fn handle_escape(&mut self) -> Option<Action> {
        if self.detail_open {
            self.detail_open = false;
            return Some(Action::CloseDetail);
        }

        if self.channel_map_open {
            self.channel_map_open = false;
            return None;
        }

        if self.focused_client_id.is_some() {
            self.focused_client_id = None;
            self.roam_history = std::sync::Arc::new(Vec::new());
            self.roam_history_mac = None;
            self.roam_history_pending_mac = None;
            return None;
        }

        if self.focused_ap_id.is_some() {
            self.focused_ap_id = None;
            self.sync_client_selection();
            self.sync_neighbor_selection();
            return Some(Action::WifiFocusAp(None));
        }

        None
    }

    fn active_len(&self) -> usize {
        match self.sub_tab {
            crate::tui::action::WifiSubTab::Overview => self.ap_devices().len(),
            crate::tui::action::WifiSubTab::Clients => self.wireless_clients().len(),
            crate::tui::action::WifiSubTab::Neighbors => self.visible_neighbors().len(),
            crate::tui::action::WifiSubTab::Roaming => self.parsed_roam_rows().len(),
        }
    }

    fn active_table_state(&mut self) -> &mut ratatui::widgets::TableState {
        match self.sub_tab {
            crate::tui::action::WifiSubTab::Overview => &mut self.ap_table_state,
            crate::tui::action::WifiSubTab::Clients => &mut self.client_table_state,
            crate::tui::action::WifiSubTab::Neighbors => &mut self.neighbor_table_state,
            crate::tui::action::WifiSubTab::Roaming => &mut self.roam_table_state,
        }
    }

    fn update_focus_from_selection(&mut self) {
        match self.sub_tab {
            crate::tui::action::WifiSubTab::Overview => {
                self.focused_ap_id = self.selected_ap().map(|device| device.id.clone());
                self.sync_client_selection();
                self.sync_neighbor_selection();
            }
            crate::tui::action::WifiSubTab::Clients => {
                self.focus_client_from_selection();
            }
            crate::tui::action::WifiSubTab::Neighbors | crate::tui::action::WifiSubTab::Roaming => {
            }
        }
    }
}

fn next_sort_field(
    sub_tab: crate::tui::action::WifiSubTab,
    current: WifiSortField,
) -> WifiSortField {
    match sub_tab {
        crate::tui::action::WifiSubTab::Overview => match current {
            WifiSortField::Health => WifiSortField::Clients,
            WifiSortField::Clients => WifiSortField::Channel,
            WifiSortField::Channel => WifiSortField::Name,
            _ => WifiSortField::Health,
        },
        crate::tui::action::WifiSubTab::Clients => match current {
            WifiSortField::Health => WifiSortField::Signal,
            WifiSortField::Signal => WifiSortField::Name,
            _ => WifiSortField::Health,
        },
        crate::tui::action::WifiSubTab::Neighbors => match current {
            WifiSortField::Signal => WifiSortField::Channel,
            WifiSortField::Channel => WifiSortField::Security,
            WifiSortField::Security => WifiSortField::Name,
            _ => WifiSortField::Signal,
        },
        crate::tui::action::WifiSubTab::Roaming => WifiSortField::Time,
    }
}
