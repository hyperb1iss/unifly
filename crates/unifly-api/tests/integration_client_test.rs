#![allow(clippy::unwrap_used)]
// Integration tests for `IntegrationClient` using wiremock.

use std::collections::HashMap;

use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use unifly_api::integration_types::{
    DeviceDetailsResponse, FirewallPolicyResponse, NetworkCreateUpdate, NetworkDetailsResponse,
    Page, SiteResponse,
};
use unifly_api::{ControllerPlatform, Error, IntegrationClient};

// ── Helpers ─────────────────────────────────────────────────────────

async fn setup() -> (MockServer, IntegrationClient) {
    let server = MockServer::start().await;
    // Classic = no proxy prefix, so wiremock paths start at /integration/
    let client = IntegrationClient::from_reqwest(
        &server.uri(),
        reqwest::Client::new(),
        ControllerPlatform::ClassicController,
    )
    .unwrap();
    (server, client)
}

async fn setup_cloud(host_id: &str) -> (MockServer, IntegrationClient) {
    let server = MockServer::start().await;
    let client = IntegrationClient::from_reqwest(
        &format!("{}/v1/connector/consoles/{host_id}", server.uri()),
        reqwest::Client::new(),
        ControllerPlatform::Cloud,
    )
    .unwrap();
    (server, client)
}

// ── Happy-path tests ────────────────────────────────────────────────

#[tokio::test]
async fn test_list_sites_pagination() {
    let (server, client) = setup().await;

    let site_a = Uuid::new_v4();
    let site_b = Uuid::new_v4();

    let body = json!({
        "offset": 0,
        "limit": 25,
        "count": 2,
        "totalCount": 2,
        "data": [
            { "id": site_a, "name": "Main", "internalReference": "default" },
            { "id": site_b, "name": "Remote", "internalReference": "site2" },
        ]
    });

    Mock::given(method("GET"))
        .and(path("/integration/v1/sites"))
        .and(query_param("offset", "0"))
        .and(query_param("limit", "25"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let page: Page<SiteResponse> = client.list_sites(0, 25).await.unwrap();

    assert_eq!(page.total_count, 2);
    assert_eq!(page.data.len(), 2);
    assert_eq!(page.data[0].name, "Main");
    assert_eq!(page.data[0].internal_reference, "default");
    assert_eq!(page.data[1].name, "Remote");
    assert_eq!(page.data[1].id, site_b);
}

#[tokio::test]
async fn test_get_device() {
    let (server, client) = setup().await;

    let site_id = Uuid::new_v4();
    let device_id = Uuid::new_v4();

    let body = json!({
        "id": device_id,
        "macAddress": "aa:bb:cc:dd:ee:ff",
        "ipAddress": "192.168.1.10",
        "name": "USW-Pro-24",
        "model": "USPPDUP",
        "state": "ONLINE",
        "supported": true,
        "firmwareVersion": "7.1.26",
        "firmwareUpdatable": false,
        "features": ["switching"],
        "interfaces": {},
        "serialNumber": "SN-1234",
        "shortName": "USW",
        "startupTimestamp": "2024-01-01T00:00:00Z"
    });

    Mock::given(method("GET"))
        .and(path(format!(
            "/integration/v1/sites/{site_id}/devices/{device_id}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let device: DeviceDetailsResponse = client.get_device(&site_id, &device_id).await.unwrap();

    assert_eq!(device.id, device_id);
    assert_eq!(device.mac_address, "aa:bb:cc:dd:ee:ff");
    assert_eq!(device.name, "USW-Pro-24");
    assert_eq!(device.model, "USPPDUP");
    assert_eq!(device.serial_number.as_deref(), Some("SN-1234"));
}

#[tokio::test]
async fn test_create_network() {
    let (server, client) = setup().await;

    let site_id = Uuid::new_v4();
    let net_id = Uuid::new_v4();

    let response_body = json!({
        "id": net_id,
        "name": "IoT VLAN",
        "enabled": true,
        "management": "GATEWAY",
        "vlanId": 30,
        "default": false,
        "metadata": {},
        "dhcpGuarding": null
    });

    Mock::given(method("POST"))
        .and(path(format!("/integration/v1/sites/{site_id}/networks")))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .mount(&server)
        .await;

    let req = NetworkCreateUpdate {
        name: "IoT VLAN".into(),
        enabled: true,
        management: "GATEWAY".into(),
        vlan_id: 30,
        dhcp_guarding: None,
        extra: HashMap::new(),
    };

    let resp: NetworkDetailsResponse = client.create_network(&site_id, &req).await.unwrap();

    assert_eq!(resp.id, net_id);
    assert_eq!(resp.name, "IoT VLAN");
    assert_eq!(resp.vlan_id, 30);
    assert!(!resp.default);
}

#[tokio::test]
async fn test_delete_firewall_policy() {
    let (server, client) = setup().await;

    let site_id = Uuid::new_v4();
    let policy_id = Uuid::new_v4();

    Mock::given(method("DELETE"))
        .and(path(format!(
            "/integration/v1/sites/{site_id}/firewall/policies/{policy_id}"
        )))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    client
        .delete_firewall_policy(&site_id, &policy_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_list_firewall_policies_accepts_legacy_policy_without_id() {
    let (server, client) = setup().await;

    let site_id = Uuid::new_v4();
    let policy_id = Uuid::new_v4();
    let body = json!({
        "offset": 0,
        "limit": 25,
        "count": 2,
        "totalCount": 2,
        "data": [
            {
                "name": "Legacy migrated policy",
                "enabled": true,
                "action": {"type": "DROP"},
                "loggingEnabled": true,
                "index": 10001
            },
            {
                "id": policy_id,
                "name": "Modern policy",
                "enabled": true,
                "action": {"type": "ALLOW"},
                "loggingEnabled": false
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path(format!(
            "/integration/v1/sites/{site_id}/firewall/policies"
        )))
        .and(query_param("offset", "0"))
        .and(query_param("limit", "25"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let page: Page<FirewallPolicyResponse> = client
        .list_firewall_policies(&site_id, 0, 25)
        .await
        .unwrap();

    assert_eq!(page.total_count, 2);
    assert_eq!(page.data.len(), 2);
    assert_eq!(page.data[0].id, None);
    assert_eq!(page.data[1].id, Some(policy_id));
}

#[tokio::test]
async fn test_list_firewall_policies_survives_spec_port_range_and_bad_items() {
    let (server, client) = setup().await;

    let site_id = Uuid::new_v4();
    // Policy 2 carries the `start`/`stop` range shape UniFi OS actually sends
    // (issue #26); policy 3 is malformed and must be dropped, not fatal.
    let body = json!({
        "offset": 0,
        "limit": 25,
        "count": 3,
        "totalCount": 3,
        "data": [
            {
                "id": Uuid::new_v4(),
                "name": "Allow HTTP from Trusted",
                "enabled": true,
                "action": {"type": "ALLOW"},
                "loggingEnabled": false
            },
            {
                "id": Uuid::new_v4(),
                "name": "Allow range",
                "enabled": true,
                "action": {"type": "ALLOW"},
                "loggingEnabled": false,
                "destination": {
                    "zoneId": Uuid::new_v4(),
                    "trafficFilter": {
                        "type": "NETWORK",
                        "networkFilter": {"networkIds": [], "matchOpposite": false},
                        "portFilter": {
                            "type": "PORTS",
                            "matchOpposite": false,
                            "items": [
                                {"type": "PORT_NUMBER_RANGE", "start": 8000, "stop": 9000}
                            ]
                        }
                    }
                }
            },
            {
                "id": Uuid::new_v4(),
                "enabled": "definitely-not-a-bool"
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path(format!(
            "/integration/v1/sites/{site_id}/firewall/policies"
        )))
        .and(query_param("offset", "0"))
        .and(query_param("limit", "25"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let page: Page<FirewallPolicyResponse> = client
        .list_firewall_policies(&site_id, 0, 25)
        .await
        .unwrap();

    assert_eq!(page.total_count, 3);
    assert_eq!(
        page.data.len(),
        2,
        "malformed item must be dropped without failing the page"
    );
    assert_eq!(page.data[0].name, "Allow HTTP from Trusted");
    assert_eq!(page.data[1].name, "Allow range");
}

#[tokio::test]
async fn test_get_firewall_policy_ordering_uses_zone_pair_query_params() {
    let (server, client) = setup().await;

    let site_id = Uuid::new_v4();
    let source_zone_id = Uuid::new_v4();
    let destination_zone_id = Uuid::new_v4();
    let policy_id = Uuid::new_v4();
    let body = json!({
        "orderedFirewallPolicyIds": {
            "beforeSystemDefined": [policy_id],
            "afterSystemDefined": [],
        }
    });

    Mock::given(method("GET"))
        .and(path(format!(
            "/integration/v1/sites/{site_id}/firewall/policies/ordering"
        )))
        .and(query_param(
            "sourceFirewallZoneId",
            source_zone_id.to_string(),
        ))
        .and(query_param(
            "destinationFirewallZoneId",
            destination_zone_id.to_string(),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let ordering = client
        .get_firewall_policy_ordering(&site_id, &source_zone_id, &destination_zone_id)
        .await
        .unwrap();

    assert_eq!(ordering.before_system_defined, vec![policy_id]);
    assert!(ordering.after_system_defined.is_empty());
}

#[tokio::test]
async fn test_set_firewall_policy_ordering_uses_zone_pair_query_params() {
    let (server, client) = setup().await;

    let site_id = Uuid::new_v4();
    let source_zone_id = Uuid::new_v4();
    let destination_zone_id = Uuid::new_v4();
    let policy_id = Uuid::new_v4();
    let body = json!({
        "orderedFirewallPolicyIds": {
            "beforeSystemDefined": [policy_id],
            "afterSystemDefined": [],
        }
    });

    Mock::given(method("PUT"))
        .and(path(format!(
            "/integration/v1/sites/{site_id}/firewall/policies/ordering"
        )))
        .and(query_param(
            "sourceFirewallZoneId",
            source_zone_id.to_string(),
        ))
        .and(query_param(
            "destinationFirewallZoneId",
            destination_zone_id.to_string(),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let ordering = client
        .set_firewall_policy_ordering(
            &site_id,
            &source_zone_id,
            &destination_zone_id,
            &unifly_api::integration_types::FirewallPolicyOrdering {
                before_system_defined: vec![policy_id],
                after_system_defined: Vec::new(),
            },
        )
        .await
        .unwrap();

    assert_eq!(ordering.before_system_defined, vec![policy_id]);
    assert!(ordering.after_system_defined.is_empty());
}

#[tokio::test]
async fn test_pagination_empty_page() {
    let (server, client) = setup().await;

    let body = json!({
        "offset": 0,
        "limit": 25,
        "count": 0,
        "totalCount": 0,
        "data": []
    });

    Mock::given(method("GET"))
        .and(path("/integration/v1/sites"))
        .and(query_param("offset", "0"))
        .and(query_param("limit", "25"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let page: Page<SiteResponse> = client.list_sites(0, 25).await.unwrap();

    assert_eq!(page.total_count, 0);
    assert_eq!(page.count, 0);
    assert!(page.data.is_empty());
}

#[tokio::test]
async fn test_list_pending_devices_uses_spec_path() {
    let (server, client) = setup().await;

    let site_id = Uuid::new_v4();
    let body = json!({
        "offset": 0,
        "limit": 10,
        "count": 1,
        "totalCount": 1,
        "data": [
            { "id": "pending-1", "name": "AP-42" }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/integration/v1/pending-devices"))
        .and(query_param("offset", "0"))
        .and(query_param("limit", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let page = client.list_pending_devices(&site_id, 0, 10).await.unwrap();

    assert_eq!(page.total_count, 1);
    assert_eq!(page.data.len(), 1);
    assert_eq!(
        page.data[0]
            .fields
            .get("name")
            .and_then(serde_json::Value::as_str),
        Some("AP-42")
    );
}

#[tokio::test]
async fn test_list_device_tags_uses_spec_path() {
    let (server, client) = setup().await;

    let site_id = Uuid::new_v4();
    let body = json!({
        "offset": 5,
        "limit": 25,
        "count": 1,
        "totalCount": 1,
        "data": [
            { "id": "tag-1", "name": "Core" }
        ]
    });

    Mock::given(method("GET"))
        .and(path(format!("/integration/v1/sites/{site_id}/device-tags")))
        .and(query_param("offset", "5"))
        .and(query_param("limit", "25"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let page = client.list_device_tags(&site_id, 5, 25).await.unwrap();

    assert_eq!(page.total_count, 1);
    assert_eq!(
        page.data[0]
            .fields
            .get("name")
            .and_then(serde_json::Value::as_str),
        Some("Core")
    );
}

#[tokio::test]
async fn test_list_vpn_tunnels_uses_spec_path() {
    let (server, client) = setup().await;

    let site_id = Uuid::new_v4();
    let body = json!({
        "offset": 0,
        "limit": 25,
        "count": 1,
        "totalCount": 1,
        "data": [
            { "id": "vpn-1", "name": "Branch Tunnel" }
        ]
    });

    Mock::given(method("GET"))
        .and(path(format!(
            "/integration/v1/sites/{site_id}/vpn/site-to-site-tunnels"
        )))
        .and(query_param("offset", "0"))
        .and(query_param("limit", "25"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let page = client.list_vpn_tunnels(&site_id, 0, 25).await.unwrap();

    assert_eq!(page.total_count, 1);
    assert_eq!(
        page.data[0]
            .fields
            .get("name")
            .and_then(serde_json::Value::as_str),
        Some("Branch Tunnel")
    );
}

#[tokio::test]
async fn test_list_dpi_categories_uses_global_path() {
    let (server, client) = setup().await;

    let site_id = Uuid::new_v4();
    let body = json!({
        "offset": 2,
        "limit": 25,
        "count": 1,
        "totalCount": 1,
        "data": [
            { "id": "cat-1", "name": "Streaming" }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/integration/v1/dpi/categories"))
        .and(query_param("offset", "2"))
        .and(query_param("limit", "25"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let page = client.list_dpi_categories(&site_id, 2, 25).await.unwrap();

    assert_eq!(page.total_count, 1);
    assert_eq!(
        page.data[0]
            .fields
            .get("name")
            .and_then(serde_json::Value::as_str),
        Some("Streaming")
    );
}

#[tokio::test]
async fn test_list_dpi_applications_uses_global_path() {
    let (server, client) = setup().await;

    let site_id = Uuid::new_v4();
    let body = json!({
        "offset": 3,
        "limit": 25,
        "count": 1,
        "totalCount": 1,
        "data": [
            { "id": "app-1", "name": "YouTube" }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/integration/v1/dpi/applications"))
        .and(query_param("offset", "3"))
        .and(query_param("limit", "25"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let page = client.list_dpi_applications(&site_id, 3, 25).await.unwrap();

    assert_eq!(page.total_count, 1);
    assert_eq!(
        page.data[0]
            .fields
            .get("name")
            .and_then(serde_json::Value::as_str),
        Some("YouTube")
    );
}

// ── Error tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_error_401_unauthorized() {
    let (server, client) = setup().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let result = client.list_sites(0, 25).await;

    assert!(
        matches!(result, Err(Error::InvalidApiKey)),
        "expected InvalidApiKey, got: {result:?}"
    );
}

#[tokio::test]
async fn test_cloud_connector_routes_integration_requests_through_proxy_path() {
    let host_id = "console-123";
    let (server, client) = setup_cloud(host_id).await;

    let body = json!({
        "offset": 0,
        "limit": 25,
        "count": 1,
        "totalCount": 1,
        "data": [
            { "id": Uuid::new_v4(), "name": "Main", "internalReference": "default" },
        ]
    });

    Mock::given(method("GET"))
        .and(path(format!(
            "/v1/connector/consoles/{host_id}/proxy/network/integration/v1/sites"
        )))
        .and(query_param("offset", "0"))
        .and(query_param("limit", "25"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let page: Page<SiteResponse> = client.list_sites(0, 25).await.unwrap();

    assert_eq!(page.total_count, 1);
    assert_eq!(page.data[0].name, "Main");
}

#[tokio::test]
async fn test_cloud_error_429_uses_retry_after_header() {
    let (server, client) = setup_cloud("console-123").await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "2.345s"))
        .mount(&server)
        .await;

    let result = client.list_sites(0, 25).await;

    assert!(
        matches!(
            result,
            Err(Error::RateLimited {
                retry_after_secs: 3
            })
        ),
        "expected cloud rate limit error, got: {result:?}"
    );
}

#[tokio::test]
async fn test_cloud_error_403_maps_access_denied() {
    let host_id = "console-403";
    let (server, client) = setup_cloud(host_id).await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let result = client.list_sites(0, 25).await;

    assert!(
        matches!(
            result,
            Err(Error::ConsoleAccessDenied { ref host_id }) if host_id == "console-403"
        ),
        "expected cloud access denied error, got: {result:?}"
    );
}

#[tokio::test]
async fn test_cloud_error_408_maps_console_offline() {
    let host_id = "console-408";
    let (server, client) = setup_cloud(host_id).await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(408))
        .mount(&server)
        .await;

    let result = client.list_sites(0, 25).await;

    assert!(
        matches!(
            result,
            Err(Error::ConsoleOffline { ref host_id }) if host_id == "console-408"
        ),
        "expected cloud offline error, got: {result:?}"
    );
}

#[tokio::test]
async fn test_error_404_not_found() {
    let (server, client) = setup().await;

    let site_id = Uuid::new_v4();
    let device_id = Uuid::new_v4();

    Mock::given(method("GET"))
        .and(path(format!(
            "/integration/v1/sites/{site_id}/devices/{device_id}"
        )))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({ "message": "Not found" })))
        .mount(&server)
        .await;

    let result = client.get_device(&site_id, &device_id).await;

    match result {
        Err(Error::Integration {
            status,
            ref message,
            ..
        }) => {
            assert_eq!(status, 404);
            assert_eq!(message, "Not found");
        }
        other => panic!("expected Integration error, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_error_422_validation() {
    let (server, client) = setup().await;

    let site_id = Uuid::new_v4();

    Mock::given(method("POST"))
        .and(path(format!("/integration/v1/sites/{site_id}/networks")))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({
            "message": "Invalid VLAN ID",
            "code": "VALIDATION_ERROR"
        })))
        .mount(&server)
        .await;

    let req = NetworkCreateUpdate {
        name: "Bad VLAN".into(),
        enabled: true,
        management: "GATEWAY".into(),
        vlan_id: 9999,
        dhcp_guarding: None,
        extra: HashMap::new(),
    };

    let result = client.create_network(&site_id, &req).await;

    match result {
        Err(Error::Integration {
            status,
            ref message,
            ref code,
        }) => {
            assert_eq!(status, 422);
            assert_eq!(message, "Invalid VLAN ID");
            assert_eq!(code.as_deref(), Some("VALIDATION_ERROR"));
        }
        other => panic!("expected Integration 422 error, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_error_500_server_error() {
    let (server, client) = setup().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let result = client.list_sites(0, 25).await;

    match result {
        Err(Error::Integration {
            status, ref code, ..
        }) => {
            assert_eq!(status, 500);
            assert!(code.is_none());
        }
        other => panic!("expected Integration 500 error, got: {other:?}"),
    }
}
