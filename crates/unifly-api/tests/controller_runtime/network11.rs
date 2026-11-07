use super::*;

#[tokio::test]
async fn firewall_rename_preserves_disabled_flow_reporting() {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;
    let id = uuid::Uuid::nil();
    let policy = json!({
        "id": id, "name": "Before", "enabled": true,
        "action": {"type": "BLOCK"}, "ipProtocolScope": {"ipVersion": "IPV4_AND_IPV6"},
        "source": {"zoneId": id}, "destination": {"zoneId": id},
        "loggingEnabled": false, "reportFlowEnabled": false, "metadata": {"origin": "USER_DEFINED"}
    });
    let route = integration_route(&format!("firewall/policies/{id}"));
    Mock::given(method("GET"))
        .and(path(&route))
        .respond_with(ResponseTemplate::new(200).set_body_json(policy.clone()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(&route))
        .and(body_partial_json(
            json!({"name": "After", "reportFlowEnabled": false}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(policy))
        .expect(1)
        .mount(&server)
        .await;
    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("fixture")),
        LEGACY_SITE_NAME,
        false,
    ));
    controller.connect_lightweight().await.unwrap();
    controller
        .execute(unifly_api::Command::UpdateFirewallPolicy {
            id: unifly_api::EntityId::Uuid(id),
            update: serde_json::from_value(json!({"name": "After"})).unwrap(),
        })
        .await
        .unwrap();
    controller.disconnect().await;
}

#[tokio::test]
async fn switching_controller_reads_all_pages_and_typed_detail() {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;
    let id = uuid::Uuid::nil();
    let lag = json!({"id": id, "type": "LOCAL", "metadata": {"origin": "USER_DEFINED"},
        "members": [{"deviceId": id, "portIdxs": [1,2]}]});
    for offset in 0..2 {
        Mock::given(method("GET"))
            .and(path(integration_route("switching/lags")))
            .and(wiremock::matchers::query_param(
                "offset",
                offset.to_string(),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "offset": offset, "limit": 200, "count": 1, "totalCount": 2, "data": [lag.clone()]
            })))
            .expect(1)
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path(integration_route(&format!("switching/lags/{id}"))))
        .respond_with(ResponseTemplate::new(200).set_body_json(lag.clone()))
        .expect(1)
        .mount(&server)
        .await;
    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("fixture")),
        LEGACY_SITE_NAME,
        false,
    ));
    controller.connect_lightweight().await.unwrap();
    assert_eq!(controller.list_lags().await.unwrap().len(), 2);
    assert_eq!(
        serde_json::to_value(controller.get_lag(&id).await.unwrap()).unwrap(),
        lag
    );
    controller.disconnect().await;
}

#[tokio::test]
async fn firewall_rename_reserializes_existing_port_ranges_with_schema_fields() {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;
    let id = uuid::Uuid::nil();
    let destination = json!({"zoneId": id, "trafficFilter": {"type": "PORT", "portFilter": {
        "type": "PORTS", "items": [{"type": "PORT_NUMBER_RANGE", "start": 8000, "stop": 9000}], "matchOpposite": false
    }}});
    let policy = json!({"id": id, "name": "Before", "enabled": true,
        "action": {"type": "BLOCK"}, "source": {"zoneId": id}, "destination": destination,
        "ipProtocolScope": {"ipVersion": "IPV4_AND_IPV6"}, "loggingEnabled": false});
    let route = integration_route(&format!("firewall/policies/{id}"));
    Mock::given(method("GET"))
        .and(path(&route))
        .respond_with(ResponseTemplate::new(200).set_body_json(policy.clone()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(&route))
        .and(body_partial_json(
            json!({"name": "After", "destination": destination}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(policy))
        .expect(1)
        .mount(&server)
        .await;
    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("fixture")),
        LEGACY_SITE_NAME,
        false,
    ));
    controller.connect_lightweight().await.unwrap();
    controller
        .execute(unifly_api::Command::UpdateFirewallPolicy {
            id: unifly_api::EntityId::Uuid(id),
            update: serde_json::from_value(json!({"name": "After"})).unwrap(),
        })
        .await
        .unwrap();
    controller.disconnect().await;
}

#[tokio::test]
async fn invalid_existing_port_does_not_erase_destination_or_send_a_write() {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;
    let id = uuid::Uuid::nil();
    let policy = json!({"id": id, "name": "Before", "enabled": true,
        "action": {"type": "BLOCK"}, "source": {"zoneId": id},
        "destination": {"zoneId": id, "trafficFilter": {"type": "PORT", "portFilter": {
            "type": "PORTS", "items": [{"type": "PORT_NUMBER", "value": "invalid"}]
        }}}, "ipProtocolScope": {"ipVersion": "IPV4_AND_IPV6"}, "loggingEnabled": false});
    let route = integration_route(&format!("firewall/policies/{id}"));
    Mock::given(method("GET"))
        .and(path(&route))
        .respond_with(ResponseTemplate::new(200).set_body_json(policy))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(&route))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("fixture")),
        LEGACY_SITE_NAME,
        false,
    ));
    controller.connect_lightweight().await.unwrap();
    let error = controller
        .execute(unifly_api::Command::UpdateFirewallPolicy {
            id: unifly_api::EntityId::Uuid(id),
            update: serde_json::from_value(json!({"name": "After"})).unwrap(),
        })
        .await
        .unwrap_err();
    assert!(matches!(error, CoreError::ValidationFailed { .. }));
    controller.disconnect().await;
}

fn existing_policy() -> serde_json::Value {
    let id = uuid::Uuid::nil();
    json!({"id": id, "name": "Before", "enabled": true,
        "action": {"type": "BLOCK"}, "source": {"zoneId": id}, "destination": {"zoneId": id},
        "ipProtocolScope": {"ipVersion": "IPV4_AND_IPV6"}, "loggingEnabled": false})
}

async fn assert_firewall_update(
    policy: serde_json::Value,
    update: serde_json::Value,
    expected: Option<serde_json::Value>,
) {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;
    let id = uuid::Uuid::nil();
    let route = integration_route(&format!("firewall/policies/{id}"));
    Mock::given(method("GET"))
        .and(path(&route))
        .respond_with(ResponseTemplate::new(200).set_body_json(policy.clone()))
        .expect(1)
        .mount(&server)
        .await;
    let should_write = expected.is_some();
    let mut write = Mock::given(method("PUT")).and(path(&route));
    if let Some(expected) = expected {
        write = write.and(body_partial_json(expected));
    }
    write
        .respond_with(ResponseTemplate::new(200).set_body_json(policy))
        .expect(u64::from(should_write))
        .mount(&server)
        .await;
    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("fixture")),
        LEGACY_SITE_NAME,
        false,
    ));
    controller.connect_lightweight().await.unwrap();
    let result = controller
        .execute(unifly_api::Command::UpdateFirewallPolicy {
            id: unifly_api::EntityId::Uuid(id),
            update: serde_json::from_value(update).unwrap(),
        })
        .await;
    if should_write {
        result.unwrap();
    } else {
        let error = result.unwrap_err();
        assert!(
            matches!(error, CoreError::ValidationFailed { .. }),
            "{error}"
        );
        assert!(
            error.to_string().contains("unsupported existing firewall"),
            "{error}"
        );
    }
    controller.disconnect().await;
    server.verify().await;
}

#[tokio::test]
async fn firewall_field_updates_preserve_network11_filter_variants_on_both_sides() {
    let id = uuid::Uuid::nil();
    let port = json!({"type": "PORTS", "items": [{"type": "PORT_NUMBER", "value": 443}], "matchOpposite": true});
    for side in ["source", "destination"] {
        for mut filter in [
            json!({"type": "VPN_SERVER", "vpnServerFilter": {"vpnServerIds": [id], "matchOpposite": true}}),
            json!({"type": "SITE_TO_SITE_VPN_TUNNEL", "siteToSiteVpnTunnelFilter": {"siteToSiteVpnTunnelId": id}}),
            json!({"type": "IPV6_IID", "ipv6IidFilter": {"ipv6Iid": "2001:db8::1/::ffff:ffff:ffff:ffff", "matchOpposite": true}}),
        ] {
            filter["portFilter"] = port.clone();
            if side == "source" && filter["type"] == "IPV6_IID" {
                filter["macAddressFilter"] = json!("00:11:22:33:44:55");
            }
            let endpoint = json!({"zoneId": id, "trafficFilter": filter});
            let mut policy = existing_policy();
            policy[side] = endpoint.clone();
            let mut expected = json!({"reportFlowEnabled": false});
            expected[side] = endpoint;
            assert_firewall_update(
                policy,
                json!({"report_flow_enabled": false}),
                Some(expected),
            )
            .await;
        }
    }
}

#[tokio::test]
async fn firewall_field_updates_preserve_domains_ranges_and_mac_restrictions() {
    let id = uuid::Uuid::nil();
    for side in ["source", "destination"] {
        for tag in ["IP_ADDRESS_RANGE", "RANGE"] {
            let filter = json!({"type": "IP_ADDRESS", "ipAddressFilter": {
                "type": "IP_ADDRESSES", "matchOpposite": true,
                "items": [{"type": tag, "start": "192.0.2.10", "stop": "192.0.2.20"}]
            }});
            let endpoint = json!({"zoneId": id, "trafficFilter": filter});
            let mut policy = existing_policy();
            policy[side] = endpoint.clone();
            let mut expected = json!({});
            expected[side] = endpoint;
            expected[side]["trafficFilter"]["ipAddressFilter"]["items"][0]["type"] =
                json!("IP_ADDRESS_RANGE");
            assert_firewall_update(policy, json!({"logging_enabled": true}), Some(expected)).await;
        }
    }
    for tag in ["DOMAINS", "SPECIFIC"] {
        let mut policy = existing_policy();
        policy["destination"]["trafficFilter"] =
            json!({"type": "DOMAIN", "domainFilter": {"type": tag, "domains": ["example.com"]}});
        let expected = json!({"destination": {"trafficFilter": {"type": "DOMAIN", "domainFilter": {"type": "DOMAINS", "domains": ["example.com"]}}}});
        assert_firewall_update(policy, json!({"logging_enabled": true}), Some(expected)).await;
    }
    for filter in [
        json!({"type": "NETWORK", "networkFilter": {"networkIds": [id], "matchOpposite": false}, "macAddressFilter": "02:11:22:33:44:55"}),
        json!({"type": "IP_ADDRESS", "ipAddressFilter": {"type": "IP_ADDRESSES", "items": [{"type": "IP_ADDRESS", "value": "192.0.2.10"}], "matchOpposite": false}, "macAddressFilter": "02:11:22:33:44:55"}),
    ] {
        let mut policy = existing_policy();
        policy["source"]["trafficFilter"] = filter.clone();
        assert_firewall_update(
            policy,
            json!({"logging_enabled": true}),
            Some(json!({"source": {"trafficFilter": filter}})),
        )
        .await;
    }
}

#[tokio::test]
async fn firewall_unknown_filters_at_every_nesting_level_block_untouched_puts() {
    let unknown = json!({"type": "FUTURE_FILTER", "meaningfulRestriction": "do-not-drop"});
    for side in ["source", "destination"] {
        let mut filters = vec![
            unknown.clone(),
            json!({"type": "PORT", "portFilter": unknown}),
            json!({"type": "PORT", "portFilter": {"type": "PORTS", "items": [unknown]}}),
            json!({"type": "IP_ADDRESS", "ipAddressFilter": unknown}),
            json!({"type": "IP_ADDRESS", "ipAddressFilter": {"type": "IP_ADDRESSES", "items": [unknown]}}),
            json!({"type": "VPN_SERVER", "vpnServerFilter": {"vpnServerIds": [uuid::Uuid::nil()], "matchOpposite": false}, "portFilter": unknown}),
        ];
        if side == "destination" {
            filters.push(json!({"type": "DOMAIN", "domainFilter": unknown}));
        }
        for filter in filters {
            let mut policy = existing_policy();
            policy[side]["trafficFilter"] = filter;
            assert_firewall_update(policy, json!({"report_flow_enabled": false}), None).await;
        }
    }
}

#[tokio::test]
async fn firewall_unknown_filter_can_be_replaced_explicitly() {
    for side in ["source", "destination"] {
        let mut policy = existing_policy();
        policy[side]["trafficFilter"] = json!({"type": "FUTURE_FILTER", "restriction": "old"});
        let mut update = json!({});
        update[format!("{side}_filter")] = json!({"type": "port", "ports": ["443"]});
        let mut expected = json!({});
        expected[side] = json!({"zoneId": uuid::Uuid::nil(), "trafficFilter": {
            "type": "PORT", "portFilter": {"type": "PORTS", "items": [{"type": "PORT_NUMBER", "value": 443}], "matchOpposite": false}
        }});
        assert_firewall_update(policy, update, Some(expected)).await;
    }
}

#[tokio::test]
async fn firewall_allow_updates_require_explicit_opt_in_for_new_return_rules() {
    for (existing_action, update, expected_return) in [
        (json!({"type": "BLOCK"}), json!({"action": "Allow"}), false),
        (json!({"type": "REJECT"}), json!({"action": "Allow"}), false),
        (
            json!({"type": "ALLOW", "allowReturnTraffic": true}),
            json!({"name": "After"}),
            true,
        ),
        (
            json!({"type": "ALLOW", "allowReturnTraffic": false}),
            json!({"name": "After"}),
            false,
        ),
        (
            json!({"type": "BLOCK"}),
            json!({"action": "Allow", "allow_return_traffic": true}),
            true,
        ),
    ] {
        let mut policy = existing_policy();
        policy["action"] = existing_action;
        assert_firewall_update(
            policy,
            update,
            Some(json!({"action": {"type": "ALLOW", "allowReturnTraffic": expected_return}})),
        )
        .await;
    }
}

fn dhcp_network_create_request() -> serde_json::Value {
    json!({
        "name": "DHCP test", "enabled": true, "vlan_id": 30,
        "dhcp_enabled": true, "isolation_enabled": false, "internet_access_enabled": true,
        "subnet": "10.0.30.1/24", "dhcp_range_start": "10.0.30.100",
        "dhcp_range_stop": "10.0.30.200"
    })
}

#[tokio::test]
async fn gateway_dhcp_create_rejects_incomplete_configuration_without_posting() {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;
    Mock::given(method("POST"))
        .and(path(integration_route("networks")))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("fixture")),
        LEGACY_SITE_NAME,
        false,
    ));
    controller.connect_lightweight().await.unwrap();
    for explicit_gateway in [false, true] {
        for missing in [
            vec!["subnet"],
            vec!["dhcp_range_start"],
            vec!["dhcp_range_stop"],
            vec!["dhcp_range_start", "dhcp_range_stop"],
        ] {
            let mut request = dhcp_network_create_request();
            if explicit_gateway {
                request["management"] = json!("Gateway");
            }
            for field in &missing {
                request.as_object_mut().unwrap().remove(*field);
            }
            let error = controller
                .execute(unifly_api::Command::CreateNetwork(
                    serde_json::from_value(request).unwrap(),
                ))
                .await
                .expect_err("incomplete DHCP must fail locally");
            assert!(
                matches!(error, CoreError::ValidationFailed { .. }),
                "{error}"
            );
            assert!(
                error.to_string().contains(if missing == ["subnet"] {
                    "IPv4 subnet"
                } else {
                    "both range start and stop"
                }),
                "{error}"
            );
        }
    }
    controller.disconnect().await;
    server.verify().await;
}

#[tokio::test]
async fn gateway_create_emits_complete_dhcp_server_or_explicitly_disabled_dhcp() {
    for enabled in [false, true] {
        let server = MockServer::start().await;
        mock_api_key_with_legacy(&server).await;
        let mut request = dhcp_network_create_request();
        request["dhcp_enabled"] = json!(enabled);
        if !enabled {
            request.as_object_mut().unwrap().remove("dhcp_range_start");
            request.as_object_mut().unwrap().remove("dhcp_range_stop");
        }
        let expected = if enabled {
            json!({"mode": "SERVER", "leaseTimeSeconds": 86400,
                "pingConflictDetectionEnabled": false,
                "ipAddressRange": {"start": "10.0.30.100", "stop": "10.0.30.200"}})
        } else {
            serde_json::Value::Null
        };
        Mock::given(method("POST"))
            .and(path(integration_route("networks")))
            .and(body_partial_json(
                json!({"management": "GATEWAY", "ipv4Configuration": {
                    "hostIpAddress": "10.0.30.1", "prefixLength": 24, "dhcpConfiguration": expected
                }}),
            ))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "id": uuid::Uuid::nil(), "name": "DHCP test", "enabled": true,
                "management": "GATEWAY", "vlanId": 30, "metadata": {}
            })))
            .expect(1)
            .mount(&server)
            .await;
        let controller = Controller::new(base_config(
            Url::parse(&server.uri()).unwrap(),
            AuthCredentials::ApiKey(secret("fixture")),
            LEGACY_SITE_NAME,
            false,
        ));
        controller.connect_lightweight().await.unwrap();
        controller
            .execute(unifly_api::Command::CreateNetwork(
                serde_json::from_value(request).unwrap(),
            ))
            .await
            .unwrap();
        controller.disconnect().await;
        server.verify().await;
    }
}

#[tokio::test]
async fn network_create_rejects_invalid_ranges_and_unsupported_settings_without_posting() {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;
    Mock::given(method("POST"))
        .and(path(integration_route("networks")))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("fixture")),
        LEGACY_SITE_NAME,
        false,
    ));
    controller.connect_lightweight().await.unwrap();
    let mut invalid = Vec::new();
    for management in ["Gateway", "Switch"] {
        for (start, stop) in [
            ("invalid", "10.0.30.200"),
            ("::1", "10.0.30.200"),
            ("10.0.30.100", "::1"),
            ("10.0.30.201", "10.0.30.200"),
        ] {
            let mut request = dhcp_network_create_request();
            request["management"] = json!(management);
            if management == "Switch" {
                request["device_id"] = json!(uuid::Uuid::nil());
            }
            request["dhcp_range_start"] = json!(start);
            request["dhcp_range_stop"] = json!(stop);
            invalid.push(request);
        }
    }
    for settings in [
        json!({"management": "Unmanaged"}),
        json!({"management": "Switch"}),
        json!({"management": "Switch", "device_id": "legacy-switch"}),
    ] {
        let mut request = dhcp_network_create_request();
        request
            .as_object_mut()
            .unwrap()
            .extend(settings.as_object().unwrap().clone());
        invalid.push(request);
    }
    let mut unmanaged_subnet = dhcp_network_create_request();
    unmanaged_subnet["management"] = json!("Unmanaged");
    unmanaged_subnet["dhcp_enabled"] = json!(false);
    unmanaged_subnet
        .as_object_mut()
        .unwrap()
        .remove("dhcp_range_start");
    unmanaged_subnet
        .as_object_mut()
        .unwrap()
        .remove("dhcp_range_stop");
    invalid.push(unmanaged_subnet);
    for request in invalid {
        let error = controller
            .execute(unifly_api::Command::CreateNetwork(
                serde_json::from_value(request).unwrap(),
            ))
            .await
            .expect_err("invalid configuration must fail before POST");
        assert!(
            matches!(error, CoreError::ValidationFailed { .. }),
            "{error}"
        );
    }
    controller.disconnect().await;
    server.verify().await;
}

#[tokio::test]
async fn switch_dhcp_update_preserves_owner_and_omits_gateway_only_fields() {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;
    let id = uuid::Uuid::nil();
    let route = integration_route(&format!("networks/{id}"));
    let network = json!({"id": id, "name": "Switch VLAN", "enabled": true,
        "management": "SWITCH", "vlanId": 30, "metadata": {}, "deviceId": id,
        "isolationEnabled": false, "cellularBackupEnabled": false,
        "ipv4Configuration": {"hostIpAddress": "10.0.30.1", "prefixLength": 24,
            "autoScaleEnabled": false, "dhcpConfiguration": null}});
    Mock::given(method("GET"))
        .and(path(&route))
        .respond_with(ResponseTemplate::new(200).set_body_json(network.clone()))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(&route))
        .and(move |request: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            let dhcp = &body["ipv4Configuration"]["dhcpConfiguration"];
            body["deviceId"] == json!(id)
                && body["management"] == "SWITCH"
                && dhcp["mode"] == "SERVER"
                && dhcp["leaseTimeSeconds"] == 86400
                && dhcp.get("pingConflictDetectionEnabled").is_none()
                && dhcp["ipAddressRange"] == json!({"start": "10.0.30.100", "stop": "10.0.30.200"})
        })
        .respond_with(ResponseTemplate::new(200).set_body_json(network))
        .expect(1)
        .mount(&server)
        .await;
    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("fixture")),
        LEGACY_SITE_NAME,
        false,
    ));
    controller.connect_lightweight().await.unwrap();
    controller
        .execute(unifly_api::Command::UpdateNetwork {
            id: unifly_api::EntityId::Uuid(id),
            update: serde_json::from_value(json!({"dhcp_enabled": true, "dhcp": {
                "range_start": "10.0.30.100", "range_stop": "10.0.30.200"
            }}))
            .unwrap(),
        })
        .await
        .unwrap();
    controller.disconnect().await;
    server.verify().await;
}
