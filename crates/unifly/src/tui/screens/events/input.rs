use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};

use unifly_api::EntityId;

use super::{CATEGORIES, EventsScreen, View};
use crate::tui::action::{Action, Notification};
use crate::tui::component::LIST_HELP;
use crate::tui::widgets::click::{self, Click};

/// Why a history load cannot run without session access.
const NEEDS_SESSION: &str = "Older events need a direct controller session";

impl EventsScreen {
    pub(super) fn handle_key_input(&mut self, key: KeyEvent) -> Option<Action> {
        if self.view == View::Alarms {
            return self.handle_alarm_key(key);
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let page = isize::try_from(self.page.get().max(1)).unwrap_or(10);
        match key.code {
            KeyCode::Esc if self.detail_open => self.detail_open = false,
            KeyCode::Esc if self.history.loading.is_some() => return Some(self.cancel_history()),
            KeyCode::Char(' ') => return Some(Action::ToggleEventPause),
            KeyCode::Char('j') | KeyCode::Down => self.move_cursor(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_cursor(-1),
            KeyCode::Char('d') if ctrl => self.move_cursor(page),
            KeyCode::Char('u') if ctrl => self.move_cursor(-page),
            KeyCode::Char('g') => self.jump(false),
            KeyCode::Char('G') => self.jump(true),
            KeyCode::Enter => self.open_detail(),
            KeyCode::Char('l') | KeyCode::Right => self.step_category(1),
            KeyCode::Char('h') | KeyCode::Left => self.step_category(-1),
            KeyCode::Char('v') => return Some(self.toggle_alarms()),
            KeyCode::Char('o') => return Some(self.load_older()),
            KeyCode::Char('f') => return Some(self.toggle_firewall()),
            KeyCode::Char('p') => return Some(self.toggle_heatmap()),
            KeyCode::Char('d') if self.detail_open => return Some(self.jump_to_device()),
            KeyCode::Char('c') if self.detail_open => return Some(self.jump_to_client()),
            _ => return None,
        }
        Some(Action::Invalidate)
    }

    /// Open the detail pane on the event under the cursor. On a live feed
    /// that is the newest event, and the feed pauses so it stays put.
    fn open_detail(&mut self) {
        if !self.paused {
            self.jump(true);
        }
        self.detail_open = self.selected().is_some();
    }

    fn has_session(&self) -> bool {
        self.access.is_some_and(|access| access.session)
    }

    /// Show or hide the hourly heatmap, or say why it cannot show.
    fn toggle_heatmap(&mut self) -> Action {
        let shown = self.heatmap_shown.get();
        if !shown && !self.heatmap_fits.get() {
            return Action::Notify(Notification::info("The heatmap needs a larger terminal"));
        }
        self.heatmap = Some(!shown);
        Action::Invalidate
    }

    /// Ask for events older than the oldest one held.
    fn load_older(&mut self) -> Action {
        if !self.has_session() {
            return Action::Notify(Notification::info(NEEDS_SESSION));
        }
        if self.history.loading.is_some() {
            return Action::Invalidate;
        }
        if self.feed.is_full() {
            self.history.note = Some(self.cap_note());
            return Action::Invalidate;
        }
        self.request_older()
    }

    /// Turn firewall blocks on (loading them for the span on screen) or
    /// off (taking them back out). Neither happens while a load runs: its
    /// result would arrive under a status that no longer matches it.
    fn toggle_firewall(&mut self) -> Action {
        if self.history.loading.is_some() {
            return Action::Invalidate;
        }
        if self.history.include_firewall {
            self.history.include_firewall = false;
            let dropped = self.drop_firewall_blocks();
            self.history.note = Some(match dropped {
                1 => "1 firewall block hidden".into(),
                dropped => format!("{dropped} firewall blocks hidden"),
            });
            return Action::Invalidate;
        }
        if !self.has_session() {
            return Action::Notify(Notification::info(NEEDS_SESSION));
        }
        self.request_firewall_blocks()
    }

    /// The device the selected event names, when it is still on the
    /// controller.
    pub(super) fn selected_device_id(&self) -> Option<EntityId> {
        let mac = self.selected()?.device_mac.as_ref()?;
        self.devices
            .iter()
            .find(|device| &device.mac == mac)
            .map(|device| device.id.clone())
    }

    /// The client the selected event names, when it is connected.
    pub(super) fn selected_client_id(&self) -> Option<EntityId> {
        let mac = self.selected()?.client_mac.as_ref()?;
        self.clients
            .iter()
            .find(|client| &client.mac == mac)
            .map(|client| client.id.clone())
    }

    fn jump_to_device(&self) -> Action {
        match self.selected_device_id() {
            Some(id) => Action::ShowDevice(id),
            None => Action::Notify(Notification::info("This event names no known device")),
        }
    }

    fn jump_to_client(&self) -> Action {
        match self.selected_client_id() {
            Some(id) => Action::ShowClient(id),
            None => Action::Notify(Notification::info("This event names no connected client")),
        }
    }

    /// Mouse and touch input: a toolbar chip presses its key, a category
    /// picks itself, a row takes the cursor (and a second tap opens it),
    /// and the wheel moves through the feed.
    pub(super) fn handle_mouse_input(&mut self, mouse: MouseEvent) -> Option<Action> {
        if let Some(delta) = click::wheel_delta(mouse) {
            let key = if delta < 0 {
                KeyCode::Up
            } else {
                KeyCode::Down
            };
            return self.handle_key_input(key.into());
        }
        let target = self.hits.borrow().clicked(mouse)?;
        match target {
            Click::Row(row) if self.view == View::Alarms => self.click_alarm_row(row),
            Click::Key(key) => Some(Action::PressKey(key)),
            Click::Action(action) => Some(action),
            Click::Tab(index) => {
                self.set_category(
                    index
                        .checked_sub(1)
                        .and_then(|at| CATEGORIES.get(at).copied()),
                );
                Some(Action::Invalidate)
            }
            Click::Row(row) => {
                let view = self.view();
                let again = self.paused && self.cursor_row(&view) == row;
                self.select_row(row);
                if again {
                    self.detail_open = true;
                }
                Some(Action::Invalidate)
            }
            Click::Absorb => None,
        }
    }

    pub(super) fn help_keys(&self) -> Vec<(&'static str, &'static str)> {
        if self.view == View::Alarms {
            return self.alarm_help();
        }
        let mut keys = Vec::new();
        if self.history.loading.is_some() && !self.detail_open {
            keys.push(("Esc", "Cancel the history load"));
        }
        if self.detail_open {
            keys.push(("Esc", "Close the event detail"));
            keys.push(("d", "Open the event's device"));
            keys.push(("c", "Open the event's client"));
        } else {
            keys.push(("Enter", "Open the selected event"));
        }
        keys.push((
            "Space",
            if self.paused {
                "Resume the live feed"
            } else {
                "Pause the feed"
            },
        ));
        keys.extend_from_slice(LIST_HELP);
        keys.extend_from_slice(&[
            ("h/l \u{2190}/\u{2192}", "Filter by category"),
            ("/", "Filter by message text"),
            ("v", "Show the alarms"),
            ("o", "Load older events"),
            ("f", "Firewall blocks on / off"),
            ("p", "Hourly heatmap on / off"),
        ]);
        keys
    }
}
