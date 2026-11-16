//! The event buffer both the Events screen and the Dashboard feed keep:
//! ordered by time, free of duplicates, and bounded.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use unifly_api::Event;

/// What makes two deliveries the same event. The controller replays its
/// backlog on every connect, and a history load overlaps the live feed,
/// so the same event arrives more than once.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(in crate::tui::screens) enum EventKey {
    Id(String),
    /// Events the controller gave no id: the same instant, type, text, and
    /// device and client, as the library's own event store tells them.
    Content {
        at: i64,
        event_type: String,
        message: String,
        device: Option<String>,
        client: Option<String>,
    },
}

impl EventKey {
    pub(in crate::tui::screens) fn of(event: &Event) -> Self {
        match &event.id {
            Some(id) => Self::Id(id.to_string()),
            None => Self::Content {
                at: event.timestamp.timestamp_millis(),
                event_type: event.event_type.clone(),
                message: event.message.clone(),
                device: event.device_mac.as_ref().map(ToString::to_string),
                client: event.client_mac.as_ref().map(ToString::to_string),
            },
        }
    }
}

/// Where an accepted event landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::tui::screens) struct Inserted {
    /// Its index once the buffer settled.
    pub at: usize,
    /// Whether the oldest event was dropped to make room.
    pub evicted: bool,
}

/// Events oldest first.
pub(in crate::tui::screens) struct Feed {
    events: VecDeque<Arc<Event>>,
    keys: HashSet<EventKey>,
    capacity: usize,
}

impl Feed {
    pub(in crate::tui::screens) fn new(capacity: usize) -> Self {
        Self {
            events: VecDeque::new(),
            keys: HashSet::new(),
            capacity: capacity.max(1),
        }
    }

    pub(in crate::tui::screens) fn events(&self) -> &VecDeque<Arc<Event>> {
        &self.events
    }

    pub(in crate::tui::screens) fn len(&self) -> usize {
        self.events.len()
    }

    pub(in crate::tui::screens) fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// How many events the buffer keeps.
    pub(in crate::tui::screens) fn capacity(&self) -> usize {
        self.capacity
    }

    /// Whether another event can only come in by pushing the oldest out.
    pub(in crate::tui::screens) fn is_full(&self) -> bool {
        self.events.len() >= self.capacity
    }

    pub(in crate::tui::screens) fn get(&self, index: usize) -> Option<&Arc<Event>> {
        self.events.get(index)
    }

    /// Place `event` by its timestamp, after any event of the same instant.
    /// A repeat delivery, or an event older than everything a full buffer
    /// holds, is dropped.
    pub(in crate::tui::screens) fn insert(&mut self, event: Arc<Event>) -> Option<Inserted> {
        let at = self
            .events
            .partition_point(|held| held.timestamp <= event.timestamp);
        let full = self.events.len() >= self.capacity;
        if full && at == 0 {
            return None;
        }
        if !self.keys.insert(EventKey::of(&event)) {
            return None;
        }
        self.events.insert(at, event);
        if !full {
            return Some(Inserted { at, evicted: false });
        }
        if let Some(oldest) = self.events.pop_front() {
            self.keys.remove(&EventKey::of(&oldest));
        }
        Some(Inserted {
            at: at - 1,
            evicted: true,
        })
    }

    /// Drop every event `drop` picks, returning how many went.
    pub(in crate::tui::screens) fn remove_where(
        &mut self,
        mut drop: impl FnMut(&Event) -> bool,
    ) -> usize {
        let before = self.events.len();
        let keys = &mut self.keys;
        self.events.retain(|event| {
            let gone = drop(event);
            if gone {
                keys.remove(&EventKey::of(event));
            }
            !gone
        });
        before - self.events.len()
    }
}

#[cfg(test)]
pub(in crate::tui::screens) mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use unifly_api::Event;

    use super::{Feed, Inserted};

    /// A client event `minute` minutes past nine on 12 October, with an id
    /// unless `id` is `None`.
    pub(in crate::tui::screens) fn event_at(minute: u32, id: Option<&str>) -> Arc<Event> {
        Arc::new(
            serde_json::from_value(json!({
                "id": id,
                "timestamp": format!("2026-10-12T09:{minute:02}:00Z"),
                "category": "Client",
                "severity": "Info",
                "event_type": "EVT_WU_Connected",
                "message": format!("client-{minute:02} connected"),
                "device_mac": null,
                "client_mac": null,
                "site_id": null,
                "raw_key": null
            }))
            .expect("event fixture should deserialize"),
        )
    }

    pub(in crate::tui::screens) fn event(minute: u32) -> Arc<Event> {
        event_at(minute, Some(&format!("evt-{minute}")))
    }

    fn messages(feed: &Feed) -> Vec<&str> {
        feed.events()
            .iter()
            .map(|event| event.message.as_str())
            .collect()
    }

    #[test]
    fn a_replayed_backlog_adds_nothing() {
        let mut feed = Feed::new(100);
        for minute in 0..5 {
            assert!(feed.insert(event(minute)).is_some());
        }
        for minute in 0..5 {
            assert!(feed.insert(event(minute)).is_none(), "minute {minute}");
        }
        assert_eq!(feed.len(), 5);
    }

    #[test]
    fn events_without_an_id_are_matched_by_what_they_say() {
        let mut feed = Feed::new(100);
        assert!(feed.insert(event_at(3, None)).is_some());
        assert!(feed.insert(event_at(3, None)).is_none());
        assert!(feed.insert(event_at(4, None)).is_some());
        assert_eq!(feed.len(), 2);
    }

    #[test]
    fn a_live_event_and_its_polled_copy_are_one_row() {
        // The frame the live stream sends, then the row a poll lists for
        // the same event: same `_id`, but no MACs and a rendered message.
        let live = Event::from(
            serde_json::from_value::<unifly_api::websocket::UnifiEvent>(json!({
                "_id": "66f0c0ffee0000000000abcd",
                "key": "EVT_WU_Connected",
                "subsystem": "wlan",
                "site_id": "site1",
                "datetime": "2026-10-12T09:03:00Z",
                "msg": "User[aa:bb:cc:dd:ee:01] has connected",
                "user": "aa:bb:cc:dd:ee:01"
            }))
            .expect("frame"),
        );
        assert!(live.id.is_some(), "the frame's id is kept");
        let polled = event_at(3, Some("66f0c0ffee0000000000abcd"));

        let mut feed = Feed::new(100);
        assert!(feed.insert(Arc::new(live)).is_some());
        assert!(feed.insert(polled).is_none());
        assert_eq!(feed.len(), 1);
    }

    #[test]
    fn events_without_an_id_differ_by_the_device_or_client_they_name() {
        let named = |client: &str| {
            let mut event = (*event_at(3, None)).clone();
            event.message = "a client connected".into();
            event.client_mac = Some(unifly_api::MacAddress::new(client));
            Arc::new(event)
        };
        let mut feed = Feed::new(100);
        assert!(feed.insert(named("aa:bb:cc:dd:ee:01")).is_some());
        assert!(feed.insert(named("aa:bb:cc:dd:ee:02")).is_some());
        assert!(feed.insert(named("aa:bb:cc:dd:ee:01")).is_none());
        assert_eq!(feed.len(), 2);
    }

    #[test]
    fn a_batch_delivered_newest_first_lands_oldest_first() {
        let mut feed = Feed::new(100);
        for minute in [7, 5, 6, 2] {
            feed.insert(event(minute));
        }
        assert_eq!(
            messages(&feed),
            [
                "client-02 connected",
                "client-05 connected",
                "client-06 connected",
                "client-07 connected",
            ]
        );
    }

    #[test]
    fn a_full_feed_drops_its_oldest_event_and_frees_its_key() {
        let mut feed = Feed::new(3);
        for minute in 1..=3 {
            feed.insert(event(minute));
        }
        assert_eq!(
            feed.insert(event(4)),
            Some(Inserted {
                at: 2,
                evicted: true
            })
        );
        assert_eq!(
            messages(&feed),
            [
                "client-02 connected",
                "client-03 connected",
                "client-04 connected"
            ]
        );
        // Older than everything held: nothing to show it under.
        assert!(feed.insert(event(0)).is_none());
        assert_eq!(feed.len(), 3);
        // The evicted event's key is free again once there is room.
        feed.remove_where(|event| event.message.starts_with("client-04"));
        assert!(feed.insert(event(1)).is_some());
    }

    #[test]
    fn removing_events_frees_their_keys() {
        let mut feed = Feed::new(10);
        for minute in 0..4 {
            feed.insert(event(minute));
        }
        assert_eq!(feed.remove_where(|event| event.message.contains("-02")), 1);
        assert_eq!(feed.len(), 3);
        assert!(feed.insert(event(2)).is_some());
    }
}
