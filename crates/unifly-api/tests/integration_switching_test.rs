#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use unifly_api::{
    ControllerPlatform, IntegrationClient,
    integration_types::{
        FirewallPolicyPatch, FirewallPolicyResponse, LagResponse, McLagDomainResponse,
        SwitchStackResponse, WifiBroadcastCreateUpdate, WifiBroadcastDetailsResponse,
    },
};
use uuid::Uuid;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn member() -> Value {
    json!({"deviceId": Uuid::nil(), "portIdxs": [1, 2]})
}
fn stack_member() -> Value {
    json!({"unitId": 1, "unitMacAddress": "00:11:22:33:44:55", "portIdxs": [3, 4]})
}
fn metadata() -> Value {
    json!({"origin": "USER_DEFINED"})
}
fn lag(kind: &str) -> Value {
    let mut value =
        json!({"id": Uuid::nil(), "metadata": metadata(), "type": kind, "members": [member()]});
    if kind == "MULTI_CHASSIS" {
        value["mcLagDomainId"] = json!(Uuid::nil());
    }
    if kind == "SWITCH_STACK" {
        value["switchStackId"] = json!(Uuid::nil());
        value["members"] = json!([stack_member()]);
    }
    value
}
fn domain() -> Value {
    json!({"id": Uuid::nil(), "name": "Core pair", "metadata": metadata(), "peers": [
    {"deviceId": Uuid::nil(), "linkPortIdxs": [47,48], "role": "TOP"},
    {"deviceId": Uuid::from_u128(1), "linkPortIdxs": [47,48], "role": "BOTTOM"}
], "lags": [{"id": Uuid::nil(), "metadata": metadata(), "members": [member()]}]})
}
fn stack() -> Value {
    json!({"id": Uuid::nil(), "name": "Core stack", "metadata": metadata(), "units": [
    {"id": 1, "macAddress": "00:11:22:33:44:55", "order": 1, "role": "ACTIVE_CONTROLLER"},
    {"id": 2, "macAddress": "00:11:22:33:44:66"}
], "lags": [{"id": Uuid::nil(), "metadata": metadata(), "members": [stack_member()]}]})
}

async fn setup(platform: ControllerPlatform) -> (MockServer, IntegrationClient) {
    unifly_api::transport::ensure_crypto_provider();
    let server = MockServer::start().await;
    let client =
        IntegrationClient::from_reqwest(&server.uri(), reqwest::Client::new(), platform).unwrap();
    (server, client)
}

#[test]
fn switching_schema_round_trips_all_membership_shapes() {
    for kind in ["LOCAL", "MULTI_CHASSIS", "SWITCH_STACK"] {
        let original = lag(kind);
        let parsed: LagResponse = serde_json::from_value(original.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), original);
    }
    let original = domain();
    let parsed: McLagDomainResponse = serde_json::from_value(original.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), original);
    let original = stack();
    let parsed: SwitchStackResponse = serde_json::from_value(original.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), original);
}

#[test]
fn switching_shapes_require_their_discriminated_membership() {
    let mut value = lag("MULTI_CHASSIS");
    value.as_object_mut().unwrap().remove("mcLagDomainId");
    assert!(serde_json::from_value::<LagResponse>(value).is_err());
    let mut value = lag("SWITCH_STACK");
    value["members"] = json!([member()]);
    assert!(serde_json::from_value::<LagResponse>(value).is_err());
}

#[tokio::test]
async fn lag_list_and_detail_use_documented_routes() {
    let (server, client) = setup(ControllerPlatform::UnifiOs).await;
    let id = Uuid::nil();
    let value = lag("LOCAL");
    let route = format!("/proxy/network/integration/v1/sites/{id}/switching/lags");
    Mock::given(method("GET"))
        .and(path(&route))
        .and(query_param("offset", "3"))
        .and(query_param("limit", "17"))
        .and(query_param("filter", "metadata.origin.eq('USER_DEFINED')"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "offset": 3, "limit": 17, "count": 1, "totalCount": 4, "data": [value.clone()]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{route}/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(value.clone()))
        .expect(1)
        .mount(&server)
        .await;
    let page = client
        .list_lags(&id, 3, 17, Some("metadata.origin.eq('USER_DEFINED')"))
        .await
        .unwrap();
    assert_eq!(page.offset, 3);
    assert_eq!(page.total_count, 4);
    assert_eq!(serde_json::to_value(&page.data[0]).unwrap(), value);
    assert_eq!(
        serde_json::to_value(client.get_lag(&id, &id).await.unwrap()).unwrap(),
        value
    );
}

#[tokio::test]
async fn mc_lag_domain_list_and_detail_use_documented_routes() {
    let (server, client) = setup(ControllerPlatform::UnifiOs).await;
    let id = Uuid::nil();
    let value = domain();
    let route = format!("/proxy/network/integration/v1/sites/{id}/switching/mc-lag-domains");
    Mock::given(method("GET"))
        .and(path(&route))
        .and(query_param("offset", "3"))
        .and(query_param("limit", "17"))
        .and(query_param("filter", "metadata.origin.eq('USER_DEFINED')"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "offset": 3, "limit": 17, "count": 1, "totalCount": 4, "data": [value.clone()]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{route}/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(value.clone()))
        .expect(1)
        .mount(&server)
        .await;
    let page = client
        .list_mc_lag_domains(&id, 3, 17, Some("metadata.origin.eq('USER_DEFINED')"))
        .await
        .unwrap();
    assert_eq!(page.offset, 3);
    assert_eq!(page.total_count, 4);
    assert_eq!(serde_json::to_value(&page.data[0]).unwrap(), value);
    assert_eq!(
        serde_json::to_value(client.get_mc_lag_domain(&id, &id).await.unwrap()).unwrap(),
        value
    );
}

#[tokio::test]
async fn switch_stack_list_and_detail_use_documented_routes() {
    let (server, client) = setup(ControllerPlatform::UnifiOs).await;
    let id = Uuid::nil();
    let value = stack();
    let route = format!("/proxy/network/integration/v1/sites/{id}/switching/switch-stacks");
    Mock::given(method("GET"))
        .and(path(&route))
        .and(query_param("offset", "3"))
        .and(query_param("limit", "17"))
        .and(query_param("filter", "metadata.origin.eq('USER_DEFINED')"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "offset": 3, "limit": 17, "count": 1, "totalCount": 4, "data": [value.clone()]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{route}/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(value.clone()))
        .expect(1)
        .mount(&server)
        .await;
    let page = client
        .list_switch_stacks(&id, 3, 17, Some("metadata.origin.eq('USER_DEFINED')"))
        .await
        .unwrap();
    assert_eq!(page.offset, 3);
    assert_eq!(page.total_count, 4);
    assert_eq!(serde_json::to_value(&page.data[0]).unwrap(), value);
    assert_eq!(
        serde_json::to_value(client.get_switch_stack(&id, &id).await.unwrap()).unwrap(),
        value
    );
}

#[tokio::test]
async fn switching_pagination_uses_server_count_without_a_page_cap() {
    let (server, client) = setup(ControllerPlatform::ClassicController).await;
    let id = Uuid::nil();
    for offset in 0..3 {
        Mock::given(method("GET"))
            .and(path(format!("/integration/v1/sites/{id}/switching/lags")))
            .and(query_param("offset", offset.to_string()))
            .and(query_param("limit", "200"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "offset": offset, "limit": 200, "count": 1, "totalCount": 3, "data": [lag("LOCAL")]
            })))
            .expect(1)
            .mount(&server)
            .await;
    }
    let records = client
        .paginate_all(200, |offset, limit| {
            client.list_lags(&id, offset, limit, None)
        })
        .await
        .unwrap();
    assert_eq!(records.len(), 3);
}

#[tokio::test]
async fn missing_switching_endpoint_is_an_error() {
    let (server, client) = setup(ControllerPlatform::ClassicController).await;
    let id = Uuid::nil();
    Mock::given(method("GET"))
        .and(path(format!("/integration/v1/sites/{id}/switching/lags")))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    assert!(client.list_lags(&id, 0, 25, None).await.is_err());
}

#[test]
fn pinned_schema_contains_all_six_switching_reads() {
    let schema: Value = serde_json::from_str(include_str!("../openapi/11.0.97.json")).unwrap();
    assert_eq!(schema["info"]["version"], "11.0.97");
    for route in [
        "lags",
        "lags/{lagId}",
        "mc-lag-domains",
        "mc-lag-domains/{mcLagDomainId}",
        "switch-stacks",
        "switch-stacks/{switchStackId}",
    ] {
        assert!(
            schema["paths"][format!("/v1/sites/{{siteId}}/switching/{route}")]["get"].is_object()
        );
    }
    assert!(schema["paths"]["/v1/sites/{siteId}/firewall/nat"].is_null());
}

#[test]
fn network11_fields_round_trip_without_inventing_older_flow_settings() {
    let policy = json!({"name": "Block", "enabled": true, "action": {"type": "BLOCK"},
        "loggingEnabled": false, "reportFlowEnabled": false, "source": {}, "destination": {}});
    let parsed: FirewallPolicyResponse = serde_json::from_value(policy.clone()).unwrap();
    assert_eq!(parsed.report_flow_enabled, Some(false));
    assert_eq!(
        serde_json::to_value(parsed).unwrap()["reportFlowEnabled"],
        false
    );
    let mut older = policy;
    older.as_object_mut().unwrap().remove("reportFlowEnabled");
    let parsed: FirewallPolicyResponse = serde_json::from_value(older).unwrap();
    assert_eq!(parsed.report_flow_enabled, None);
    assert!(
        serde_json::to_value(parsed)
            .unwrap()
            .get("reportFlowEnabled")
            .is_none()
    );
    let patch: FirewallPolicyPatch =
        serde_json::from_value(json!({"reportFlowEnabled": false})).unwrap();
    assert_eq!(
        serde_json::to_value(patch).unwrap(),
        json!({"reportFlowEnabled": false})
    );
}

#[test]
fn wifi_detail_and_request_serialize_exact_network11_field_names() {
    let details = json!({"id": Uuid::nil(), "name": "IoT", "type": "STANDARD", "enabled": true,
        "metadata": {}, "securityConfiguration": {}, "channel2gLockedTo6": true, "dtimPeriod2gLockedTo3": true});
    let parsed: WifiBroadcastDetailsResponse = serde_json::from_value(details).unwrap();
    assert!(parsed.channel2g_locked_to6 && parsed.dtim_period2g_locked_to3);
    let wire = serde_json::to_value(&parsed).unwrap();
    assert_eq!(wire["channel2gLockedTo6"], true);
    assert_eq!(wire["dtimPeriod2gLockedTo3"], true);
    let request: WifiBroadcastCreateUpdate = serde_json::from_value(json!({
        "name": "IoT", "type": "STANDARD", "enabled": true,
        "channel2gLockedTo6": true, "dtimPeriod2gLockedTo3": true
    }))
    .unwrap();
    assert_eq!(
        serde_json::to_value(request).unwrap()["dtimPeriod2gLockedTo3"],
        true
    );
    let older: WifiBroadcastDetailsResponse = serde_json::from_value(json!({"id": Uuid::nil(),
        "name": "Old", "type": "STANDARD", "enabled": true, "metadata": {}, "securityConfiguration": {}})).unwrap();
    assert!(!older.channel2g_locked_to6 && !older.dtim_period2g_locked_to3);
}
