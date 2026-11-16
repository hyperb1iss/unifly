use std::collections::HashSet;

use crate::model::Event;
use crate::store::{DataStore, event_storage_key};

/// The events in `events` no subscriber has been handed, oldest first:
/// subscribers append what is broadcast, and the controller lists newest
/// first. An event is known when the store holds it or when the live stream
/// already delivered it ahead of this listing.
pub(super) fn unseen_events(store: &DataStore, events: &[Event]) -> Vec<Event> {
    let mut seen: HashSet<String> = store
        .events_snapshot()
        .iter()
        .map(|event| event_storage_key(event))
        .collect();

    let mut fresh: Vec<Event> = events
        .iter()
        .filter(|event| {
            let key = event_storage_key(event);
            !store.live_event_seen(&key) && seen.insert(key)
        })
        .cloned()
        .collect();
    fresh.sort_by_key(|event| event.timestamp);
    fresh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str, timestamp: &str) -> Event {
        serde_json::from_value(serde_json::json!({
            "id": id, "timestamp": timestamp, "category": "Client",
            "severity": "Info", "event_type": "EVT_WU_Connected",
            "message": id, "device_mac": null, "client_mac": null,
            "site_id": null, "raw_key": null
        }))
        .expect("event fixture")
    }

    /// The frame the live stream delivers for a client connecting, and the
    /// row a later poll of `stat/event` lists for the same event.
    fn live_and_polled() -> (Event, Event) {
        let frame = serde_json::json!({
            "_id": "66f0c0ffee0000000000abcd",
            "key": "EVT_WU_Connected",
            "subsystem": "wlan",
            "site_id": "site1",
            "datetime": "2026-11-13T10:00:00Z",
            "msg": "User[aa:bb:cc:dd:ee:01] has connected",
            "user": "aa:bb:cc:dd:ee:01"
        });
        let live = Event::from(
            serde_json::from_value::<crate::websocket::UnifiEvent>(frame.clone()).expect("frame"),
        );
        let polled = crate::convert::system_log::legacy_history_event(
            serde_json::from_value(frame).expect("polled row"),
        )
        .expect("polled event");
        (live, polled)
    }

    #[test]
    fn a_live_event_is_not_handed_on_again_when_a_refresh_lists_it() {
        let store = DataStore::new();
        let (live, polled) = live_and_polled();

        assert!(store.note_live_event(&live), "the frame is news");
        assert!(!store.note_live_event(&live), "the same frame twice is not");
        let other = event("other", "2026-11-13T10:01:00Z");
        let fresh = unseen_events(&store, &[other.clone(), polled]);

        let order: Vec<&str> = fresh.iter().map(|event| event.message.as_str()).collect();
        assert_eq!(order, ["other"], "only the event the stream never sent");
    }

    #[test]
    fn a_frame_for_an_event_a_refresh_already_stored_is_not_news() {
        let store = DataStore::new();
        let (live, polled) = live_and_polled();
        let key = event_storage_key(&polled);
        let id = polled.id.clone().expect("a polled event has an id");
        store.events.upsert(key, id, polled);

        assert!(!store.note_live_event(&live));
    }

    #[test]
    fn fresh_events_are_handed_on_oldest_first() {
        // The controller lists newest first; subscribers append.
        let listed = [
            event("third", "2026-11-13T10:02:00Z"),
            event("second", "2026-11-13T10:01:00Z"),
            event("first", "2026-11-13T10:00:00Z"),
        ];
        let fresh = unseen_events(&DataStore::new(), &listed);
        let order: Vec<&str> = fresh.iter().map(|event| event.message.as_str()).collect();
        assert_eq!(order, ["first", "second", "third"]);
    }
}
