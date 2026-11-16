use crate::integration_types::ClientResponse;
use crate::model::{Client, Network};
use crate::session::models::{SessionClientEntry, SessionDevice};

#[test]
fn hybrid_clients_take_the_session_network_name() {
    let mut clients: Vec<Client> = vec![
        ClientResponse {
            id: uuid::Uuid::from_u128(1),
            name: "render-07".into(),
            client_type: "WIRED".into(),
            ip_address: Some("10.40.20.17".into()),
            connected_at: None,
            mac_address: Some("3e:bf:ec:41:e9:0c".into()),
            access: serde_json::json!({ "type": "DEFAULT" }),
        }
        .into(),
    ];
    let session: SessionClientEntry = serde_json::from_value(serde_json::json!({
        "_id": "abc",
        "mac": "3e:bf:ec:41:e9:0c",
        "ip": "10.40.20.17",
        "network": "Render",
        "network_id": "5f1a2b3c4d5e6f7a8b9c0d1e"
    }))
    .expect("deserialize");

    super::merge_session_clients(&mut clients, &[session], &[]);

    assert_eq!(clients[0].network_name.as_deref(), Some("Render"));
}

#[test]
fn hybrid_clients_take_the_session_ipv6_addresses() {
    let mut clients: Vec<Client> = vec![
        ClientResponse {
            id: uuid::Uuid::from_u128(1),
            name: "selene".into(),
            client_type: "WIRELESS".into(),
            ip_address: Some("10.4.20.198".into()),
            connected_at: None,
            mac_address: Some("3e:bf:ec:41:e9:0d".into()),
            access: serde_json::json!({ "type": "DEFAULT" }),
        }
        .into(),
    ];
    let session: SessionClientEntry = serde_json::from_value(serde_json::json!({
        "_id": "abc",
        "mac": "3e:bf:ec:41:e9:0d",
        "ip": "10.4.20.198",
        "last_ipv6": ["2001:db8:3fa::9"],
    }))
    .expect("deserialize");

    super::merge_session_clients(&mut clients, &[session], &[]);

    assert!(clients[0].has_address("2001:db8:3fa::9".parse().expect("address")));
}

fn network(name: &str, ipv6: &serde_json::Value) -> Network {
    let details: crate::integration_types::NetworkDetailsResponse =
        serde_json::from_value(serde_json::json!({
            "id": uuid::Uuid::from_u128(name.len().try_into().unwrap_or_default()), "name": name,
            "enabled": true, "management": "GATEWAY", "vlanId": 10,
            "metadata": {"origin": "USER_DEFINED"}, "ipv6Configuration": ipv6,
        }))
        .expect("network");
    Network::from(details)
}

#[test]
fn a_delegated_network_takes_the_global_subnet_the_gateway_serves_on_it() {
    let delegated = serde_json::json!({
        "interfaceType": "PREFIX_DELEGATION",
        "clientAddressAssignment": {"slaacEnabled": true},
    });
    let fixed = serde_json::json!({
        "interfaceType": "STATIC", "hostIpAddress": "2001:db8:99::1", "prefixLength": 64,
        "clientAddressAssignment": {"slaacEnabled": true},
    });
    let mut networks = vec![
        network("Home", &delegated),
        network("Lab", &fixed),
        network("Off", &serde_json::Value::Null),
    ];
    let gateway: SessionDevice = serde_json::from_value(serde_json::json!({
        "mac": "6c:63:f8:38:d9:01", "type": "udm", "state": 1,
        "network_table": [
            {"_id": "65c3d8", "name": "Home",
             "ipv6_subnets": ["fd00:c0de:15::1/64", "2001:db8:3fa::1/64"]},
            {"_id": "65c3d9", "name": "Lab", "ipv6_subnets": ["2001:db8:77::1/64"]},
            {"_id": "65c3da", "name": "Off", "ipv6_subnets": ["2001:db8:78::1/64"]},
            "not a row",
        ],
    }))
    .expect("deserialize");

    super::merge_gateway_prefixes(&mut networks, &[gateway]);

    assert_eq!(
        networks[0].ipv6_prefix.as_deref(),
        Some("2001:db8:3fa::1/64")
    );
    // A static network keeps the subnet it names; one without IPv6 has none.
    assert_eq!(
        networks[1].ipv6_prefix.as_deref(),
        Some("2001:db8:99::1/64")
    );
    assert_eq!(networks[2].ipv6_prefix, None);
}
