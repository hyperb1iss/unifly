// ── Full refresh application logic ──
//
// Applies bulk data snapshots from the Integration and Session API
// into the DataStore. Integration data is primary; Session fills gaps.

use std::collections::HashSet;

use chrono::Utc;

use super::DataStore;
use super::collection::EntityCollection;
use crate::model::{
    AclRule, Client, Device, DnsPolicy, EntityId, Event, FirewallGroup, FirewallPolicy,
    FirewallZone, NatPolicy, Network, Site, TrafficMatchingList, Voucher, WifiBroadcast,
};

/// Upsert all incoming entities, then prune any existing keys not in the
/// incoming set. This avoids the brief empty state that `clear()` causes.
fn upsert_and_prune<T: Clone + Send + Sync + 'static>(
    collection: &EntityCollection<T>,
    items: Vec<(String, EntityId, T)>,
) {
    let _batch = collection.begin_batch();
    let incoming_keys: HashSet<String> = items.iter().map(|(k, _, _)| k.clone()).collect();
    for (key, id, entity) in items {
        collection.upsert(key, id, entity);
    }
    for existing_key in collection.keys() {
        if !incoming_keys.contains(&existing_key) {
            collection.remove(&existing_key);
        }
    }
}

/// Apply one collection from a refresh snapshot. `Some` is authoritative and
/// goes through [`upsert_and_prune`], so `Some(empty)` clears the collection.
/// `None` means the fetch failed, and the collection keeps its last known
/// contents untouched.
fn sync_collection<T: Clone + Send + Sync + 'static>(
    collection: &EntityCollection<T>,
    items: Option<Vec<T>>,
    keyed: impl Fn(&T) -> (String, EntityId),
) {
    let Some(items) = items else {
        return;
    };
    upsert_and_prune(
        collection,
        items
            .into_iter()
            .map(|item| {
                let (key, id) = keyed(&item);
                (key, id, item)
            })
            .collect(),
    );
}

/// All collections fetched during a single refresh cycle.
///
/// Plain `Vec` fields come from required fetches, whose failure aborts the
/// whole refresh before a snapshot exists. `Option` fields come from
/// optional fetches: `None` means that fetch failed and the stored
/// collection must be left as is, never treated as empty.
pub(crate) struct RefreshSnapshot {
    pub devices: Vec<Device>,
    pub clients: Vec<Client>,
    pub networks: Vec<Network>,
    pub wifi: Vec<WifiBroadcast>,
    pub policies: Option<Vec<FirewallPolicy>>,
    pub zones: Option<Vec<FirewallZone>>,
    pub acls: Option<Vec<AclRule>>,
    pub nat: Option<Vec<NatPolicy>>,
    pub dns: Option<Vec<DnsPolicy>>,
    pub vouchers: Option<Vec<Voucher>>,
    pub sites: Vec<Site>,
    pub events: Option<Vec<Event>>,
    pub traffic_matching_lists: Vec<TrafficMatchingList>,
    pub firewall_groups: Option<Vec<FirewallGroup>>,
}

pub(crate) fn event_storage_key(event: &Event) -> String {
    event.id.as_ref().map_or_else(
        || {
            format!(
                "evt:{}:{}:{}:{}:{}:{}:{}",
                event.timestamp.timestamp_millis(),
                event.raw_key.as_deref().unwrap_or_default(),
                event.event_type,
                event.message,
                event
                    .site_id
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                event
                    .device_mac
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                event
                    .client_mac
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
            )
        },
        std::string::ToString::to_string,
    )
}

pub(crate) fn event_storage_id(event: &Event, key: &str) -> EntityId {
    event
        .id
        .clone()
        .unwrap_or_else(|| EntityId::Legacy(key.to_owned()))
}

impl DataStore {
    /// Apply a full refresh snapshot.
    ///
    /// Uses upsert-then-prune: incoming entities are upserted first, then
    /// any keys not present in the incoming set are removed. This avoids the
    /// brief "empty" state that a clear-then-insert approach would cause.
    /// Collections whose optional fetch failed (`None`) are skipped.
    pub(crate) fn apply_integration_snapshot(&self, snap: RefreshSnapshot) {
        sync_collection(&self.devices, Some(snap.devices), |d| {
            (d.mac.as_str().to_owned(), d.id.clone())
        });
        sync_collection(&self.clients, Some(snap.clients), |c| {
            (c.mac.as_str().to_owned(), c.id.clone())
        });
        sync_collection(&self.networks, Some(snap.networks), |n| {
            (format!("net:{}", n.id), n.id.clone())
        });
        sync_collection(&self.wifi_broadcasts, Some(snap.wifi), |wb| {
            (format!("wifi:{}", wb.id), wb.id.clone())
        });
        sync_collection(&self.firewall_policies, snap.policies, |p| {
            (format!("fwp:{}", p.id), p.id.clone())
        });
        sync_collection(&self.firewall_zones, snap.zones, |z| {
            (format!("fwz:{}", z.id), z.id.clone())
        });
        sync_collection(&self.acl_rules, snap.acls, |a| {
            (format!("acl:{}", a.id), a.id.clone())
        });
        sync_collection(&self.nat_policies, snap.nat, |n| {
            (format!("nat:{}", n.id), n.id.clone())
        });
        sync_collection(&self.dns_policies, snap.dns, |d| {
            (format!("dns:{}", d.id), d.id.clone())
        });
        sync_collection(&self.vouchers, snap.vouchers, |v| {
            (format!("vch:{}", v.id), v.id.clone())
        });
        sync_collection(&self.sites, Some(snap.sites), |s| {
            (format!("site:{}", s.id), s.id.clone())
        });
        sync_collection(&self.events, snap.events, |e| {
            let key = event_storage_key(e);
            let id = event_storage_id(e, &key);
            (key, id)
        });
        sync_collection(
            &self.traffic_matching_lists,
            Some(snap.traffic_matching_lists),
            |t| (format!("tml:{}", t.id), t.id.clone()),
        );
        sync_collection(&self.firewall_groups, snap.firewall_groups, |g| {
            (format!("fwg:{}", g.id), g.id.clone())
        });

        self.last_full_refresh.send_replace(Some(Utc::now()));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::model::{EventCategory, EventSeverity};
    use chrono::{TimeZone, Utc};

    #[test]
    fn upsert_and_prune_batches_snapshot_updates() {
        let collection: EntityCollection<String> = EntityCollection::new();
        collection.upsert("stale".into(), EntityId::from("stale"), "old".into());

        let version_rx = collection.version_receiver();
        let start_version = *version_rx.borrow();

        upsert_and_prune(
            &collection,
            vec![
                ("keep-a".into(), EntityId::from("a"), "one".into()),
                ("keep-b".into(), EntityId::from("b"), "two".into()),
            ],
        );

        assert_eq!(*version_rx.borrow(), start_version + 1);
        assert_eq!(collection.len(), 2);
        assert!(collection.get_by_key("stale").is_none());
        assert_eq!(collection.snapshot().len(), 2);
    }

    /// A snapshot whose required collections are empty and whose optional
    /// collections all failed to fetch.
    fn failed_optional_snapshot() -> RefreshSnapshot {
        RefreshSnapshot {
            devices: Vec::new(),
            clients: Vec::new(),
            networks: Vec::new(),
            wifi: Vec::new(),
            policies: None,
            zones: None,
            acls: None,
            nat: None,
            dns: None,
            vouchers: None,
            sites: Vec::new(),
            events: None,
            traffic_matching_lists: Vec::new(),
            firewall_groups: None,
        }
    }

    fn test_event(message: &str) -> Event {
        Event {
            id: None,
            timestamp: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            category: EventCategory::System,
            severity: EventSeverity::Info,
            event_type: "EVT_TEST".into(),
            message: message.into(),
            device_mac: None,
            client_mac: None,
            site_id: None,
            raw_key: Some("EVT_TEST".into()),
            source: crate::model::common::DataSource::SessionApi,
        }
    }

    #[test]
    fn sync_collection_skips_failed_fetch_and_prunes_authoritative_empty() {
        let string_key = |item: &String| (item.clone(), EntityId::from(item.clone()));
        let collection: EntityCollection<String> = EntityCollection::new();
        sync_collection(&collection, Some(vec!["a".into(), "b".into()]), string_key);
        assert_eq!(collection.len(), 2);

        let version_rx = collection.version_receiver();
        let before = *version_rx.borrow();
        sync_collection(&collection, None, string_key);
        assert_eq!(collection.len(), 2);
        assert_eq!(
            *version_rx.borrow(),
            before,
            "a failed fetch must not mutate"
        );

        sync_collection(&collection, Some(Vec::new()), string_key);
        assert_eq!(collection.len(), 0);
    }

    #[test]
    fn failed_optional_fetches_keep_last_known_entries() {
        let store = DataStore::new();
        let nat = crate::convert::nat_policy_from_v2(&serde_json::json!({
            "_id": "nat-1",
            "type": "DNAT",
            "description": "web",
        }))
        .unwrap();
        let group = crate::convert::firewall_group_from_session(&serde_json::json!({
            "_id": "fwg-1",
            "name": "web-ports",
        }))
        .unwrap();

        store.apply_integration_snapshot(RefreshSnapshot {
            nat: Some(vec![nat]),
            events: Some(vec![test_event("first")]),
            firewall_groups: Some(vec![group]),
            ..failed_optional_snapshot()
        });
        assert_eq!(store.nat_policies_snapshot().len(), 1);
        assert_eq!(store.events_snapshot().len(), 1);
        assert_eq!(store.firewall_groups_snapshot().len(), 1);

        store.apply_integration_snapshot(failed_optional_snapshot());
        assert_eq!(store.nat_policies_snapshot().len(), 1);
        assert_eq!(store.events_snapshot().len(), 1);
        assert_eq!(store.firewall_groups_snapshot().len(), 1);

        store.apply_integration_snapshot(RefreshSnapshot {
            nat: Some(Vec::new()),
            events: Some(Vec::new()),
            firewall_groups: Some(Vec::new()),
            ..failed_optional_snapshot()
        });
        assert_eq!(store.nat_policies_snapshot().len(), 0);
        assert_eq!(store.events_snapshot().len(), 0);
        assert_eq!(store.firewall_groups_snapshot().len(), 0);
    }

    #[test]
    fn event_snapshot_keeps_distinct_id_less_events_with_same_timestamp() {
        let store = DataStore::new();

        store.apply_integration_snapshot(RefreshSnapshot {
            events: Some(vec![test_event("first"), test_event("second")]),
            ..failed_optional_snapshot()
        });

        assert_eq!(store.events_snapshot().len(), 2);
    }
}
