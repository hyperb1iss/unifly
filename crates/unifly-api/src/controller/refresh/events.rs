use std::collections::HashSet;

use crate::model::Event;
use crate::store::{DataStore, event_storage_key};

/// The events in `events` the store has not seen, oldest first: subscribers
/// append what is broadcast, and the controller lists newest first.
pub(super) fn unseen_events(store: &DataStore, events: &[Event]) -> Vec<Event> {
    let mut seen: HashSet<String> = store
        .events_snapshot()
        .iter()
        .map(|event| event_storage_key(event))
        .collect();

    let mut fresh: Vec<Event> = events
        .iter()
        .filter(|event| seen.insert(event_storage_key(event)))
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
