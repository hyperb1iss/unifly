//! Events screen: the controller's event feed, live or scrolled back, with
//! category and text filters, a detail pane, and older history on demand.

mod detail;
pub(in crate::tui::screens) mod feed;
mod input;
mod punchcard;
pub(in crate::tui::screens) mod render;
mod state;
#[cfg(test)]
mod tests;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::Frame;
use ratatui::layout::Rect;

use unifly_api::model::EventCategory;
use unifly_api::{Client, Device};

use crate::tui::action::{Access, Action};
use crate::tui::component::Component;
use crate::tui::screens::link::Link;
use crate::tui::widgets::click::{Click, HitMap};

use feed::{EventKey, Feed};

/// Events kept in memory.
const CAPACITY: usize = 10_000;

/// The categories the filter steps through, after "All".
const CATEGORIES: [EventCategory; 8] = [
    EventCategory::Device,
    EventCategory::Client,
    EventCategory::Network,
    EventCategory::System,
    EventCategory::Admin,
    EventCategory::Firewall,
    EventCategory::Vpn,
    EventCategory::Unknown,
];

/// How far back each press of `o` reaches, in hours, and how many events
/// it asks for.
const HISTORY_STEPS: [(u32, u32); 4] = [(24, 500), (72, 1_000), (168, 2_500), (720, 5_000)];

/// The history window the feed has been filled from.
#[derive(Debug, Default)]
struct History {
    /// Index into [`HISTORY_STEPS`] of the widest window loaded so far.
    loaded: Option<usize>,
    include_firewall: bool,
    /// Hours the load in flight reaches back.
    loading: Option<u32>,
    /// The outcome of the last load, shown until the next one.
    note: Option<String>,
}

pub struct EventsScreen {
    focused: bool,
    feed: Feed,
    paused: bool,
    /// Index into the feed of the event under the cursor, while paused.
    cursor: usize,
    /// The first filtered row drawn while paused, kept so the list holds
    /// still as the cursor moves inside it.
    top: Cell<usize>,
    /// Rows the list drew last frame, for paging.
    page: Cell<usize>,
    /// Events that arrived since the feed was paused.
    unseen: usize,
    category: Option<EventCategory>,
    search: String,
    detail_open: bool,
    /// Whether the hourly heatmap is forced on or off; unset, it shows
    /// when the list keeps enough rows beside it.
    heatmap: Option<bool>,
    /// Whether the last frame drew the heatmap.
    heatmap_shown: Cell<bool>,
    access: Option<Access>,
    link: Link,
    history: History,
    /// Firewall blocks a history load brought in, so turning them off can
    /// take them back out.
    firewall_extra: HashSet<EventKey>,
    devices: Arc<Vec<Arc<Device>>>,
    clients: Arc<Vec<Arc<Client>>>,
    /// What the last frame drew where, for clicks.
    hits: RefCell<HitMap<Click>>,
}

impl Default for EventsScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for EventsScreen {
    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        Ok(self.handle_key_input(key))
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        Ok(self.handle_mouse_input(mouse))
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        Ok(self.apply_action(action))
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        self.render_screen(frame, area);
    }

    fn search_query(&self) -> Option<&str> {
        Some(&self.search)
    }

    fn help(&self) -> Vec<(&'static str, &'static str)> {
        self.help_keys()
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn id(&self) -> &'static str {
        "Events"
    }
}
