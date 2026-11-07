use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

use chrono::{DateTime, Utc};

use crate::model::Event;

/// Keep only the best requested rows while raw pagination continues to verify
/// completeness. The largest (worst) key is evicted first.
pub(super) struct RetainedEvents {
    events: BinaryHeap<RankedEvent>,
    limit: usize,
    next_sequence: usize,
}

impl RetainedEvents {
    pub(super) fn new(limit: u32) -> Self {
        Self {
            events: BinaryHeap::new(),
            limit: usize::try_from(limit).unwrap_or(usize::MAX),
            next_sequence: 0,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.events.len()
    }

    pub(super) fn push(&mut self, event: Event) {
        if self.limit == 0 {
            return;
        }
        let ranked = RankedEvent::new(event, self.next_sequence);
        self.next_sequence += 1;
        if self.events.len() < self.limit {
            self.events.push(ranked);
        } else if let Some(mut worst) = self.events.peek_mut()
            && ranked < *worst
        {
            *worst = ranked;
        }
    }

    pub(super) fn finish(self) -> Vec<Event> {
        self.events
            .into_sorted_vec()
            .into_iter()
            .map(|ranked| ranked.event)
            .collect()
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct EventOrder {
    timestamp: Reverse<DateTime<Utc>>,
    missing_id: bool,
    id: Option<String>,
    event_type: String,
    message: String,
    raw_key: Option<String>,
    device_mac: Option<String>,
    client_mac: Option<String>,
    site_id: Option<String>,
    sequence: usize,
}

struct RankedEvent {
    order: EventOrder,
    event: Event,
}

impl RankedEvent {
    fn new(event: Event, sequence: usize) -> Self {
        Self {
            order: EventOrder {
                timestamp: Reverse(event.timestamp),
                missing_id: event.id.is_none(),
                id: event.id.as_ref().map(ToString::to_string),
                event_type: event.event_type.clone(),
                message: event.message.clone(),
                raw_key: event.raw_key.clone(),
                device_mac: event.device_mac.as_ref().map(|mac| mac.as_str().to_owned()),
                client_mac: event.client_mac.as_ref().map(|mac| mac.as_str().to_owned()),
                site_id: event.site_id.as_ref().map(ToString::to_string),
                sequence,
            },
            event,
        }
    }
}

impl PartialEq for RankedEvent {
    fn eq(&self, other: &Self) -> bool {
        self.order == other.order
    }
}

impl Eq for RankedEvent {}

impl PartialOrd for RankedEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RankedEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        self.order.cmp(&other.order)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::model::EventSeverity;
    use crate::session::system_log::SystemLogEntry;

    fn event(id: Option<&str>, timestamp: i64) -> Event {
        Event::try_from(
            serde_json::from_value::<SystemLogEntry>(json!({
                "id": id, "timestamp": timestamp, "message": "same"
            }))
            .expect("entry"),
        )
        .expect("event")
    }

    #[test]
    fn retaining_large_history_keeps_only_best_requested_rows() {
        let mut retained = RetainedEvents::new(3);
        for index in 0..10_000 {
            retained.push(event(Some(&format!("{index:05}")), index));
            assert!(retained.len() <= 3);
        }
        let result = retained.finish();
        let timestamps: Vec<_> = result
            .iter()
            .map(|event| event.timestamp.timestamp_millis())
            .collect();
        assert_eq!(timestamps, [9999, 9998, 9997]);
    }

    #[test]
    fn equal_sort_keys_keep_arrival_order_when_evicting_worst() {
        let mut retained = RetainedEvents::new(2);
        let mut first = event(Some("same"), 1000);
        first.severity = EventSeverity::Warning;
        let mut second = first.clone();
        second.severity = EventSeverity::Error;
        retained.push(first);
        retained.push(second);
        retained.push(event(Some("better"), 2000));
        let result = retained.finish();
        assert_eq!(
            result.last().expect("earlier tie survives").severity,
            EventSeverity::Warning
        );
    }
}
