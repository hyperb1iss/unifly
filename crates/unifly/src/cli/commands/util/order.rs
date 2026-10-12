//! Deterministic ordering for lists read from controller snapshots.
//!
//! The controller store keeps entities in a concurrent map, so snapshot order
//! changes from run to run. Every list command that reads a snapshot sorts it
//! here before `apply_list_args` filters and pages, which keeps
//! `--offset`/`--limit` pages stable and disjoint and gives every output
//! format the same rows in the same order. The TUI list screens sort with
//! these same functions, so both surfaces show one order.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;

use unifly_api::model::{
    AclRule, Client, Device, DnsPolicy, EntityId, Event, FirewallGroup, FirewallPolicy,
    FirewallZone, NatPolicy, Network, Site, TrafficMatchingList, Voucher, WifiBroadcast,
};

/// Sort key for a display name: named items first, compared
/// case-insensitively, then blank or missing names.
fn name_key(name: Option<&str>) -> (bool, String) {
    match name.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => (false, name.to_lowercase()),
        None => (true, String::new()),
    }
}

/// Sort key that places present values in ascending order and missing
/// values after them.
fn present_first<T: Ord>(value: Option<T>) -> (bool, Option<T>) {
    (value.is_none(), value)
}

fn sorted<T, K: Ord>(items: &[Arc<T>], key: impl FnMut(&Arc<T>) -> K) -> Vec<Arc<T>> {
    let mut items = items.to_vec();
    items.sort_by_cached_key(key);
    items
}

/// Devices by name, then MAC.
pub fn devices(items: &[Arc<Device>]) -> Vec<Arc<Device>> {
    sorted(items, |device| {
        (
            name_key(device.name.as_deref()),
            device.mac.as_str().to_owned(),
        )
    })
}

/// Clients by the name the table shows (alias, else hostname), then IP,
/// then MAC. Unnamed clients follow the named ones.
pub fn clients(items: &[Arc<Client>]) -> Vec<Arc<Client>> {
    sorted(items, |client| {
        let shown = client.name.as_deref().or(client.hostname.as_deref());
        (
            name_key(shown),
            present_first::<IpAddr>(client.ip),
            client.mac.as_str().to_owned(),
        )
    })
}

/// Networks by name, then VLAN, then ID.
pub fn networks(items: &[Arc<Network>]) -> Vec<Arc<Network>> {
    sorted(items, |network| {
        (
            name_key(Some(&network.name)),
            present_first(network.vlan_id),
            network.id.to_string(),
        )
    })
}

/// WiFi broadcasts by SSID name, then ID.
pub fn wifi_broadcasts(items: &[Arc<WifiBroadcast>]) -> Vec<Arc<WifiBroadcast>> {
    sorted(items, |wifi| {
        (name_key(Some(&wifi.name)), wifi.id.to_string())
    })
}

/// Sites by display name, then internal name, then ID.
pub fn sites(items: &[Arc<Site>]) -> Vec<Arc<Site>> {
    sorted(items, |site| {
        (
            name_key(Some(&site.name)),
            site.internal_name.clone(),
            site.id.to_string(),
        )
    })
}

/// Firewall zones by name, then ID.
pub fn firewall_zones(items: &[Arc<FirewallZone>]) -> Vec<Arc<FirewallZone>> {
    sorted(items, |zone| {
        (name_key(Some(&zone.name)), zone.id.to_string())
    })
}

/// Firewall groups by name, then type, then ID.
pub fn firewall_groups(items: &[Arc<FirewallGroup>]) -> Vec<Arc<FirewallGroup>> {
    sorted(items, |group| {
        (
            name_key(Some(&group.name)),
            group.group_type.to_string(),
            group.id.to_string(),
        )
    })
}

/// DNS policies by domain, then record type, then value, then ID.
pub fn dns_policies(items: &[Arc<DnsPolicy>]) -> Vec<Arc<DnsPolicy>> {
    sorted(items, |policy| {
        (
            name_key(Some(&policy.domain)),
            policy.policy_type.to_string(),
            policy.value.clone(),
            policy.id.to_string(),
        )
    })
}

/// Traffic matching lists by name, then ID.
pub fn traffic_lists(items: &[Arc<TrafficMatchingList>]) -> Vec<Arc<TrafficMatchingList>> {
    sorted(items, |list| {
        (name_key(Some(&list.name)), list.id.to_string())
    })
}

/// Firewall policies in evaluation order.
///
/// A policy only ever matches traffic between its own source and
/// destination zones, and the controller numbers each zone pair separately:
/// user rules placed before the system rules from 10000, system rules from
/// 30000, and the pair's default verdict at `i32::MAX`. So policies group by
/// zone pair (source zone name, then destination zone name) and run by
/// ascending `index` inside each pair, with the ID as a final tie-breaker.
/// Zones missing from `zones` sort by ID after the named ones.
pub fn firewall_policies(
    policies: &[Arc<FirewallPolicy>],
    zones: &[Arc<FirewallZone>],
) -> Vec<Arc<FirewallPolicy>> {
    let zone_names: HashMap<&EntityId, &str> = zones
        .iter()
        .map(|zone| (&zone.id, zone.name.as_str()))
        .collect();
    let zone_key = |zone_id: Option<&EntityId>| {
        let name = zone_id.and_then(|id| zone_names.get(id).copied());
        (
            name_key(name),
            present_first(zone_id.map(ToString::to_string)),
        )
    };
    sorted(policies, |policy| {
        (
            zone_key(policy.source.zone_id.as_ref()),
            zone_key(policy.destination.zone_id.as_ref()),
            present_first(policy.index),
            policy.id.to_string(),
        )
    })
}

/// NAT rules by `rule_index`, then name, then ID.
pub fn nat_policies(items: &[Arc<NatPolicy>]) -> Vec<Arc<NatPolicy>> {
    sorted(items, |policy| {
        (
            present_first(policy.rule_index),
            name_key(Some(&policy.name)),
            policy.id.to_string(),
        )
    })
}

/// ACL rules in evaluation order (lower index first), then name, then ID.
pub fn acl_rules(items: &[Arc<AclRule>]) -> Vec<Arc<AclRule>> {
    sorted(items, |rule| {
        (
            present_first(rule.index),
            name_key(Some(&rule.name)),
            rule.id.to_string(),
        )
    })
}

/// Events newest first, so `--limit N` keeps the N most recent. Same-instant
/// events fall back to ID, type, message, and then the fields the store keys
/// an ID-less event by, so distinct records never compare equal.
pub fn events(items: &[Arc<Event>]) -> Vec<Arc<Event>> {
    sorted(items, |event| {
        (
            Reverse(event.timestamp),
            present_first(event.id.as_ref().map(ToString::to_string)),
            event.event_type.clone(),
            event.message.clone(),
            event.raw_key.clone(),
            event.device_mac.as_ref().map(|mac| mac.as_str().to_owned()),
            event.client_mac.as_ref().map(|mac| mac.as_str().to_owned()),
            event.site_id.as_ref().map(ToString::to_string),
        )
    })
}

/// Vouchers newest first, then code, then ID. Vouchers without a creation
/// time come last.
pub fn vouchers(items: &[Arc<Voucher>]) -> Vec<Arc<Voucher>> {
    sorted(items, |voucher| {
        (
            present_first(voucher.created_at.map(Reverse)),
            voucher.code.clone(),
            voucher.id.to_string(),
        )
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use unifly_api::model::{AclRule, Client, Device, FirewallPolicy, FirewallZone, NatPolicy};

    use super::{acl_rules, clients, devices, firewall_policies, name_key, nat_policies};
    use crate::cli::args::ListArgs;
    use crate::cli::commands::util::apply_list_args;

    const ZONE_INTERNAL: &str = "11111111-1111-4111-8111-111111111111";
    const ZONE_EXTERNAL: &str = "22222222-2222-4222-8222-222222222222";
    const ZONE_GATEWAY: &str = "33333333-3333-4333-8333-333333333333";

    fn from_json<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Arc<T> {
        Arc::new(serde_json::from_value(value).expect("fixture should deserialize"))
    }

    fn device(name: Option<&str>, mac: &str) -> Arc<Device> {
        from_json(json!({
            "id": mac,
            "mac": mac,
            "ip": null,
            "wan_ipv6": null,
            "name": name,
            "model": null,
            "device_type": "Switch",
            "state": "Online",
            "firmware_version": null,
            "firmware_updatable": false,
            "adopted_at": null,
            "provisioned_at": null,
            "last_seen": null,
            "serial": null,
            "supported": true,
            "ports": [],
            "radios": [],
            "uplink_device_id": null,
            "uplink_device_mac": null,
            "has_switching": true,
            "has_access_point": false,
            "stats": {},
            "client_count": null,
            "origin": null
        }))
    }

    fn zone(id: &str, name: &str) -> Arc<FirewallZone> {
        from_json(json!({
            "id": id,
            "name": name,
            "network_ids": [],
            "origin": null
        }))
    }

    fn policy(name: &str, src: &str, dst: &str, index: i32) -> Arc<FirewallPolicy> {
        from_json(json!({
            "id": format!("fwp:{name}"),
            "name": name,
            "description": null,
            "enabled": true,
            "index": index,
            "action": "Allow",
            "ip_version": "Both",
            "source": {"zone_id": src, "filter": null},
            "destination": {"zone_id": dst, "filter": null},
            "source_summary": null,
            "destination_summary": null,
            "protocol_summary": null,
            "schedule": null,
            "ipsec_mode": null,
            "connection_states": [],
            "logging_enabled": false,
            "origin": null
        }))
    }

    fn names<T>(items: &[Arc<T>], name: impl Fn(&T) -> String) -> Vec<String> {
        items.iter().map(|item| name(item)).collect()
    }

    fn list_args(offset: u32, limit: u32) -> ListArgs {
        ListArgs {
            limit,
            offset,
            all: false,
            filter: None,
        }
    }

    #[test]
    fn name_key_sorts_named_case_insensitively_before_unnamed() {
        let mut keys = vec![
            name_key(None),
            name_key(Some("beta")),
            name_key(Some("  ")),
            name_key(Some("Alpha")),
        ];
        keys.sort();
        assert_eq!(
            keys,
            vec![
                (false, "alpha".to_owned()),
                (false, "beta".to_owned()),
                (true, String::new()),
                (true, String::new()),
            ]
        );
    }

    #[test]
    fn devices_order_ignores_snapshot_order() {
        let fleet = vec![
            device(Some("switch-b"), "aa:00:00:00:00:02"),
            device(None, "aa:00:00:00:00:09"),
            device(Some("AP-Lobby"), "aa:00:00:00:00:05"),
            device(Some("switch-b"), "aa:00:00:00:00:01"),
            device(None, "aa:00:00:00:00:03"),
        ];
        let mut reversed = fleet.clone();
        reversed.reverse();
        let mut rotated = fleet.clone();
        rotated.rotate_left(2);

        let expected = vec![
            "aa:00:00:00:00:05",
            "aa:00:00:00:00:01",
            "aa:00:00:00:00:02",
            "aa:00:00:00:00:03",
            "aa:00:00:00:00:09",
        ];
        for input in [&fleet, &reversed, &rotated] {
            let macs = names(&devices(input), |d| d.mac.as_str().to_owned());
            assert_eq!(macs, expected);
        }
    }

    #[test]
    fn clients_fall_back_from_alias_to_hostname_to_ip_to_mac() {
        let client = |name: Option<&str>, hostname: Option<&str>, ip: Option<&str>, mac: &str| {
            from_json::<Client>(json!({
                "id": mac,
                "mac": mac,
                "ip": ip,
                "name": name,
                "hostname": hostname,
                "client_type": "Wired",
                "connected_at": null,
                "uplink_device_id": null,
                "uplink_device_mac": null,
                "network_id": null,
                "vlan": null,
                "wireless": null,
                "guest_auth": null,
                "is_guest": false,
                "tx_bytes": null,
                "rx_bytes": null,
                "bandwidth": null,
                "os_name": null,
                "device_class": null,
                "use_fixedip": false,
                "fixed_ip": null,
                "blocked": false
            }))
        };
        let input = vec![
            client(None, None, None, "cc:00:00:00:00:01"),
            client(None, None, Some("10.0.0.20"), "cc:00:00:00:00:02"),
            client(None, Some("printer"), None, "cc:00:00:00:00:03"),
            client(Some("Laptop"), Some("zz-host"), None, "cc:00:00:00:00:04"),
            client(None, None, Some("10.0.0.3"), "cc:00:00:00:00:05"),
        ];
        let macs = names(&clients(&input), |c| c.mac.as_str().to_owned());
        assert_eq!(
            macs,
            vec![
                "cc:00:00:00:00:04",
                "cc:00:00:00:00:03",
                "cc:00:00:00:00:05",
                "cc:00:00:00:00:02",
                "cc:00:00:00:00:01",
            ]
        );
    }

    #[test]
    fn firewall_policies_follow_evaluation_order_per_zone_pair() {
        let zones = vec![
            zone(ZONE_INTERNAL, "Internal"),
            zone(ZONE_EXTERNAL, "External"),
            zone(ZONE_GATEWAY, "Gateway"),
        ];
        let input = vec![
            policy("int-ext default", ZONE_INTERNAL, ZONE_EXTERNAL, i32::MAX),
            policy("ext-gw return", ZONE_EXTERNAL, ZONE_GATEWAY, 30_000),
            policy("int-ext user 2", ZONE_INTERNAL, ZONE_EXTERNAL, 10_001),
            policy("ext-int user", ZONE_EXTERNAL, ZONE_INTERNAL, 10_000),
            policy("int-ext system", ZONE_INTERNAL, ZONE_EXTERNAL, 30_000),
            policy("ext-gw user", ZONE_EXTERNAL, ZONE_GATEWAY, 10_000),
            policy("int-ext user 1", ZONE_INTERNAL, ZONE_EXTERNAL, 10_000),
        ];
        let expected = vec![
            "ext-gw user",
            "ext-gw return",
            "ext-int user",
            "int-ext user 1",
            "int-ext user 2",
            "int-ext system",
            "int-ext default",
        ];

        let mut reversed = input.clone();
        reversed.reverse();
        for snapshot in [&input, &reversed] {
            let ordered = firewall_policies(snapshot, &zones);
            assert_eq!(names(&ordered, |p| p.name.clone()), expected);
        }
    }

    #[test]
    fn firewall_policies_with_unknown_zones_sort_after_named_zones() {
        let zones = vec![zone(ZONE_INTERNAL, "Internal")];
        let input = vec![
            policy("unknown pair", ZONE_GATEWAY, ZONE_GATEWAY, 10_000),
            policy("known pair", ZONE_INTERNAL, ZONE_INTERNAL, 20_000),
        ];
        let ordered = firewall_policies(&input, &zones);
        assert_eq!(
            names(&ordered, |p| p.name.clone()),
            vec!["known pair", "unknown pair"]
        );
    }

    #[test]
    fn acl_and_nat_rules_follow_their_index() {
        let acl = |name: &str, index: Option<i32>| {
            from_json::<AclRule>(json!({
                "id": name,
                "name": name,
                "enabled": true,
                "index": index,
                "rule_type": "Ipv4",
                "action": "Block",
                "source_summary": null,
                "destination_summary": null,
                "origin": null
            }))
        };
        let rules = vec![acl("c", Some(2)), acl("unindexed", None), acl("a", Some(0))];
        assert_eq!(
            names(&acl_rules(&rules), |r| r.name.clone()),
            vec!["a", "c", "unindexed"]
        );

        let nat = |name: &str, rule_index: Option<u64>| {
            from_json::<NatPolicy>(json!({
                "id": name,
                "name": name,
                "description": null,
                "enabled": true,
                "nat_type": "Destination",
                "interface_id": null,
                "protocol": null,
                "src_address": null,
                "src_port": null,
                "dst_address": null,
                "dst_port": null,
                "translated_address": null,
                "translated_port": null,
                "rule_index": rule_index,
                "origin": null
            }))
        };
        let rules = vec![
            nat("second", Some(2)),
            nat("loose", None),
            nat("first", Some(1)),
        ];
        assert_eq!(
            names(&nat_policies(&rules), |r| r.name.clone()),
            vec!["first", "second", "loose"]
        );
    }

    #[test]
    fn sorted_pages_are_stable_and_disjoint_across_snapshot_orders() {
        let fleet: Vec<Arc<Device>> = (0..7)
            .map(|i| device(Some(&format!("dev-{i}")), &format!("aa:00:00:00:00:0{i}")))
            .collect();
        let mut shuffled = fleet.clone();
        shuffled.swap(0, 5);
        shuffled.swap(2, 6);
        shuffled.reverse();

        let page = |snapshot: &[Arc<Device>], offset: u32| {
            let rows = apply_list_args(devices(snapshot), &list_args(offset, 3), |_, _| true);
            names(&rows, |d| d.name.clone().unwrap_or_default())
        };

        for offset in [0, 3, 6] {
            assert_eq!(page(&fleet, offset), page(&shuffled, offset));
        }

        let mut seen: Vec<String> = [0, 3, 6]
            .into_iter()
            .flat_map(|offset| page(&shuffled, offset))
            .collect();
        let total = seen.len();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), total, "pages overlap");
        assert_eq!(total, fleet.len(), "pages skip rows");
    }
}
