use std::collections::HashMap;

use serde_json::{Value, json};

use super::{apply_ipv4_update, build_create_network_payload};
use crate::command::{CreateNetworkRequest, DhcpUpdate, UpdateNetworkRequest};

fn request(management: &str) -> CreateNetworkRequest {
    serde_json::from_value(json!({
        "name": "Network", "enabled": true, "vlan_id": 30, "management": management,
        "device_id": if management == "Switch" { json!("00000000-0000-0000-0000-000000000001") } else { Value::Null },
        "subnet": "10.0.30.1/24", "dhcp_enabled": true,
        "dhcp_range_start": "10.0.30.100", "dhcp_range_stop": "10.0.30.200",
        "isolation_enabled": false, "internet_access_enabled": true
    })).expect("request")
}

fn ipv4(dhcp: &Value) -> HashMap<String, Value> {
    HashMap::from([(
        "ipv4Configuration".into(),
        json!({
            "hostIpAddress": "10.0.30.1", "prefixLength": 24, "autoScaleEnabled": false,
            "dhcpConfiguration": dhcp,
        }),
    )])
}

#[test]
fn create_enable_and_partial_updates_share_range_validation() {
    for management in ["Gateway", "Switch"] {
        for (start, stop) in [
            ("not-an-ip", "10.0.30.200"),
            ("10.0.30.100", "invalid"),
            ("::1", "10.0.30.200"),
            ("10.0.30.100", "2001:db8::1"),
            ("10.0.30.201", "10.0.30.200"),
        ] {
            let mut create = request(management);
            create.dhcp_range_start = Some(start.into());
            create.dhcp_range_stop = Some(stop.into());
            let create_error = build_create_network_payload(create).expect_err("invalid create");
            let update = UpdateNetworkRequest {
                dhcp_enabled: Some(true),
                dhcp: Some(DhcpUpdate {
                    range_start: Some(start.into()),
                    range_stop: Some(stop.into()),
                    dns_servers: None,
                }),
                ..Default::default()
            };
            let update_error = apply_ipv4_update(
                &mut ipv4(&Value::Null),
                &management.to_uppercase(),
                false,
                &update,
            )
            .expect_err("invalid enable");
            assert_eq!(create_error.to_string(), update_error.to_string());
            // A retained range must be validated even when enabling supplies no new bounds.
            let mut retained =
                ipv4(&json!({"mode": "RELAY", "ipAddressRange": {"start": start, "stop": stop}}));
            let toggle = UpdateNetworkRequest {
                dhcp_enabled: Some(true),
                ..Default::default()
            };
            assert!(
                apply_ipv4_update(&mut retained, &management.to_uppercase(), false, &toggle)
                    .is_err()
            );
            let mut serving = ipv4(
                &json!({"mode": "SERVER", "ipAddressRange": {"start": start, "stop": "10.0.30.200"}}),
            );
            let partial = UpdateNetworkRequest {
                dhcp: Some(DhcpUpdate {
                    range_stop: Some(stop.into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            assert!(
                apply_ipv4_update(&mut serving, &management.to_uppercase(), true, &partial)
                    .is_err()
            );
        }
    }
}

#[test]
fn managed_network_creation_and_enabling_emit_the_same_server_contract() {
    for management in ["Gateway", "Switch"] {
        let mut create = request(management);
        create.dhcp_lease_time = Some(3600);
        create.dns_servers = Some(vec!["192.0.2.53".into()]);
        let payload = build_create_network_payload(create).expect("managed create");
        let wire = serde_json::to_value(payload).expect("wire");
        let dhcp = &wire["ipv4Configuration"]["dhcpConfiguration"];
        assert_eq!(dhcp["mode"], "SERVER");
        assert_eq!(dhcp["leaseTimeSeconds"], 3600);
        assert_eq!(dhcp["dnsServerIpAddressesOverride"], json!(["192.0.2.53"]));
        assert_eq!(
            dhcp.get("pingConflictDetectionEnabled"),
            (management == "Gateway").then_some(&Value::Bool(false))
        );
        let spec: Value =
            serde_json::from_str(include_str!("../../../../openapi/11.0.97.json")).expect("spec");
        let schema = format!("Integration{management}ManagedNetworkCreateUpdateDto");
        for required in spec["components"]["schemas"][schema]["required"]
            .as_array()
            .expect("required fields")
        {
            assert!(
                wire.get(required.as_str().expect("name")).is_some(),
                "missing {required}"
            );
        }
        if management == "Switch" {
            assert_eq!(wire["deviceId"], "00000000-0000-0000-0000-000000000001");
            assert!(wire.get("internetAccessEnabled").is_none());
            assert!(wire.get("mdnsForwardingEnabled").is_none());
        }
        let mut extra = ipv4(&Value::Null);
        let update = UpdateNetworkRequest {
            dhcp_enabled: Some(true),
            dhcp: Some(DhcpUpdate {
                range_start: Some("10.0.30.100".into()),
                range_stop: Some("10.0.30.200".into()),
                dns_servers: None,
            }),
            ..Default::default()
        };
        apply_ipv4_update(&mut extra, &management.to_uppercase(), false, &update).expect("enable");
        let enabled = &extra["ipv4Configuration"]["dhcpConfiguration"];
        assert_eq!(enabled["ipAddressRange"], dhcp["ipAddressRange"]);
        assert_eq!(
            enabled.get("pingConflictDetectionEnabled"),
            dhcp.get("pingConflictDetectionEnabled")
        );
    }
}

#[test]
fn switch_creation_requires_owner_and_managed_networks_require_ipv4() {
    for owner in [
        None,
        Some(crate::model::EntityId::Legacy("old-switch-id".into())),
    ] {
        let mut create = request("Switch");
        create.device_id = owner;
        assert!(
            build_create_network_payload(create)
                .expect_err("owner required")
                .to_string()
                .contains("Integration UUID")
        );
    }
    for management in ["Gateway", "Switch"] {
        for enabled in [false, true] {
            let mut create = request(management);
            create.subnet = None;
            create.dhcp_enabled = enabled;
            if !enabled {
                create.dhcp_range_start = None;
                create.dhcp_range_stop = None;
            }
            assert!(
                build_create_network_payload(create)
                    .expect_err("IPv4 required")
                    .to_string()
                    .contains("IPv4 subnet")
            );
        }
    }
}

#[test]
fn unmanaged_networks_reject_requested_ipv4_or_dhcp_but_plain_vlan_works() {
    for dhcp in [false, true] {
        let mut create = request("Unmanaged");
        create.dhcp_enabled = dhcp;
        if !dhcp {
            create.dhcp_range_start = None;
            create.dhcp_range_stop = None;
        }
        assert!(build_create_network_payload(create).is_err());
    }
    let mut create = request("Unmanaged");
    create.subnet = None;
    assert!(build_create_network_payload(create.clone()).is_err());
    create.dhcp_enabled = false;
    create.dhcp_range_start = None;
    create.dhcp_range_stop = None;
    let payload = build_create_network_payload(create).expect("plain VLAN");
    assert!(payload.extra.is_empty());
    let update = UpdateNetworkRequest {
        dhcp_enabled: Some(true),
        ..Default::default()
    };
    assert!(apply_ipv4_update(&mut ipv4(&Value::Null), "UNMANAGED", false, &update).is_err());
}
