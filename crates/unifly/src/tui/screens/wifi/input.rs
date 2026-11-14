use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};

use super::keys::next_sort_field;
use super::{OverviewPanel, WifiScreen};
use crate::tui::action::{Action, Notification, WifiSubTab};
use crate::tui::widgets::click::{self, Click};

impl WifiScreen {
    pub(super) fn handle_key_input(&mut self, key: KeyEvent) -> Option<Action> {
        // A prompt takes every key while it is open, so a key that gets
        // here means none is. One that was cancelled leaves nothing
        // behind, least of all a typed passphrase.
        self.ssid_prompt = None;
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
            KeyCode::Char('l') | KeyCode::Right => Some(self.switch_sub_tab(self.sub_tab.next())),
            KeyCode::Char('h') | KeyCode::Left => Some(self.switch_sub_tab(self.sub_tab.prev())),
            KeyCode::Enter => self.handle_enter(),
            KeyCode::Esc => self.handle_escape(),
            _ => match self.sub_tab {
                WifiSubTab::Overview => self.handle_overview_key(key),
                WifiSubTab::Clients => self.handle_clients_key(key),
                WifiSubTab::Ssids => self.handle_ssid_key(key),
                WifiSubTab::Neighbors => self.handle_neighbors_key(key),
                WifiSubTab::Roaming => match key.code {
                    KeyCode::Char('r') => self.refresh_roam_history(),
                    _ => None,
                },
            },
        }
    }

    fn handle_overview_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            // The sort field changes in the action handler, after it has
            // captured which rows are selected under the current order.
            KeyCode::Char('s') => Some(Action::WifiSortColumn(next_sort_field(
                self.sub_tab,
                self.ap_sort,
            ))),
            KeyCode::Char('c') => Some(Action::WifiToggleChannelMap),
            KeyCode::Char('[') if self.channel_map_open() => {
                Some(Action::WifiBandSelect(self.selected_band.prev()))
            }
            KeyCode::Char(']') if self.channel_map_open() => {
                Some(Action::WifiBandSelect(self.selected_band.next()))
            }
            KeyCode::Char(' ') => {
                let selected = self.selected_ap()?.id.clone();
                let focused = self.focused_ap.as_ref().map(|focused| &focused.id);
                Some(Action::WifiFocusAp(
                    (focused != Some(&selected)).then_some(selected),
                ))
            }
            KeyCode::Char('o') => self
                .selected_ap()
                .map(|ap| Action::ShowDevice(ap.id.clone())),
            KeyCode::Char('R') => self
                .selected_ap()
                .map(|ap| Action::RequestRestart(ap.id.clone())),
            KeyCode::Char('L') => self
                .selected_ap()
                .map(|ap| Action::RequestLocate(ap.id.clone())),
            KeyCode::Char('U') => self.selected_ap().map(|ap| {
                if ap.firmware_updatable {
                    Action::RequestUpgrade(ap.id.clone())
                } else {
                    Action::Notify(Notification::info(format!(
                        "{} has no firmware update waiting",
                        super::state::display_device_name(ap)
                    )))
                }
            }),
            _ => None,
        }
    }

    fn handle_clients_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Char('s') => Some(Action::WifiSortColumn(next_sort_field(
                self.sub_tab,
                self.client_sort,
            ))),
            KeyCode::Char('o') => self
                .selected_client()
                .map(|client| Action::ShowClient(client.id.clone())),
            KeyCode::Char('r') => {
                self.selected_client()?;
                Some(self.switch_sub_tab(WifiSubTab::Roaming))
            }
            KeyCode::Char('b') => self.selected_client().map(|client| {
                if client.blocked {
                    Action::RequestUnblockClient(client.id.clone())
                } else {
                    Action::RequestBlockClient(client.id.clone())
                }
            }),
            KeyCode::Char('x') => self
                .selected_client()
                .map(|client| Action::RequestKickClient(client.id.clone())),
            KeyCode::Char('F') => self
                .selected_client()
                .map(|client| Action::RequestForgetClient(client.id.clone())),
            _ => None,
        }
    }

    fn handle_neighbors_key(&mut self, key: KeyEvent) -> Option<Action> {
        if self.session_missing() {
            return None;
        }
        match key.code {
            KeyCode::Char('s') => Some(Action::WifiSortColumn(next_sort_field(
                self.sub_tab,
                self.neighbor_sort,
            ))),
            KeyCode::Char('f' | ']') => Some(Action::WifiBandSelect(self.selected_band.next())),
            KeyCode::Char('[') => Some(Action::WifiBandSelect(self.selected_band.prev())),
            _ => None,
        }
    }

    /// Mouse and touch input: sub-tabs, rows, chips, and hints by click,
    /// the list by wheel.
    pub(super) fn handle_mouse_input(&mut self, mouse: MouseEvent) -> Option<Action> {
        if let Some(delta) = click::wheel_delta(mouse) {
            self.move_selection(delta);
            return None;
        }
        let target = self.hits.borrow().clicked(mouse)?;
        match target {
            Click::Tab(index) => {
                let tab = *WifiSubTab::ALL.get(index)?;
                (tab != self.sub_tab).then(|| self.switch_sub_tab(tab))
            }
            Click::Row(index) => self.click_row(index),
            Click::Key(key) => Some(Action::PressKey(key)),
            Click::Action(action) => Some(action),
            Click::Absorb => None,
        }
    }

    /// Switch to `tab` now, and say so: the action is applied again when
    /// it comes round, which changes nothing.
    fn switch_sub_tab(&mut self, tab: WifiSubTab) -> Action {
        self.enter_sub_tab(tab);
        Action::WifiSubTab(tab)
    }

    /// A row selects it, and a second tap acts like `Enter`.
    fn click_row(&mut self, index: usize) -> Option<Action> {
        if self.active_table_state().selected() == Some(index) {
            return self.handle_enter();
        }
        self.select_row(index);
        None
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
        self.select_row(next);
    }

    fn select_last(&mut self) {
        let len = self.active_len();
        if len > 0 {
            self.select_row(len - 1);
        }
    }

    fn handle_enter(&mut self) -> Option<Action> {
        match self.sub_tab {
            WifiSubTab::Overview => {
                self.selected_ap()?;
                self.overview_panel = if self.overview_panel == OverviewPanel::Detail {
                    OverviewPanel::None
                } else {
                    OverviewPanel::Detail
                };
                Some(Action::Invalidate)
            }
            WifiSubTab::Clients => {
                if self.client_detail_open {
                    self.client_detail_open = false;
                    return Some(Action::Invalidate);
                }
                let ip = self.selected_client().map(|client| client.ip)?;
                self.client_detail_open = true;
                // Opening the panel asks at once; the settle delay is for
                // scrolling with it open.
                self.client_cursor_moved_at = None;
                ip.filter(|_| !self.session_missing())
                    .and_then(|ip| self.request_client_detail(ip.to_string(), Instant::now()))
                    .or(Some(Action::Invalidate))
            }
            WifiSubTab::Ssids => {
                if !self.ssid_detail_open {
                    self.selected_ssid()?;
                }
                self.ssid_detail_open = !self.ssid_detail_open;
                Some(Action::Invalidate)
            }
            WifiSubTab::Neighbors => None,
            WifiSubTab::Roaming => self.refresh_roam_history(),
        }
    }

    /// Fetch the Roaming tab's history again, unless a fetch is running.
    fn refresh_roam_history(&mut self) -> Option<Action> {
        if self.session_missing() {
            return None;
        }
        self.roam_target.as_ref()?;
        Some(self.request_roam_history().unwrap_or(Action::Invalidate))
    }

    /// Close the innermost thing this sub-tab is showing: its panel, then
    /// the AP focus where the focus narrows the list. With nothing to
    /// close the key goes back to the app.
    fn handle_escape(&mut self) -> Option<Action> {
        // A consumed Esc returns an action, or the App would go on to its
        // own Esc and leave the screen.
        match self.sub_tab {
            WifiSubTab::Overview if self.overview_panel != OverviewPanel::None => {
                self.overview_panel = OverviewPanel::None;
                Some(Action::Invalidate)
            }
            WifiSubTab::Clients if self.client_detail_open => {
                self.client_detail_open = false;
                Some(Action::Invalidate)
            }
            WifiSubTab::Ssids if self.ssid_detail_open => {
                self.ssid_detail_open = false;
                Some(Action::Invalidate)
            }
            WifiSubTab::Overview | WifiSubTab::Clients | WifiSubTab::Neighbors
                if self.focused_ap.is_some() =>
            {
                Some(Action::WifiFocusAp(None))
            }
            _ => None,
        }
    }
}
