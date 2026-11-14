use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::sync::Arc;

use unifly_api::Event;
use unifly_api::model::EventCategory;

use super::feed::{EventKey, Feed};
use super::{CAPACITY, EventsScreen, HISTORY_STEPS, History};
use crate::tui::action::Action;
use crate::tui::screens::link::Link;

impl EventsScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            feed: Feed::new(CAPACITY),
            paused: false,
            cursor: 0,
            top: Cell::new(0),
            page: Cell::new(10),
            unseen: 0,
            category: None,
            search: String::new(),
            detail_open: false,
            heatmap: None,
            heatmap_shown: Cell::new(false),
            access: None,
            link: Link::default(),
            history: History::default(),
            firewall_extra: HashSet::new(),
            devices: Arc::default(),
            clients: Arc::default(),
            view: super::View::default(),
            alarms: super::alarms::AlarmList::default(),
            hits: RefCell::default(),
        }
    }

    /// Whether a filter is narrowing the list.
    pub(super) fn filtered(&self) -> bool {
        self.category.is_some() || !self.search.is_empty()
    }

    /// Feed indices of the events the filters let through, oldest first.
    pub(super) fn view(&self) -> Vec<usize> {
        let needle = self.search.to_lowercase();
        self.feed
            .events()
            .iter()
            .enumerate()
            .filter(|(_, event)| self.passes(event, &needle))
            .map(|(index, _)| index)
            .collect()
    }

    /// Whether `event` passes the category filter and holds `needle`, the
    /// search text in lowercase, in its message or type.
    fn passes(&self, event: &Event, needle: &str) -> bool {
        self.category
            .is_none_or(|category| event.category == category)
            && (needle.is_empty()
                || event.message.to_lowercase().contains(needle)
                || event.event_type.to_lowercase().contains(needle))
    }

    /// The row of `view` the cursor is on: its own event, or the nearest
    /// one after it when a filter hid that event.
    pub(super) fn cursor_row(&self, view: &[usize]) -> usize {
        view.partition_point(|index| *index < self.cursor)
            .min(view.len().saturating_sub(1))
    }

    /// The event under the cursor, when the feed is paused on one that the
    /// filters still show.
    pub(super) fn selected(&self) -> Option<&Arc<Event>> {
        if !self.paused {
            return None;
        }
        let view = self.view();
        let index = *view.get(self.cursor_row(&view))?;
        self.feed.get(index)
    }

    /// Stop following the feed, with the cursor on the newest event.
    fn pause(&mut self, view: &[usize]) {
        self.paused = true;
        self.unseen = 0;
        self.cursor = view.last().copied().unwrap_or(0);
    }

    /// Follow the feed again.
    fn resume(&mut self) {
        self.paused = false;
        self.unseen = 0;
        self.detail_open = false;
    }

    pub(super) fn toggle_pause(&mut self) {
        if self.paused {
            self.resume();
        } else {
            let view = self.view();
            self.pause(&view);
        }
    }

    /// Move the cursor `delta` rows, pausing a live feed first so the rows
    /// hold still. Moving forward on a live feed does nothing: it already
    /// shows the newest event.
    pub(super) fn move_cursor(&mut self, delta: isize) {
        let view = self.view();
        if view.is_empty() || (!self.paused && delta >= 0) {
            return;
        }
        if !self.paused {
            self.pause(&view);
        }
        let last = view.len() - 1;
        let row = self
            .cursor_row(&view)
            .saturating_add_signed(delta)
            .min(last);
        self.place_cursor(&view, row);
    }

    /// Put the cursor on `row` of `view`, pausing a live feed.
    pub(super) fn select_row(&mut self, row: usize) {
        let view = self.view();
        if view.is_empty() {
            return;
        }
        if !self.paused {
            self.pause(&view);
        }
        self.place_cursor(&view, row.min(view.len() - 1));
    }

    fn place_cursor(&mut self, view: &[usize], row: usize) {
        self.cursor = view[row];
        if row + 1 == view.len() {
            self.unseen = 0;
        }
    }

    /// Jump to the oldest (`false`) or newest (`true`) event.
    pub(super) fn jump(&mut self, newest: bool) {
        let rows = self.view().len();
        if rows > 0 {
            self.select_row(if newest { rows - 1 } else { 0 });
        }
    }

    /// Step the category filter `step` places along its strip, wrapping.
    pub(super) fn step_category(&mut self, step: isize) {
        let all = super::CATEGORIES;
        let count = all.len() + 1;
        let at = self
            .category
            .and_then(|category| all.iter().position(|each| *each == category))
            .map_or(0, |index| index + 1);
        let next = at
            .checked_add_signed(step.rem_euclid(isize::try_from(count).unwrap_or(1)))
            .unwrap_or(0)
            % count;
        self.set_category(next.checked_sub(1).map(|index| all[index]));
    }

    pub(super) fn set_category(&mut self, category: Option<EventCategory>) {
        self.category = category;
        self.refilter();
    }

    /// After a filter change the cursor's event may be hidden: a detail
    /// for it closes, and the cursor settles on the nearest shown event.
    fn refilter(&mut self) {
        if !self.paused {
            return;
        }
        let view = self.view();
        if view.binary_search(&self.cursor).is_err() {
            self.detail_open = false;
        }
        if let Some(index) = view.get(self.cursor_row(&view)) {
            self.cursor = *index;
        }
    }

    /// Take one event into the feed. Returns whether it was accepted.
    fn receive(&mut self, event: &Arc<Event>, live: bool) -> bool {
        let Some(landed) = self.feed.insert(Arc::clone(event)) else {
            return false;
        };
        if !self.paused {
            return true;
        }
        // The cursor names a feed index, so it follows its event when one
        // lands before it or the oldest falls off the front.
        let mut cursor = self.cursor;
        if landed.evicted {
            cursor = cursor.saturating_sub(1);
        }
        if landed.at <= cursor && self.feed.len() > 1 {
            cursor += 1;
        } else if live && self.passes(event, &self.search.to_lowercase()) {
            self.unseen += 1;
        }
        self.cursor = cursor.min(self.feed.len().saturating_sub(1));
        true
    }

    pub(super) fn apply_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::EventReceived(event) => {
                self.receive(event, true);
            }
            Action::EventHistoryLoaded {
                within_hours,
                include_firewall,
                events,
            } => self.history_loaded(*within_hours, *include_firewall, events),
            Action::EventHistoryFailed(message) => {
                self.history.loading = None;
                self.history.note = Some(format!("history: {message}"));
            }
            Action::ToggleEventPause => self.toggle_pause(),
            Action::AlarmsLoaded(alarms) => self.alarms.loaded(alarms),
            Action::AlarmsFailed(message) => self.alarms.failed(message),
            Action::SearchInput(query) => {
                self.search.clone_from(query);
                self.refilter();
            }
            Action::CloseSearch => {
                self.search.clear();
                self.refilter();
            }
            Action::AccessUpdated(access) => self.access = Some(*access),
            Action::Connecting
            | Action::Connected
            | Action::Reconnecting { .. }
            | Action::Disconnected(_) => self.link.follow(action),
            Action::DevicesUpdated(devices) => self.devices = Arc::clone(devices),
            Action::ClientsUpdated(clients) => self.clients = Arc::clone(clients),
            Action::ControllerChanged => {
                *self = Self {
                    focused: self.focused,
                    heatmap: self.heatmap,
                    ..Self::new()
                };
            }
            _ => {}
        }
        None
    }

    fn history_loaded(&mut self, within_hours: u32, include_firewall: bool, events: &[Arc<Event>]) {
        let mut added = 0_usize;
        for event in events {
            if !self.receive(event, false) {
                continue;
            }
            added += 1;
            if include_firewall && event.category == EventCategory::Firewall {
                self.firewall_extra.insert(EventKey::of(event));
            }
        }
        self.history.loading = None;
        let step = HISTORY_STEPS
            .iter()
            .position(|(hours, _)| *hours == within_hours);
        self.history.loaded = self.history.loaded.max(step);
        self.history.note = Some(match added {
            0 => "nothing more".into(),
            1 => "1 more event".into(),
            count => format!("{count} more events"),
        });
    }

    /// The next, wider history window to ask for: `(hours, limit)`.
    pub(super) fn next_history_window(&self) -> (u32, u32) {
        let step = self
            .history
            .loaded
            .map_or(0, |step| (step + 1).min(HISTORY_STEPS.len() - 1));
        HISTORY_STEPS[step]
    }

    /// The window the feed has been filled from so far: `(hours, limit)`.
    pub(super) fn history_window(&self) -> (u32, u32) {
        HISTORY_STEPS[self.history.loaded.unwrap_or(0)]
    }

    /// Take back out the firewall blocks a history load brought in.
    pub(super) fn drop_firewall_blocks(&mut self) -> usize {
        let extra = std::mem::take(&mut self.firewall_extra);
        let cursor_key = self.feed.get(self.cursor).map(|event| EventKey::of(event));
        let dropped = self
            .feed
            .remove_where(|event| extra.contains(&EventKey::of(event)));
        // Indices shifted: find the cursor's event again, or the one that
        // took its place.
        if self.paused && dropped > 0 {
            let events = self.feed.events();
            self.cursor = cursor_key
                .and_then(|key| events.iter().position(|event| EventKey::of(event) == key))
                .unwrap_or_else(|| self.cursor.min(events.len().saturating_sub(1)));
            self.refilter();
        }
        dropped
    }
}

/// A history window as the status line names it.
pub(super) fn window_label(hours: u32) -> String {
    if hours <= 48 {
        format!("{hours}h")
    } else {
        format!("{}d", hours / 24)
    }
}
