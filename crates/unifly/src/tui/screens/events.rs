//! Events screen: the controller's event feed, live or scrolled back, with
//! category and text filters, a detail pane, and older history on demand.

mod alarms;
mod chrome;
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
use crate::tui::widgets::click::{Click, Hint, HitMap};

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

/// How far back of the oldest event held each load of older history
/// reaches, in hours, and how many events it asks for. A load that finds
/// nothing moves on to the next, wider step by itself.
const HISTORY_STEPS: [(u32, u32); 4] = [(24, 500), (72, 1_000), (168, 2_500), (720, 5_000)];

/// The screen's two lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum View {
    /// The event feed.
    #[default]
    Feed,
    /// The controller's unarchived alarms.
    Alarms,
}

/// What a history load is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryLoad {
    /// Events older than the oldest one held.
    Older,
    /// The firewall blocks of the span the feed already covers.
    Firewall,
}

/// A history load in flight.
#[derive(Debug, Clone, Copy)]
struct PendingHistory {
    /// Names the request, so a result that arrives after a cancel, or for
    /// an earlier request, is not taken for this one's.
    id: u64,
    load: HistoryLoad,
    /// Hours the load reaches back.
    hours: u32,
}

/// Older history loaded into the feed on request.
#[derive(Debug, Default)]
struct History {
    /// Index into [`HISTORY_STEPS`] of the window the next load asks for.
    step: usize,
    /// Whether a load has come back, so the title bar says how far the
    /// feed reaches.
    loaded: bool,
    /// Whether firewall blocks are loaded. Set by the load that brings
    /// them, never by asking.
    include_firewall: bool,
    loading: Option<PendingHistory>,
    /// The id the last request went out under.
    last_id: u64,
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
    /// Index into the feed of the event that row shows. It follows its
    /// event as others land before it, as the cursor does, so a history
    /// load or an eviction does not slide the rows.
    anchor: Cell<usize>,
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
    /// Whether the last frame had room for the heatmap.
    heatmap_fits: Cell<bool>,
    /// Whether the last frame had room for the feed and alarm chips on
    /// the toolbar. Without them the title says what they said and the
    /// status bar offers their keys.
    chips_shown: Cell<bool>,
    access: Option<Access>,
    link: Link,
    history: History,
    /// Firewall blocks a history load brought in, so turning them off can
    /// take them back out.
    firewall_extra: HashSet<EventKey>,
    devices: Arc<Vec<Arc<Device>>>,
    clients: Arc<Vec<Arc<Client>>>,
    /// Which of the screen's two lists is showing.
    view: View,
    alarms: alarms::AlarmList,
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
        // Search filters the feed; the Alarms view has nothing to filter.
        (self.view == View::Feed).then_some(self.search.as_str())
    }

    fn hints(&self) -> Vec<(u8, Hint)> {
        if self.view == View::Alarms {
            self.alarm_hints()
        } else {
            self.feed_hints()
        }
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
