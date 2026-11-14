use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::sync::Arc;

use unifly_api::Event;
use unifly_api::model::EventCategory;

use super::feed::{EventKey, Feed};
use super::{CAPACITY, EventsScreen, HISTORY_STEPS, History, HistoryLoad, PendingHistory};
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
            anchor: Cell::new(0),
            page: Cell::new(10),
            unseen: 0,
            category: None,
            search: String::new(),
            detail_open: false,
            heatmap: None,
            heatmap_shown: Cell::new(false),
            heatmap_fits: Cell::new(true),
            chips_shown: Cell::new(true),
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

    /// Whether the filters let any event through.
    pub(super) fn any_listed(&self) -> bool {
        let needle = self.search.to_lowercase();
        self.feed
            .events()
            .iter()
            .any(|event| self.passes(event, &needle))
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
        // The cursor and the first drawn row name feed indices, so each
        // follows its event when one lands before it or the oldest falls
        // off the front.
        let last = self.feed.len().saturating_sub(1);
        let follow = |index: usize| {
            let index = if landed.evicted {
                index.saturating_sub(1)
            } else {
                index
            };
            let pushed = landed.at <= index && last > 0;
            ((index + usize::from(pushed)).min(last), pushed)
        };
        // The cursor's own event fell off the front: a detail open on it
        // would go on to describe the event that took its place.
        if landed.evicted && self.cursor == 0 {
            self.detail_open = false;
        }
        let (cursor, pushed) = follow(self.cursor);
        if !pushed && live && self.passes(event, &self.search.to_lowercase()) {
            self.unseen += 1;
        }
        self.cursor = cursor;
        self.anchor.set(follow(self.anchor.get()).0);
        true
    }

    pub(super) fn apply_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::EventReceived(event) => {
                self.receive(event, true);
            }
            Action::EventHistoryLoaded { id, query, events } => {
                return self.history_loaded(*id, query, events);
            }
            Action::EventHistoryFailed { id, message } => {
                if self.history.loading.is_some_and(|load| load.id == *id) {
                    self.history.loading = None;
                    self.history.note = Some(format!("history: {message}"));
                }
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

    /// Merge what a history load returned. A result nobody is waiting for
    /// (the load was cancelled, or a newer one replaced it) is dropped.
    fn history_loaded(
        &mut self,
        id: u64,
        query: &unifly_api::EventHistoryQuery,
        events: &[Arc<Event>],
    ) -> Option<Action> {
        let pending = self.history.loading.filter(|load| load.id == id)?;
        self.history.loading = None;
        self.history.loaded = true;
        let mut added = 0_usize;
        for event in events {
            if !self.receive(event, false) {
                continue;
            }
            added += 1;
            if query.include_firewall && event.category == EventCategory::Firewall {
                self.firewall_extra.insert(EventKey::of(event));
            }
        }
        // Blocks are on once a load that asked for them has come back.
        self.history.include_firewall |= query.include_firewall;
        let hit_limit = events.len() >= usize::try_from(query.limit).unwrap_or(usize::MAX);
        if pending.load == HistoryLoad::Firewall {
            let blocks = self.firewall_extra.len();
            self.history.note = Some(match (blocks, hit_limit) {
                (0, _) => "no firewall blocks in this span".into(),
                (1, false) => "1 firewall block".into(),
                (blocks, false) => format!("{blocks} firewall blocks"),
                (blocks, true) => format!("{blocks} firewall blocks, limit reached"),
            });
            return None;
        }

        let last_step = HISTORY_STEPS.len() - 1;
        if added == 0 && !self.feed.is_full() && self.history.step < last_step {
            // Nothing in this window: look further back without making
            // the user ask again.
            self.history.step += 1;
            return Some(self.request_older());
        }
        if !hit_limit {
            // The window is spent, so the next load reaches further.
            self.history.step = (self.history.step + 1).min(last_step);
        }
        self.history.note = Some(if self.feed.is_full() {
            self.cap_note()
        } else {
            match (added, hit_limit) {
                (0, _) => format!("no older events in {}", window_label(query.within_hours)),
                (1, false) => "1 more event".into(),
                (count, false) => format!("{count} more events"),
                (count, true) => format!("{count} more events, o for more"),
            }
        });
        None
    }

    /// Ask for the window of history before the oldest event held.
    pub(super) fn request_older(&mut self) -> Action {
        let (within_hours, limit) = HISTORY_STEPS[self.history.step];
        self.request_history(
            HistoryLoad::Older,
            unifly_api::EventHistoryQuery {
                within_hours,
                limit,
                include_firewall: self.history.include_firewall,
                until: self.feed.events().front().map(|oldest| oldest.timestamp),
            },
        )
    }

    /// Ask for the firewall blocks of the span the feed covers.
    pub(super) fn request_firewall_blocks(&mut self) -> Action {
        let (widest, limit) = HISTORY_STEPS[HISTORY_STEPS.len() - 1];
        let span_hours = self.feed.events().front().map_or(24, |oldest| {
            let hours = (chrono::Utc::now() - oldest.timestamp).num_hours() + 1;
            u32::try_from(hours).unwrap_or(widest).clamp(1, widest)
        });
        self.request_history(
            HistoryLoad::Firewall,
            unifly_api::EventHistoryQuery {
                within_hours: span_hours,
                limit,
                include_firewall: true,
                until: None,
            },
        )
    }

    fn request_history(
        &mut self,
        load: HistoryLoad,
        query: unifly_api::EventHistoryQuery,
    ) -> Action {
        self.history.last_id += 1;
        let id = self.history.last_id;
        self.history.loading = Some(PendingHistory {
            id,
            load,
            hours: query.within_hours,
        });
        self.history.note = None;
        Action::RequestEventHistory { id, query }
    }

    /// What the title bar says when the feed is full, which is why
    /// nothing older can be shown.
    pub(super) fn cap_note(&self) -> String {
        format!(
            "limit reached: the newest {} events are kept",
            self.feed.capacity()
        )
    }

    /// Stop waiting for the history load in flight.
    pub(super) fn cancel_history(&mut self) -> Action {
        self.history.loading = None;
        self.history.note = Some("load cancelled".into());
        Action::CancelEventHistory
    }

    /// When the oldest event held happened, once history has been loaded.
    pub(super) fn history_reach(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.history
            .loaded
            .then(|| self.feed.events().front().map(|oldest| oldest.timestamp))
            .flatten()
    }

    /// Take back out the firewall blocks a history load brought in.
    pub(super) fn drop_firewall_blocks(&mut self) -> usize {
        let extra = std::mem::take(&mut self.firewall_extra);
        let key_at = |feed: &Feed, index: usize| feed.get(index).map(|event| EventKey::of(event));
        let cursor_key = key_at(&self.feed, self.cursor);
        let anchor_key = key_at(&self.feed, self.anchor.get());
        let dropped = self
            .feed
            .remove_where(|event| extra.contains(&EventKey::of(event)));
        // Indices shifted: find the cursor's event and the first drawn
        // row's again, or the ones that took their places.
        if self.paused && dropped > 0 {
            let events = self.feed.events();
            let find = |key: Option<EventKey>, was: usize| {
                key.and_then(|key| events.iter().position(|event| EventKey::of(event) == key))
                    .unwrap_or_else(|| was.min(events.len().saturating_sub(1)))
            };
            self.cursor = find(cursor_key, self.cursor);
            self.anchor.set(find(anchor_key, self.anchor.get()));
            self.refilter();
        }
        dropped
    }
}

/// A history window as the title bar names it.
pub(super) fn window_label(hours: u32) -> String {
    if hours <= 48 {
        format!("{hours}h")
    } else {
        format!("{}d", hours / 24)
    }
}
