//! What a network update or delete sends, and what it refuses to send.

use super::*;

const NETWORK: &str = "8a1e5f3c-2b4d-4e6f-9a0b-1c2d3e4f5a11";
/// The WAN the controller lists first, its primary.
const PRIMARY_WAN: &str = "5b1e5f3c-2b4d-4e6f-9a0b-1c2d3e4f5a55";

/// A gateway-managed network that uses the optional parts of the schema.
fn gateway_network() -> serde_json::Value {
    json!({
        "id": NETWORK, "name": "Lab", "enabled": true, "management": "GATEWAY",
        "vlanId": 30, "default": false,
        "metadata": {"origin": "USER_DEFINED"},
        "isolationEnabled": false, "cellularBackupEnabled": true,
        "internetAccessEnabled": true, "mdnsForwardingEnabled": true,
        "zoneId": "6a1e5f3c-2b4d-4e6f-9a0b-1c2d3e4f5a99",
        "dhcpGuarding": {"trustedDhcpServerIpAddresses": ["10.0.30.1"]},
        "ipv4Configuration": {
            "autoScaleEnabled": false, "hostIpAddress": "10.0.30.1", "prefixLength": 24,
            "additionalHostIpSubnets": ["10.0.31.1/24"],
            "dhcpConfiguration": {
                "mode": "SERVER",
                "ipAddressRange": {"start": "10.0.30.100", "stop": "10.0.30.254"},
                "leaseTimeSeconds": 3600,
                "dnsServerIpAddressesOverride": ["10.0.30.2", "10.0.30.3"],
                "domainName": "lab.internal",
                "ntpServerIpAddresses": ["10.0.30.4"],
                "option43Value": "01:04:0a:00:1e:05",
                "pingConflictDetectionEnabled": true,
                "tftpServerAddress": "10.0.30.6",
                "winsServerIpAddresses": ["10.0.30.7"],
            },
        },
        "ipv6Configuration": {
            "interfaceType": "PREFIX_DELEGATION",
            "prefixDelegationWanInterfaceId": "7a1e5f3c-2b4d-4e6f-9a0b-1c2d3e4f5a77",
            "clientAddressAssignment": {"dhcpv6Enabled": false, "slaacEnabled": true},
            "routerAdvertisement": {"priority": "HIGH"},
            "dnsServerIpAddressesOverride": ["2001:db8::53"],
        },
    })
}

/// What happened to one network write: the outcome, and every write
/// request the controller received for the network.
struct Written {
    result: Result<unifly_api::CommandResult, CoreError>,
    requests: Vec<Request>,
}

impl Written {
    /// The body of the one PUT the update sent.
    fn put_body(&self) -> serde_json::Value {
        assert!(self.result.is_ok(), "{:?}", self.result);
        let [put] = self.requests.as_slice() else {
            panic!("expected one write, got {}", self.requests.len());
        };
        assert_eq!(put.method.as_str(), "PUT");
        serde_json::from_slice(&put.body).unwrap()
    }

    /// The validation message of an update that sent nothing.
    fn refusal(&self) -> String {
        assert!(self.requests.is_empty(), "a refused update writes nothing");
        match &self.result {
            Err(CoreError::ValidationFailed { message }) => message.clone(),
            other => panic!("expected a validation failure, got {other:?}"),
        }
    }
}

async fn run_on_network(network: serde_json::Value, command: unifly_api::Command) -> Written {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;
    let route = integration_route(&format!("networks/{NETWORK}"));
    Mock::given(method("GET"))
        .and(path(route.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(network.clone()))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(route.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(network.clone()))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(route.clone()))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(integration_route("wans")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "offset": 0, "limit": 25, "count": 2, "totalCount": 2, "data": [
                {"id": PRIMARY_WAN, "name": "WAN"},
                {"id": "6b1e5f3c-2b4d-4e6f-9a0b-1c2d3e4f5a66", "name": "WAN2"},
            ]
        })))
        .mount(&server)
        .await;
    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("the-key")),
        LEGACY_SITE_NAME,
        false,
    ));
    controller.connect_lightweight().await.unwrap();
    let result = controller.execute(command).await;
    controller.disconnect().await;
    let requests = server
        .received_requests()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|request| request.url.path() == route && request.method.as_str() != "GET")
        .collect();
    Written { result, requests }
}

/// Boxed: the controller's futures are large, and every test holds one.
fn update_network(
    network: serde_json::Value,
    update: serde_json::Value,
) -> std::pin::Pin<Box<dyn Future<Output = Written>>> {
    Box::pin(run_on_network(
        network,
        unifly_api::Command::UpdateNetwork {
            id: unifly_api::EntityId::from(NETWORK.to_owned()),
            update: serde_json::from_value(update).unwrap(),
        },
    ))
}

/// `network` as an update sends it back: without the fields only the
/// controller sets. DHCP guarding is always named; the API reads a null
/// there the same as a missing key, as the feature being off.
fn as_written(network: &serde_json::Value) -> serde_json::Value {
    let mut expected = network.clone();
    let fields = expected.as_object_mut().unwrap();
    for read_only in ["id", "default", "metadata"] {
        fields.remove(read_only);
    }
    fields.entry("dhcpGuarding").or_insert(json!(null));
    expected
}

#[tokio::test]
async fn a_rename_sends_every_other_network_field_back_unchanged() {
    let network = gateway_network();
    let body = update_network(network.clone(), json!({"name": "Workshop"}))
        .await
        .put_body();

    let mut expected = as_written(&network);
    expected["name"] = json!("Workshop");
    assert_eq!(body, expected);
}

#[tokio::test]
async fn clearing_the_dns_servers_removes_the_override_and_nothing_else() {
    let network = gateway_network();
    let body = update_network(network.clone(), json!({"dhcp": {"dns_servers": []}}))
        .await
        .put_body();

    let mut expected = as_written(&network);
    expected["ipv4Configuration"]["dhcpConfiguration"]
        .as_object_mut()
        .unwrap()
        .remove("dnsServerIpAddressesOverride");
    assert_eq!(
        body, expected,
        "no override key means the servers the gateway picks; an empty list is not valid"
    );
}

#[tokio::test]
async fn dns_servers_must_be_at_most_four_ipv4_addresses() {
    let five =
        json!({"dhcp": {"dns_servers": ["1.1.1.1", "1.0.0.1", "8.8.8.8", "8.8.4.4", "9.9.9.9"]}});
    let refusal = update_network(gateway_network(), five).await.refusal();
    assert!(refusal.contains("at most 4"), "{refusal}");

    let ipv6 = json!({"dhcp": {"dns_servers": ["2001:db8::53"]}});
    let refusal = update_network(gateway_network(), ipv6).await.refusal();
    assert!(refusal.contains("not an IPv4 address"), "{refusal}");

    let four = json!({"dhcp": {"dns_servers": ["1.1.1.1", "1.0.0.1", "8.8.8.8", "8.8.4.4"]}});
    let body = update_network(gateway_network(), four).await.put_body();
    assert_eq!(
        body["ipv4Configuration"]["dhcpConfiguration"]["dnsServerIpAddressesOverride"],
        json!(["1.1.1.1", "1.0.0.1", "8.8.8.8", "8.8.4.4"])
    );
}

#[tokio::test]
async fn turning_ipv6_off_removes_its_configuration_and_nothing_else() {
    let network = gateway_network();
    let body = update_network(network.clone(), json!({"ipv6_enabled": false}))
        .await
        .put_body();

    let mut expected = as_written(&network);
    expected
        .as_object_mut()
        .unwrap()
        .remove("ipv6Configuration");
    assert_eq!(body, expected);
}

/// A yes on a network without IPv6 asks for what the controller's own
/// form starts from: a prefix delegated from the primary WAN, handed out
/// by SLAAC, with the gateway advertised as the router.
#[tokio::test]
async fn turning_ipv6_on_delegates_a_prefix_from_the_primary_wan() {
    let mut network = gateway_network();
    network.as_object_mut().unwrap().remove("ipv6Configuration");
    let body = update_network(network.clone(), json!({"ipv6_enabled": true}))
        .await
        .put_body();
    let mut expected = as_written(&network);
    expected["ipv6Configuration"] = json!({
        "interfaceType": "PREFIX_DELEGATION",
        "prefixDelegationWanInterfaceId": PRIMARY_WAN,
        "clientAddressAssignment": {"slaacEnabled": true},
        "routerAdvertisement": {"priority": "HIGH"},
    });
    assert_eq!(body, expected);

    // Restating that a configured network has IPv6 changes nothing.
    let network = gateway_network();
    let body = update_network(network.clone(), json!({"ipv6_enabled": true}))
        .await
        .put_body();
    assert_eq!(body, as_written(&network));
}

#[tokio::test]
async fn turning_dhcp_off_nulls_the_configuration_and_keeps_the_rest_of_ipv4() {
    let network = gateway_network();
    let body = update_network(network.clone(), json!({"dhcp_enabled": false}))
        .await
        .put_body();

    let mut expected = as_written(&network);
    expected["ipv4Configuration"]["dhcpConfiguration"] = json!(null);
    assert_eq!(body, expected);
}

#[tokio::test]
async fn an_unmanaged_network_refuses_the_settings_it_has_no_field_for() {
    let unmanaged = json!({
        "id": NETWORK, "name": "Trunk", "enabled": true, "management": "UNMANAGED",
        "vlanId": 40, "default": false, "metadata": {"origin": "USER_DEFINED"},
    });
    for (update, setting) in [
        (json!({"isolation_enabled": true}), "isolation"),
        (json!({"internet_access_enabled": false}), "internet access"),
        (json!({"mdns_forwarding_enabled": true}), "mDNS forwarding"),
        (json!({"ipv6_enabled": false}), "IPv6"),
    ] {
        let refusal = update_network(unmanaged.clone(), update).await.refusal();
        assert!(refusal.contains(setting), "{refusal}");
    }

    let body = update_network(unmanaged.clone(), json!({"vlan_id": 41}))
        .await
        .put_body();
    let mut expected = as_written(&unmanaged);
    expected["vlanId"] = json!(41);
    assert_eq!(
        body, expected,
        "an unmanaged network's update carries no gateway or switch setting"
    );
}

#[tokio::test]
async fn a_switch_managed_network_takes_isolation_but_not_gateway_settings() {
    let switched = json!({
        "id": NETWORK, "name": "Storage", "enabled": true, "management": "SWITCH",
        "vlanId": 50, "default": false, "metadata": {"origin": "USER_DEFINED"},
        "deviceId": "5a1e5f3c-2b4d-4e6f-9a0b-1c2d3e4f5a55",
        "isolationEnabled": false, "cellularBackupEnabled": false,
        "ipv4Configuration": {
            "autoScaleEnabled": false, "hostIpAddress": "10.0.50.1", "prefixLength": 24,
        },
    });
    let refusal = update_network(switched.clone(), json!({"internet_access_enabled": false}))
        .await
        .refusal();
    assert!(refusal.contains("internet access"), "{refusal}");

    let body = update_network(switched.clone(), json!({"isolation_enabled": true}))
        .await
        .put_body();
    let mut expected = as_written(&switched);
    expected["isolationEnabled"] = json!(true);
    assert_eq!(body, expected);
}

#[tokio::test]
async fn a_forced_delete_says_so_in_the_query_and_a_plain_one_does_not() {
    for (force, query) in [(false, None), (true, Some("force=true"))] {
        let written = Box::pin(run_on_network(
            gateway_network(),
            unifly_api::Command::DeleteNetwork {
                id: unifly_api::EntityId::from(NETWORK.to_owned()),
                force,
            },
        ))
        .await;
        assert!(written.result.is_ok(), "{:?}", written.result);
        let [delete] = written.requests.as_slice() else {
            panic!("expected one delete, got {}", written.requests.len());
        };
        assert_eq!(delete.method.as_str(), "DELETE");
        assert_eq!(delete.url.query(), query);
    }
}
