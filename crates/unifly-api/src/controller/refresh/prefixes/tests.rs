use crate::model::Network;
use crate::session::models::SessionDevice;

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

    super::merge(&mut networks, &[gateway]);

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
