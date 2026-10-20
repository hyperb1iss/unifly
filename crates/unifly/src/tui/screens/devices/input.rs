use super::DevicesScreen;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};

use crate::tui::action::{Action, DeviceDetailTab};
use crate::tui::widgets::click::{self, Click};

impl DevicesScreen {
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
            KeyCode::Enter => self.open_selected_detail().map(Action::OpenDeviceDetail),
            KeyCode::Char('R') => self.current_action_device_id().map(Action::RequestRestart),
            KeyCode::Char('L') => self.current_action_device_id().map(Action::RequestLocate),
            KeyCode::Char('U') => self.current_action_device_id().map(Action::RequestUpgrade),
            _ => None,
        }
    }

    pub(super) fn apply_action(&mut self, action: &Action) {
        match action {
            Action::DevicesUpdated(devices) => {
                self.apply_devices_update(devices);
            }
            Action::ClientsUpdated(clients) => {
                self.clients = std::sync::Arc::clone(clients);
            }
            Action::CloseDetail => {
                self.close_detail();
            }
            Action::DeviceDetailTab(tab) => {
                self.detail_tab = *tab;
            }
            Action::SearchInput(query) => {
                self.set_search_query(query);
            }
            Action::CloseSearch => {
                self.clear_search();
            }
            _ => {}
        }
    }

    fn handle_detail_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc => {
                self.close_detail();
                Some(Action::CloseDetail)
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.detail_tab = previous_detail_tab(self.detail_tab);
                None
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.detail_tab = next_detail_tab(self.detail_tab);
                None
            }
            KeyCode::Char('R') => self.current_action_device_id().map(Action::RequestRestart),
            KeyCode::Char('L') => self.current_action_device_id().map(Action::RequestLocate),
            KeyCode::Char('U') => self.current_action_device_id().map(Action::RequestUpgrade),
            _ => None,
        }
    }

    /// Mouse and touch input: tabs and rows by click, the list by wheel.
    pub(super) fn handle_mouse_input(&mut self, mouse: MouseEvent) -> Option<Action> {
        if let Some(delta) = click::wheel_delta(mouse) {
            return self.scroll(delta);
        }
        let target = self.hits.borrow().clicked(mouse)?;
        match target {
            Click::Tab(index) => self.click_detail_tab(index),
            Click::Row(index) => self.click_row(index),
            _ => None,
        }
    }

    /// A detail tab switches to it, like `h`/`l`.
    fn click_detail_tab(&mut self, index: usize) -> Option<Action> {
        let tab = *DeviceDetailTab::ALL.get(index)?;
        if tab == self.detail_tab {
            return None;
        }
        self.detail_tab = tab;
        Some(Action::DeviceDetailTab(tab))
    }

    /// A row selects its device, and a second tap opens it, like `Enter`.
    /// With a detail open, the detail follows the tapped device.
    fn click_row(&mut self, index: usize) -> Option<Action> {
        if self.detail_open {
            self.select(index);
            return self.retarget_detail();
        }
        if self.selected_row_index(&self.filtered_devices()) == Some(index) {
            return self.open_selected_detail().map(Action::OpenDeviceDetail);
        }
        self.select(index);
        None
    }

    /// The wheel moves the selection, and an open detail follows it.
    fn scroll(&mut self, delta: isize) -> Option<Action> {
        self.move_selection(delta);
        if self.detail_open {
            self.retarget_detail()
        } else {
            None
        }
    }

    /// Point the open detail at the selected device, on the same tab, so
    /// stepping from switch to switch stays on their ports.
    fn retarget_detail(&mut self) -> Option<Action> {
        let tab = self.detail_tab;
        let opened = self.open_selected_detail();
        self.detail_tab = tab;
        opened.map(Action::OpenDeviceDetail)
    }
}

fn previous_detail_tab(tab: DeviceDetailTab) -> DeviceDetailTab {
    match tab {
        DeviceDetailTab::Overview => DeviceDetailTab::Ports,
        DeviceDetailTab::Performance => DeviceDetailTab::Overview,
        DeviceDetailTab::Radios => DeviceDetailTab::Performance,
        DeviceDetailTab::Clients => DeviceDetailTab::Radios,
        DeviceDetailTab::Ports => DeviceDetailTab::Clients,
    }
}

fn next_detail_tab(tab: DeviceDetailTab) -> DeviceDetailTab {
    match tab {
        DeviceDetailTab::Overview => DeviceDetailTab::Performance,
        DeviceDetailTab::Performance => DeviceDetailTab::Radios,
        DeviceDetailTab::Radios => DeviceDetailTab::Clients,
        DeviceDetailTab::Clients => DeviceDetailTab::Ports,
        DeviceDetailTab::Ports => DeviceDetailTab::Overview,
    }
}
