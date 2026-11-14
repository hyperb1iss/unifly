//! Networks screen: the network table with a detail panel, and a form for
//! editing a network or creating one.

mod edit;
mod input;
mod render;
mod state;

use std::cell::RefCell;
use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::TableState;
use tokio::sync::mpsc::UnboundedSender;

use unifly_api::Network;
use unifly_api::model::FirewallZone;

use crate::tui::action::{Access, Action};
use crate::tui::component::{Component, LIST_HELP};
use crate::tui::table::TableScroll;
use crate::tui::widgets::click::{Click, HitMap};

use self::edit::NetworkForm;

pub struct NetworksScreen {
    focused: bool,
    networks: Arc<Vec<Arc<Network>>>,
    /// Firewall zones, for the detail's zone name.
    zones: Arc<Vec<Arc<FirewallZone>>>,
    /// What the controller can reach, once it has connected.
    access: Option<Access>,
    table_state: TableState,
    table_scroll: TableScroll,
    detail_open: bool,
    form: Option<NetworkForm>,
    /// What the last frame drew where, for clicks.
    hits: RefCell<HitMap<Click>>,
    action_tx: Option<UnboundedSender<Action>>,
}

impl Default for NetworksScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for NetworksScreen {
    fn init(&mut self, action_tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(action_tx);
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        Ok(self.handle_key_input(key))
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        Ok(self.handle_mouse_input(mouse))
    }

    fn handle_paste(&mut self, text: &str) -> Option<Action> {
        let form = self.form.as_mut().filter(|form| !form.saving)?;
        form.paste(text);
        None
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        self.apply_action(action);
        Ok(None)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        self.render_screen(frame, area);
    }

    fn captures_input(&self) -> bool {
        self.form.is_some()
    }

    fn help(&self) -> Vec<(&'static str, &'static str)> {
        if self.form.is_some() {
            return vec![
                ("Tab \u{2193}", "Next field"),
                ("Shift+Tab \u{2191}", "Previous field"),
                ("Space", "Flip an on/off field"),
                ("\u{2190}/\u{2192}", "Move in the text"),
                ("Home/End", "Start / end of the text"),
                ("^w  ^u", "Delete word / to start"),
                ("Enter", "Save"),
                ("Esc", "Close without saving"),
            ];
        }
        let mut keys = LIST_HELP.to_vec();
        keys.extend([
            ("Enter", "Show / hide the detail"),
            ("Esc", "Hide the detail"),
            ("e", "Edit the network"),
            ("c", "Create a network"),
            ("D", "Delete the network"),
        ]);
        keys
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn id(&self) -> &'static str {
        "Networks"
    }
}
