use super::DevicesScreen;
use super::detail::tabs_for;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};

use crate::tui::action::Action;
use crate::tui::widgets::click::{self, Click};

/// Rows a page key moves.
const PAGE: isize = 10;

impl DevicesScreen {
    pub(super) fn handle_key_input(&mut self, key: KeyEvent) -> Option<Action> {
        if let Some(action) = self.handle_command_key(key) {
            return Some(action);
        }
        if self.detail.is_some() {
            return self.handle_detail_key(key);
        }

        if let Some(delta) = movement(key) {
            self.move_list(delta);
            return None;
        }
        match key.code {
            KeyCode::Char('l') | KeyCode::Right => self.set_filter(self.filter.stepped(1)),
            KeyCode::Char('h') | KeyCode::Left => self.set_filter(self.filter.stepped(-1)),
            KeyCode::Char('s') => self.set_sort(self.sort.next()),
            KeyCode::Char('r') => self.set_sort(self.sort.reversed()),
            KeyCode::Enter => {
                self.open_selected_detail();
            }
            _ => {}
        }
        None
    }

    /// Commands that act on a device, in the table and in the detail.
    fn handle_command_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Char('R') => self.request_restart(),
            KeyCode::Char('L') => self.request_locate(),
            KeyCode::Char('U') => self.request_upgrade(),
            KeyCode::Char('A') => self.request_adopt(),
            KeyCode::Char('P') => self.request_provision(),
            KeyCode::Char('T') => self.request_speedtest(),
            _ => None,
        }
    }

    /// Keys with a detail open. On the Ports and Clients tabs movement
    /// drives the tab's own list; on the other tabs it steps to the next
    /// device, keeping the tab.
    fn handle_detail_key(&mut self, key: KeyEvent) -> Option<Action> {
        if let Some(delta) = movement(key) {
            if self.detail_has_list() {
                self.move_detail_list(delta);
                return None;
            }
            self.move_list(delta);
            return self.retarget_detail();
        }
        match key.code {
            KeyCode::Esc => {
                self.close_detail();
                Some(Action::CloseDetail)
            }
            KeyCode::Char('h') | KeyCode::Left => self.step_detail_tab(-1),
            KeyCode::Char('l') | KeyCode::Right => self.step_detail_tab(1),
            KeyCode::Enter => self
                .cursor_client()
                .map(|client| Action::ShowClient(client.id.clone())),
            KeyCode::Char('F') => self.request_forget(),
            KeyCode::Char('C') => self.request_power_cycle(),
            KeyCode::Char('O') => self.request_poe_toggle(),
            KeyCode::Char('N') => self.request_port_rename(),
            _ => None,
        }
    }

    fn move_list(&mut self, movement: Movement) {
        match movement {
            Movement::By(delta) => self.move_selection(delta),
            Movement::First => self.select(0),
            Movement::Last => self.select_last(),
        }
    }

    fn move_detail_list(&mut self, movement: Movement) {
        match movement {
            Movement::By(delta) => self.move_detail_cursor(delta),
            Movement::First => self.set_detail_cursor(0),
            Movement::Last => self.set_detail_cursor(usize::MAX),
        }
    }

    /// Mouse and touch input: tabs, filters, and rows by click, and the
    /// list under the pointer by wheel.
    pub(super) fn handle_mouse_input(&mut self, mouse: MouseEvent) -> Option<Action> {
        if let Some(delta) = click::wheel_delta(mouse) {
            return self.scroll(mouse, delta);
        }
        let detail_row = self.detail_hits.borrow().rows.clicked(mouse);
        if let Some(index) = detail_row {
            return self.click_detail_row(index);
        }
        let target = self.hits.borrow().clicked(mouse)?;
        match target {
            Click::Tab(index) if self.in_detail(mouse) => self.click_detail_tab(index),
            Click::Tab(index) => self.click_filter(index),
            Click::Row(index) => self.click_row(index),
            Click::Key(key) => Some(Action::PressKey(key)),
            _ => None,
        }
    }

    fn in_detail(&self, mouse: MouseEvent) -> bool {
        self.detail_hits.borrow().contains(mouse.column, mouse.row)
    }

    /// A detail tab switches to it, like `h`/`l`.
    fn click_detail_tab(&mut self, index: usize) -> Option<Action> {
        let tab = *tabs_for(self.detail_device()?).get(index)?;
        self.set_detail_tab(tab)
    }

    /// A filter selects it, like `h`/`l`.
    fn click_filter(&mut self, index: usize) -> Option<Action> {
        let filter = *super::view::DeviceFilter::ALL.get(index)?;
        self.set_filter(filter);
        None
    }

    /// A row in the detail's list moves the cursor to it, and a second tap
    /// on a client opens it, like `Enter`.
    fn click_detail_row(&mut self, index: usize) -> Option<Action> {
        let again = self
            .detail
            .as_ref()
            .is_some_and(|detail| detail.cursor_index() == index);
        self.set_detail_cursor(index);
        again
            .then(|| self.cursor_client())
            .flatten()
            .map(|client| Action::ShowClient(client.id.clone()))
    }

    /// A row selects its device, and a second tap opens it, like `Enter`.
    /// With a detail open, the detail follows the tapped device.
    fn click_row(&mut self, index: usize) -> Option<Action> {
        if self.detail.is_some() {
            self.select(index);
            return self.retarget_detail();
        }
        if self.selected_row_index(&self.filtered_devices()) == Some(index) {
            self.open_selected_detail();
            return None;
        }
        self.select(index);
        None
    }

    /// The wheel moves the list under the pointer: the detail's own list
    /// over the pane, the device table elsewhere, with an open detail
    /// following the selection.
    fn scroll(&mut self, mouse: MouseEvent, delta: isize) -> Option<Action> {
        if self.detail_has_list() && self.in_detail(mouse) {
            self.move_detail_cursor(delta);
            return None;
        }
        self.move_selection(delta);
        self.retarget_detail()
    }
}

/// A list movement a key asks for.
#[derive(Debug, Clone, Copy)]
enum Movement {
    By(isize),
    First,
    Last,
}

/// The movement `key` means in any list: `j`/`k` and the arrows, `g`/`G`,
/// and the page keys, which arrive as `Ctrl+d`/`Ctrl+u`.
fn movement(key: KeyEvent) -> Option<Movement> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('d') if ctrl => Some(Movement::By(PAGE)),
        KeyCode::Char('u') if ctrl => Some(Movement::By(-PAGE)),
        KeyCode::Char('j') | KeyCode::Down => Some(Movement::By(1)),
        KeyCode::Char('k') | KeyCode::Up => Some(Movement::By(-1)),
        KeyCode::Char('g') => Some(Movement::First),
        KeyCode::Char('G') => Some(Movement::Last),
        _ => None,
    }
}
