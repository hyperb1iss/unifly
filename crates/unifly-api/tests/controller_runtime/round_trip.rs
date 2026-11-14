//! Updates that replace a whole record must send back every field the
//! request did not name. Each test fetches a record, changes one thing,
//! and compares the body the controller received with the fetched one.

use super::*;

/// Connect in API-key mode, serve `record` at `route`, run `command`, and
/// return the body of the one write the controller received there.
async fn written_body(
    write_method: &str,
    route: &str,
    record: &serde_json::Value,
    command: unifly_api::Command,
) -> serde_json::Value {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;
    Mock::given(method("GET"))
        .and(path(route))
        .respond_with(ResponseTemplate::new(200).set_body_json(record.clone()))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method(write_method))
        .and(path(route))
        .respond_with(ResponseTemplate::new(200).set_body_json(record.clone()))
        .expect(1)
        .mount(&server)
        .await;
    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("the-key")),
        LEGACY_SITE_NAME,
        false,
    ));
    controller.connect_lightweight().await.unwrap();
    controller.execute(command).await.unwrap();
    controller.disconnect().await;
    server.verify().await;

    let write = server
        .received_requests()
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|request| request.method.as_str() == write_method && request.url.path() == route)
        .expect("the update writes the record");
    serde_json::from_slice(&write.body).unwrap()
}

/// `record` as an update may send it: without the fields only the
/// controller sets, and with `changes` applied.
fn expected_write(
    record: &serde_json::Value,
    read_only: &[&str],
    changes: &serde_json::Value,
) -> serde_json::Value {
    let mut expected = record.clone();
    let fields = expected.as_object_mut().unwrap();
    for field in read_only {
        fields.remove(*field);
    }
    for (field, value) in changes.as_object().unwrap() {
        fields.insert(field.clone(), value.clone());
    }
    expected
}

fn acl_toggle(id: uuid::Uuid, enabled: bool) -> unifly_api::Command {
    unifly_api::Command::UpdateAclRule {
        id: unifly_api::EntityId::Uuid(id),
        update: serde_json::from_value(json!({ "enabled": enabled })).unwrap(),
    }
}

#[tokio::test]
async fn disabling_an_ipv4_acl_rule_keeps_its_protocol_filter() {
    let id = uuid::Uuid::from_u128(0xac1);
    let network = uuid::Uuid::from_u128(0x10);
    let rule = json!({
        "id": id, "type": "IPV4", "name": "Block camera web", "action": "BLOCK",
        "enabled": true, "index": 3, "description": "TCP only",
        "protocolFilter": ["TCP"],
        "sourceFilter": {"type": "NETWORKS", "networkIds": [network]},
        "destinationFilter": {
            "type": "IP_ADDRESSES_OR_SUBNETS",
            "ipAddressesOrSubnets": ["10.0.50.0/24"],
            "portFilter": [80, 443],
        },
        "enforcingDeviceFilter": {"deviceIds": [uuid::Uuid::from_u128(0xd1)]},
        "metadata": {"origin": "USER_DEFINED"},
    });
    let route = integration_route(&format!("acl-rules/{id}"));

    let body = written_body("PUT", &route, &rule, acl_toggle(id, false)).await;

    assert_eq!(body["protocolFilter"], json!(["TCP"]));
    assert_eq!(
        body,
        expected_write(
            &rule,
            &["id", "index", "metadata"],
            &json!({"enabled": false})
        ),
        "everything but `enabled` goes back as fetched"
    );
}

#[tokio::test]
async fn enabling_a_mac_acl_rule_keeps_its_network_filter() {
    let id = uuid::Uuid::from_u128(0xac2);
    let network = uuid::Uuid::from_u128(0x20);
    let rule = json!({
        "id": id, "type": "MAC", "name": "Allow printer", "action": "ALLOW",
        "enabled": false, "index": 0,
        "networkIdFilter": network,
        "sourceFilter": {"type": "MAC_ADDRESSES", "macAddresses": ["aa:bb:cc:00:11:22"]},
        "destinationFilter": {"type": "MAC_ADDRESSES", "macAddresses": ["aa:bb:cc:00:11:33"]},
        "metadata": {"origin": "USER_DEFINED"},
    });
    let route = integration_route(&format!("acl-rules/{id}"));

    let body = written_body("PUT", &route, &rule, acl_toggle(id, true)).await;

    assert_eq!(body["networkIdFilter"], json!(network));
    assert_eq!(
        body,
        expected_write(
            &rule,
            &["id", "index", "metadata"],
            &json!({"enabled": true})
        ),
        "everything but `enabled` goes back as fetched"
    );
}

/// A user policy that uses every part of the update schema.
fn rich_policy(id: uuid::Uuid) -> serde_json::Value {
    json!({
        "id": id, "name": "Block IoT to servers", "description": "after hours",
        "enabled": true, "index": 10_002,
        "action": {"type": "BLOCK"},
        "source": {
            "zoneId": uuid::Uuid::from_u128(0x51),
            "trafficFilter": {
                "type": "NETWORK",
                "networkFilter": {
                    "networkIds": [uuid::Uuid::from_u128(0x61)],
                    "matchOpposite": false,
                },
                "macAddressFilter": "aa:bb:cc:00:11:22",
            },
        },
        "destination": {
            "zoneId": uuid::Uuid::from_u128(0x52),
            "trafficFilter": {
                "type": "IP_ADDRESS",
                "ipAddressFilter": {
                    "type": "IP_ADDRESSES",
                    "items": [{"type": "SUBNET", "value": "10.0.9.0/24"}],
                    "matchOpposite": false,
                },
                "portFilter": {
                    "type": "PORTS",
                    "items": [{"type": "PORT_NUMBER_RANGE", "start": 8000, "stop": 8100}],
                    "matchOpposite": true,
                },
            },
        },
        "ipProtocolScope": {
            "ipVersion": "IPV4",
            "protocolFilter": {
                "type": "NAMED_PROTOCOL",
                "protocol": {"name": "TCP"},
                "matchOpposite": false,
            },
        },
        "connectionStateFilter": ["NEW", "ESTABLISHED"],
        "ipsecFilter": "MATCH_NOT_ENCRYPTED",
        "schedule": {
            "mode": "EVERY_DAY",
            "timeFilter": {"startTime": "22:00", "stopTime": "06:00"},
        },
        "loggingEnabled": true, "reportFlowEnabled": false,
        "metadata": {"origin": "USER_DEFINED"},
    })
}

#[tokio::test]
async fn disabling_a_firewall_policy_sends_the_rest_of_it_back_unchanged() {
    let id = uuid::Uuid::from_u128(0xf1);
    let policy = rich_policy(id);
    let route = integration_route(&format!("firewall/policies/{id}"));

    let body = written_body(
        "PUT",
        &route,
        &policy,
        unifly_api::Command::UpdateFirewallPolicy {
            id: unifly_api::EntityId::Uuid(id),
            update: serde_json::from_value(json!({"enabled": false})).unwrap(),
        },
    )
    .await;

    assert_eq!(
        body,
        expected_write(
            &policy,
            &["id", "index", "metadata"],
            &json!({"enabled": false})
        ),
    );
}

#[tokio::test]
async fn changing_a_policy_ip_version_keeps_its_protocol_filter() {
    let id = uuid::Uuid::from_u128(0xf2);
    let policy = rich_policy(id);
    let route = integration_route(&format!("firewall/policies/{id}"));

    let body = written_body(
        "PUT",
        &route,
        &policy,
        unifly_api::Command::UpdateFirewallPolicy {
            id: unifly_api::EntityId::Uuid(id),
            update: serde_json::from_value(json!({"ip_version": "IPV4_AND_IPV6"})).unwrap(),
        },
    )
    .await;

    let mut scope = policy["ipProtocolScope"].clone();
    scope["ipVersion"] = json!("IPV4_AND_IPV6");
    assert_eq!(
        body,
        expected_write(
            &policy,
            &["id", "index", "metadata"],
            &json!({"ipProtocolScope": scope})
        ),
        "the policy still matches TCP only"
    );
}

#[tokio::test]
async fn renaming_a_firewall_zone_keeps_its_networks() {
    let id = uuid::Uuid::from_u128(0x71);
    let zone = json!({
        "id": id, "name": "Cameras",
        "networkIds": [uuid::Uuid::from_u128(0x81), uuid::Uuid::from_u128(0x82)],
        "metadata": {"origin": "USER_DEFINED"},
    });
    let route = integration_route(&format!("firewall/zones/{id}"));

    let body = written_body(
        "PUT",
        &route,
        &zone,
        unifly_api::Command::UpdateFirewallZone {
            id: unifly_api::EntityId::Uuid(id),
            update: serde_json::from_value(json!({"name": "CCTV"})).unwrap(),
        },
    )
    .await;

    assert_eq!(
        body,
        expected_write(&zone, &["id", "metadata"], &json!({"name": "CCTV"})),
    );
}

#[tokio::test]
async fn disabling_a_dns_record_keeps_its_fields() {
    let id = uuid::Uuid::from_u128(0x91);
    let record = json!({
        "id": id, "type": "SRV_RECORD", "enabled": true, "domain": "example.internal",
        "serverDomain": "sip.example.internal", "service": "_sip", "protocol": "_tcp",
        "port": 5060, "priority": 10, "weight": 5,
        "metadata": {"origin": "USER_DEFINED"},
    });
    let route = integration_route(&format!("dns/policies/{id}"));

    let body = written_body(
        "PUT",
        &route,
        &record,
        unifly_api::Command::UpdateDnsPolicy {
            id: unifly_api::EntityId::Uuid(id),
            update: serde_json::from_value(json!({"enabled": false})).unwrap(),
        },
    )
    .await;

    assert_eq!(
        body,
        expected_write(&record, &["id", "metadata"], &json!({"enabled": false})),
    );
}

#[tokio::test]
async fn disabling_a_nat_rule_puts_the_whole_rule_back() {
    let rule = json!({
        "_id": "nat-7", "description": "web", "enabled": true, "type": "DNAT",
        "ip_version": "IPV4", "is_predefined": false, "rule_index": 7,
        "protocol": "tcp", "ip_address": "10.0.0.10", "port": "8443",
        "in_interface": "wan-1", "logging": true, "exclude": false,
        "setting_preference": "manual", "pppoe_use_base_interface": false,
        "destination_filter": {
            "filter_type": "ADDRESS_AND_PORT", "address": "203.0.113.9", "port": "443",
            "invert_address": false, "invert_port": false,
        },
        "source_filter": {"filter_type": "NONE", "firewall_group_ids": []},
        "a_field_from_a_later_release": {"kept": true},
    });
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;
    Mock::given(method("GET"))
        .and(path("/proxy/network/v2/api/site/default/nat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([rule])))
        .with_priority(1)
        .mount(&server)
        .await;
    let route = "/proxy/network/v2/api/site/default/nat/nat-7";
    Mock::given(method("PUT"))
        .and(path(route))
        .and(body_json(expected_write(
            &rule,
            &[],
            &json!({"enabled": false}),
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(rule.clone()))
        .expect(1)
        .mount(&server)
        .await;
    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("the-key")),
        LEGACY_SITE_NAME,
        false,
    ));
    controller.connect().await.unwrap();
    controller
        .execute(unifly_api::Command::UpdateNatPolicy {
            id: unifly_api::EntityId::from("nat-7".to_owned()),
            update: serde_json::from_value(json!({"enabled": false})).unwrap(),
        })
        .await
        .unwrap();
    controller.disconnect().await;
    server.verify().await;
}

fn patch_policy(id: uuid::Uuid, fields: &serde_json::Value) -> unifly_api::Command {
    let flag = |name: &str| fields.get(name).and_then(serde_json::Value::as_bool);
    unifly_api::Command::PatchFirewallPolicy {
        id: unifly_api::EntityId::Uuid(id),
        enabled: flag("enabled"),
        logging: flag("logging"),
        report_flow_enabled: flag("report_flow_enabled"),
    }
}

#[tokio::test]
async fn a_policy_patch_carries_logging_and_flow_reporting_only() {
    let id = uuid::Uuid::from_u128(0xf3);
    let route = integration_route(&format!("firewall/policies/{id}"));

    let body = written_body(
        "PATCH",
        &route,
        &rich_policy(id),
        patch_policy(id, &json!({"logging": false, "report_flow_enabled": true})),
    )
    .await;

    assert_eq!(
        body,
        json!({"loggingEnabled": false, "reportFlowEnabled": true})
    );
}

/// The controller's PATCH has no `enabled` field, so the change must
/// arrive as a full update, and never as a PATCH the controller could
/// accept and ignore.
#[tokio::test]
async fn a_policy_patch_that_sets_enabled_is_sent_as_a_full_update() {
    let id = uuid::Uuid::from_u128(0xf4);
    let policy = rich_policy(id);
    let route = integration_route(&format!("firewall/policies/{id}"));

    let body = written_body(
        "PUT",
        &route,
        &policy,
        patch_policy(id, &json!({"enabled": false, "logging": false})),
    )
    .await;

    assert_eq!(
        body,
        expected_write(
            &policy,
            &["id", "index", "metadata"],
            &json!({"enabled": false, "loggingEnabled": false})
        ),
    );
}
