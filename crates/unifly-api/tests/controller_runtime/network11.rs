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
