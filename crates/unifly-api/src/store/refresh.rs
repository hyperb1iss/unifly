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

/// All collections fetched during a single Integration API refresh cycle.
///
/// Bundles the 12 entity vectors that `apply_integration_snapshot` needs,
/// keeping the function signature manageable.
pub(crate) struct RefreshSnapshot {
    pub devices: Vec<Device>,
    pub clients: Vec<Client>,
    pub networks: Vec<Network>,
    pub wifi: Vec<WifiBroadcast>,
    pub policies: Vec<FirewallPolicy>,
    pub zones: Vec<FirewallZone>,
    pub acls: Vec<AclRule>,
    pub nat: Vec<NatPolicy>,
    pub dns: Vec<DnsPolicy>,
    pub vouchers: Vec<Voucher>,
    pub sites: Vec<Site>,
    pub events: Vec<Event>,
    pub traffic_matching_lists: Vec<TrafficMatchingList>,
    pub firewall_groups: Vec<FirewallGroup>,
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
    /// Apply a full Integration API data refresh.
    ///
    /// Uses upsert-then-prune: incoming entities are upserted first, then
    /// any keys not present in the incoming set are removed. This avoids the
    /// brief "empty" state that a clear-then-insert approach would cause.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn apply_integration_snapshot(&self, snap: RefreshSnapshot) {
        upsert_and_prune(
            &self.devices,
            snap.devices
                .into_iter()
                .map(|d| {
                    let key = d.mac.as_str().to_owned();
                    let id = d.id.clone();
                    (key, id, d)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.clients,
            snap.clients
                .into_iter()
                .map(|c| {
                    let key = c.mac.as_str().to_owned();
                    let id = c.id.clone();
                    (key, id, c)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.networks,
            snap.networks
                .into_iter()
                .map(|n| {
                    let key = format!("net:{}", n.id);
                    let id = n.id.clone();
                    (key, id, n)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.wifi_broadcasts,
            snap.wifi
                .into_iter()
                .map(|wb| {
                    let key = format!("wifi:{}", wb.id);
                    let id = wb.id.clone();
                    (key, id, wb)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.firewall_policies,
            snap.policies
                .into_iter()
                .map(|p| {
                    let key = format!("fwp:{}", p.id);
                    let id = p.id.clone();
                    (key, id, p)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.firewall_zones,
            snap.zones
                .into_iter()
                .map(|z| {
                    let key = format!("fwz:{}", z.id);
                    let id = z.id.clone();
                    (key, id, z)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.acl_rules,
            snap.acls
                .into_iter()
                .map(|a| {
                    let key = format!("acl:{}", a.id);
                    let id = a.id.clone();
                    (key, id, a)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.nat_policies,
            snap.nat
                .into_iter()
                .map(|n| {
                    let key = format!("nat:{}", n.id);
                    let id = n.id.clone();
                    (key, id, n)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.dns_policies,
            snap.dns
                .into_iter()
                .map(|d| {
                    let key = format!("dns:{}", d.id);
                    let id = d.id.clone();
                    (key, id, d)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.vouchers,
            snap.vouchers
                .into_iter()
                .map(|v| {
                    let key = format!("vch:{}", v.id);
                    let id = v.id.clone();
                    (key, id, v)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.sites,
            snap.sites
                .into_iter()
                .map(|s| {
                    let key = format!("site:{}", s.id);
                    let id = s.id.clone();
                    (key, id, s)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.events,
            snap.events
                .into_iter()
                .map(|e| {
                    let key = event_storage_key(&e);
                    let id = event_storage_id(&e, &key);
                    (key, id, e)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.traffic_matching_lists,
            snap.traffic_matching_lists
                .into_iter()
                .map(|t| {
                    let key = format!("tml:{}", t.id);
                    let id = t.id.clone();
                    (key, id, t)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.firewall_groups,
            snap.firewall_groups
                .into_iter()
                .map(|g| {
                    let key = format!("fwg:{}", g.id);
                    let id = g.id.clone();
                    (key, id, g)
                })
                .collect(),
        );

        let _ = self.last_full_refresh.send(Some(Utc::now()));
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

    #[test]
    fn event_snapshot_keeps_distinct_id_less_events_with_same_timestamp() {
        let store = DataStore::new();
        let timestamp = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();

        store.apply_integration_snapshot(RefreshSnapshot {
            devices: Vec::new(),
            clients: Vec::new(),
            networks: Vec::new(),
            wifi: Vec::new(),
            policies: Vec::new(),
            zones: Vec::new(),
            acls: Vec::new(),
            nat: Vec::new(),
            dns: Vec::new(),
            vouchers: Vec::new(),
            sites: Vec::new(),
            events: vec![
                Event {
                    id: None,
                    timestamp,
                    category: EventCategory::System,
                    severity: EventSeverity::Info,
                    event_type: "EVT_TEST".into(),
                    message: "first".into(),
                    device_mac: None,
                    client_mac: None,
                    site_id: None,
                    raw_key: Some("EVT_TEST".into()),
                    source: crate::model::common::DataSource::SessionApi,
                },
                Event {
                    id: None,
                    timestamp,
                    category: EventCategory::System,
                    severity: EventSeverity::Info,
                    event_type: "EVT_TEST".into(),
                    message: "second".into(),
                    device_mac: None,
                    client_mac: None,
                    site_id: None,
                    raw_key: Some("EVT_TEST".into()),
                    source: crate::model::common::DataSource::SessionApi,
                },
            ],
            traffic_matching_lists: Vec::new(),
            firewall_groups: Vec::new(),
        });

        assert_eq!(store.events_snapshot().len(), 2);
    }
}
