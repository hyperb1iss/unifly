//! Every pinned firewall discriminator must have a lossless typed fixture.

use std::collections::BTreeSet;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use unifly_api::integration_types::{
    DestTrafficFilter, DomainFilter, IpAddressFilter, IpAddressItem, PortFilter, PortItem,
    SourceTrafficFilter,
};

const ID: &str = "00000000-0000-0000-0000-000000000010";

fn assert_contract<T: DeserializeOwned + Serialize>(schema: &str, fixtures: Vec<Value>) {
    let spec: Value = serde_json::from_str(include_str!("../openapi/11.0.97.json")).expect("spec");
    let declared: BTreeSet<_> = spec["components"]["schemas"][schema]["discriminator"]["mapping"]
        .as_object()
        .expect("discriminator mapping")
        .keys()
        .map(String::as_str)
        .collect();
    let covered: BTreeSet<_> = fixtures
        .iter()
        .map(|fixture| fixture["type"].as_str().expect("fixture type"))
        .collect();
    assert_eq!(
        covered, declared,
        "{schema}: every canonical tag needs a fixture"
    );
    for fixture in fixtures {
        let decoded: T = serde_json::from_value(fixture.clone()).expect("decode canonical fixture");
        assert_eq!(
            serde_json::to_value(decoded).expect("encode fixture"),
            fixture
        );
    }
}

fn port() -> Value {
    json!({"type": "PORTS", "items": [{"type": "PORT_NUMBER", "value": 443}], "matchOpposite": true})
}

fn common_filters() -> Vec<Value> {
    vec![
        json!({"type": "NETWORK", "networkFilter": {"networkIds": [ID], "matchOpposite": true}}),
        json!({"type": "IP_ADDRESS", "ipAddressFilter": {"type": "IP_ADDRESSES", "items": [{"type": "IP_ADDRESS_RANGE", "start": "192.0.2.1", "stop": "192.0.2.9"}], "matchOpposite": true}}),
        json!({"type": "PORT"}),
        json!({"type": "REGION", "regionFilter": {"regions": ["US"]}}),
        json!({"type": "VPN_SERVER", "vpnServerFilter": {"vpnServerIds": [ID], "matchOpposite": true}}),
        json!({"type": "SITE_TO_SITE_VPN_TUNNEL", "siteToSiteVpnTunnelFilter": {"siteToSiteVpnTunnelId": ID}}),
        json!({"type": "IPV6_IID", "ipv6IidFilter": {"ipv6Iid": "2001:db8::1/::ffff:ffff:ffff:ffff", "matchOpposite": true}}),
    ]
}

#[test]
fn all_source_discriminators_preserve_complete_filters() {
    let mut fixtures = common_filters();
    for filter in &mut fixtures {
        if matches!(
            filter["type"].as_str(),
            Some("NETWORK" | "IP_ADDRESS" | "IPV6_IID")
        ) {
            filter["macAddressFilter"] = json!("02:11:22:33:44:55");
        }
    }
    fixtures.push(json!({"type": "MAC_ADDRESS", "macAddressFilter": {"macAddresses": ["02:11:22:33:44:55", "02:11:22:33:44:66"]}}));
    for filter in &mut fixtures {
        filter["portFilter"] = port();
    }
    assert_contract::<SourceTrafficFilter>("Firewall policy source traffic filter", fixtures);
}

#[test]
fn all_destination_discriminators_preserve_complete_filters() {
    let mut fixtures = common_filters();
    fixtures.extend([
        json!({"type": "APPLICATION", "applicationFilter": {"applicationIds": [1, 2]}}),
        json!({"type": "APPLICATION_CATEGORY", "applicationCategoryFilter": {"applicationCategoryIds": [1, 2]}}),
        json!({"type": "DOMAIN", "domainFilter": {"type": "DOMAINS", "domains": ["example.com"]}}),
    ]);
    for filter in &mut fixtures {
        filter["portFilter"] = port();
    }
    assert_contract::<DestTrafficFilter>("Firewall policy destination traffic filter", fixtures);
}

#[test]
fn all_nested_discriminators_preserve_complete_filters() {
    assert_contract::<IpAddressFilter>(
        "Firewall policy IP address filter",
        vec![
            json!({"type": "IP_ADDRESSES", "items": [{"type": "IP_ADDRESS", "value": "192.0.2.1"}], "matchOpposite": true}),
            json!({"type": "TRAFFIC_MATCHING_LIST", "trafficMatchingListId": ID, "matchOpposite": true}),
        ],
    );
    assert_contract::<IpAddressItem>(
        "IP matching",
        vec![
            json!({"type": "IP_ADDRESS", "value": "192.0.2.1"}),
            json!({"type": "IP_ADDRESS_RANGE", "start": "192.0.2.1", "stop": "192.0.2.9"}),
            json!({"type": "SUBNET", "value": "192.0.2.0/24"}),
        ],
    );
    assert_contract::<PortFilter>(
        "Firewall policy port filter",
        vec![
            port(),
            json!({"type": "TRAFFIC_MATCHING_LIST", "trafficMatchingListId": ID, "matchOpposite": true}),
        ],
    );
    assert_contract::<PortItem>(
        "Port matching",
        vec![
            json!({"type": "PORT_NUMBER", "value": 443}),
            json!({"type": "PORT_NUMBER_RANGE", "start": 8000, "stop": 9000}),
        ],
    );
    assert_contract::<DomainFilter>(
        "Firewall policy domain filter",
        vec![json!({"type": "DOMAINS", "domains": ["example.com"]})],
    );
}
