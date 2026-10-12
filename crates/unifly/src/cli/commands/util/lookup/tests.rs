use std::sync::Arc;

use serde_json::json;
use unifly_api::{Device, FirewallGroup, MacAddress, Network};

use super::{Shape, lookup, lookup_mac, resolve};
use crate::cli::commands::util::fixtures::{client, device, from_json};
use crate::cli::error::CliError;

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
fn shape_alone_decides_how_an_identifier_reads() {
    assert_eq!(
        Shape::of(" 10.40.0.7 "),
        Shape::Ip("10.40.0.7".parse().expect("ip"))
    );
    // Twelve hex digits once the colons go, but an IPv6 address first.
    assert!(matches!(Shape::of("::3624:909d:f310"), Shape::Ip(_)));
    assert_eq!(
        Shape::of("AABB.CCDD.EEFF"),
        Shape::Mac(MacAddress::new("aa:bb:cc:dd:ee:ff"))
    );
    assert_eq!(Shape::of("ap-roof"), Shape::Name);
}

/// A device named after some other MAC must never capture a command aimed
/// at that MAC: the get and ID-based mutations (remove, restart,
/// port-cycle) report not found, and MAC-based mutations (upgrade,
/// provision, locate, port-set) pass the MAC through untouched.
#[test]
fn a_device_named_like_a_mac_never_captures_that_mac() {
    let fleet = vec![device(
        "11111111-1111-4111-8111-111111111111",
        Some("aa:bb:cc:dd:ee:ff"),
        "02:00:00:00:00:01",
        Some("10.40.0.5"),
    )];
    for needle in ["aa:bb:cc:dd:ee:ff", "AA-BB-CC-DD-EE-FF", "aabb.ccdd.eeff"] {
        assert!(
            matches!(resolve(&fleet, needle), Err(CliError::NotFound { .. })),
            "get {needle} must not resolve the MAC-named device"
        );
        match lookup_mac(&fleet, needle) {
            Ok(mac) => assert_eq!(mac.as_str(), "aa:bb:cc:dd:ee:ff", "{needle}"),
            Err(error) => panic!("{needle} should pass through, got {error:?}"),
        }
    }
}

/// The same for IP addresses, which have no pass-through: every path is
/// not found rather than the IP-named device.
#[test]
fn a_device_named_like_an_ip_never_captures_that_ip() {
    let fleet = vec![
        device(
            "11111111-1111-4111-8111-111111111111",
            Some("10.40.0.99"),
            "02:00:00:00:00:01",
            Some("10.40.0.5"),
        ),
        device(
            "22222222-2222-4222-8222-222222222222",
            Some("bench"),
            "02:00:00:00:00:02",
            Some("10.40.0.7"),
        ),
    ];
    assert!(matches!(
        resolve(&fleet, "10.40.0.99"),
        Err(CliError::NotFound { .. })
    ));
    assert!(matches!(
        lookup_mac(&fleet, "10.40.0.99"),
        Err(CliError::NotFound { .. })
    ));
    // An IP that a device does hold still resolves it.
    assert_eq!(
        name_of(resolve(&fleet, "10.40.0.7").expect("by IP")),
        "bench"
    );
    assert_eq!(
        lookup_mac(&fleet, "10.40.0.7").expect("by IP").as_str(),
        "02:00:00:00:00:02"
    );
}

#[test]
fn ip_addresses_never_match_or_pass_through_as_macs() {
    let fleet = fleet();
    // Twelve hex digits once the colons go: the MAC of ap-roof.
    assert!(matches!(lookup(&fleet, "::3624:909d:f310"), Ok(None)));
    assert!(matches!(
        lookup_mac(&fleet, "::aabb:ccdd:eeff"),
        Err(CliError::NotFound { .. })
    ));
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
    let clients = vec![client(
        "Crew Laptop",
        "crew-laptop-01",
        "76:ce:f4:ac:26:ee",
        Some("10.40.0.131"),
    )];
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
