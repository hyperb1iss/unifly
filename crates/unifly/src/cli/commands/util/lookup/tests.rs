use std::sync::Arc;

use serde_json::json;
use unifly_api::{Client, Device, FirewallGroup, Network};

use super::{lookup, resolve};
use crate::cli::error::CliError;

fn from_json<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Arc<T> {
    Arc::new(serde_json::from_value(value).expect("fixture should deserialize"))
}

fn device(id: &str, name: Option<&str>, mac: &str, ip: Option<&str>) -> Arc<Device> {
    from_json(json!({
        "id": id,
        "mac": mac,
        "ip": ip,
        "wan_ipv6": null,
        "name": name,
        "model": null,
        "device_type": "AccessPoint",
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
        "has_switching": false,
        "has_access_point": true,
        "stats": {},
        "client_count": null,
        "origin": null
    }))
}

const ROOF_ID: &str = "7369b778-1354-4760-83b8-2d36a5d49050";

fn fleet() -> Vec<Arc<Device>> {
    vec![
        device(
            ROOF_ID,
            Some("ap-roof"),
            "36:24:90:9d:f3:10",
            Some("10.40.0.18"),
        ),
        device(
            "aa20f96c-c798-482a-9612-821927355a99",
            Some("ap-dock"),
            "e2:be:2f:cd:b9:65",
            Some("10.40.0.19"),
        ),
        device(
            "1a0703fe-da7e-4a36-9e6e-7d5d3e1d045b",
            None,
            "b6:16:7f:be:df:a5",
            None,
        ),
    ]
}

fn name_of(found: &Arc<Device>) -> &str {
    found.name.as_deref().unwrap_or("(unnamed)")
}

#[test]
fn resolves_each_identifier_kind() {
    let fleet = fleet();
    for needle in [
        ROOF_ID,
        "7369B778-1354-4760-83B8-2D36A5D49050",
        "ap-roof",
        "AP-Roof",
        "  ap-roof ",
        "10.40.0.18",
        "36:24:90:9d:f3:10",
    ] {
        let found = resolve(&fleet, needle).unwrap_or_else(|error| panic!("{needle}: {error}"));
        assert_eq!(name_of(found), "ap-roof", "{needle}");
    }
}

#[test]
fn normalizes_every_common_mac_spelling() {
    let fleet = fleet();
    for needle in [
        "36:24:90:9D:F3:10",
        "36-24-90-9D-F3-10",
        "3624.909d.f310",
        "3624909DF310",
    ] {
        match lookup(&fleet, needle) {
            Ok(Some(found)) => assert_eq!(name_of(found), "ap-roof", "{needle}"),
            other => panic!("{needle} should resolve, got {other:?}"),
        }
    }
}

#[test]
fn unnamed_records_still_resolve_by_mac() {
    let fleet = fleet();
    let found = resolve(&fleet, "B6-16-7F-BE-DF-A5").expect("gateway by MAC");
    assert!(found.name.is_none());
}

#[test]
fn duplicate_names_are_ambiguous_and_list_every_candidate() {
    let mut fleet = fleet();
    fleet.push(device(
        "0b8e1a6e-5c55-4d3c-9d1d-2f3b6f1e0c11",
        Some("AP-ROOF"),
        "02:00:00:00:00:99",
        Some("10.40.0.99"),
    ));
    match resolve(&fleet, "ap-roof") {
        Err(CliError::Ambiguous {
            resource_type,
            count,
            candidates,
            list_command,
            ..
        }) => {
            assert_eq!(resource_type, "device");
            assert_eq!(count, 2);
            assert_eq!(list_command, "devices list");
            assert!(candidates.contains(ROOF_ID), "{candidates}");
            assert!(candidates.contains("02:00:00:00:00:99"), "{candidates}");
            assert!(candidates.contains("ip 10.40.0.99"), "{candidates}");
        }
        other => panic!("expected an ambiguity error, got {other:?}"),
    }
    // A stronger identifier still picks one of them.
    let found = resolve(&fleet, "10.40.0.99").expect("by IP");
    assert_eq!(name_of(found), "AP-ROOF");
}

#[test]
fn stronger_tiers_win_and_empty_tiers_fall_through() {
    let fleet = vec![
        device(
            "11111111-1111-4111-8111-111111111111",
            Some("10.40.0.7"),
            "02:00:00:00:00:01",
            None,
        ),
        device(
            "22222222-2222-4222-8222-222222222222",
            Some("bench"),
            "02:00:00:00:00:02",
            Some("10.40.0.7"),
        ),
        device(
            "33333333-3333-4333-8333-333333333333",
            Some("020000000009"),
            "02:00:00:00:00:03",
            None,
        ),
    ];
    // The IP tier answers before a name that happens to look like an IP.
    assert_eq!(
        name_of(resolve(&fleet, "10.40.0.7").expect("by IP")),
        "bench"
    );
    // A MAC-shaped needle that matches no MAC falls through to names.
    assert_eq!(
        name_of(resolve(&fleet, "020000000009").expect("by name")),
        "020000000009"
    );
}

#[test]
fn misses_point_at_the_list_command() {
    let fleet = fleet();
    assert!(matches!(lookup(&fleet, ""), Ok(None)));
    assert!(matches!(lookup(&fleet, "ap-ro"), Ok(None)));
    match resolve(&fleet, "ap-attic") {
        Err(CliError::NotFound {
            resource_type,
            identifier,
            list_command,
        }) => {
            assert_eq!(resource_type, "device");
            assert_eq!(identifier, "ap-attic");
            assert_eq!(list_command, "devices list");
        }
        other => panic!("expected not found, got {other:?}"),
    }
}

#[test]
fn clients_answer_to_alias_or_hostname() {
    let client: Arc<Client> = from_json(json!({
        "id": "9d5cf91f-0513-4c50-b79c-b3350a023b95",
        "mac": "76:ce:f4:ac:26:ee",
        "ip": "10.40.0.131",
        "name": "Crew Laptop",
        "hostname": "crew-laptop-01",
        "client_type": "Wireless",
        "connected_at": null,
        "uplink_device_id": null,
        "uplink_device_mac": null,
        "switch_port": null,
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
    }));
    let clients = vec![client];
    for needle in [
        "crew laptop",
        "CREW-LAPTOP-01",
        "76CE.F4AC.26EE",
        "10.40.0.131",
    ] {
        assert!(
            matches!(lookup(&clients, needle), Ok(Some(_))),
            "{needle} should resolve"
        );
    }
}

#[test]
fn named_records_resolve_by_name_and_groups_by_external_id() {
    let networks: Vec<Arc<Network>> = vec![from_json(json!({
        "id": "b87df54f-371c-4499-b803-4c1d5c186a7f",
        "name": "Studio",
        "enabled": true,
        "management": null,
        "purpose": null,
        "is_default": false,
        "vlan_id": 10,
        "subnet": null,
        "gateway_ip": null,
        "dhcp": null,
        "ipv6_enabled": false,
        "ipv6_mode": null,
        "ipv6_prefix": null,
        "dhcpv6_enabled": false,
        "slaac_enabled": false,
        "ntp_server": null,
        "pxe_enabled": false,
        "tftp_server": null,
        "firewall_zone_id": null,
        "isolation_enabled": false,
        "internet_access_enabled": true,
        "mdns_forwarding_enabled": false,
        "cellular_backup_enabled": false,
        "origin": null
    }))];
    assert_eq!(
        resolve(&networks, "studio").expect("by name").name,
        "Studio"
    );

    let groups: Vec<Arc<FirewallGroup>> = vec![from_json(json!({
        "id": "65f0c0ffee0000000000abcd",
        "external_id": "5d2c4b8e-0b5f-4f8e-9a51-7b0c3f9e6a10",
        "name": "Render Ports",
        "group_type": "PortGroup",
        "group_members": ["8080"]
    }))];
    for needle in [
        "65f0c0ffee0000000000abcd",
        "5D2C4B8E-0B5F-4F8E-9A51-7B0C3F9E6A10",
        "render ports",
    ] {
        assert!(
            matches!(lookup(&groups, needle), Ok(Some(_))),
            "{needle} should resolve"
        );
    }
}
