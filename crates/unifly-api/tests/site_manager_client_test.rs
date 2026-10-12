#![allow(clippy::unwrap_used)]

use serde_json::json;
use wiremock::matchers::{body_json, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

use unifly_api::site_manager_types::IspMetricInterval;
use unifly_api::{Error, SiteManagerClient};

async fn setup() -> (MockServer, SiteManagerClient) {
    unifly_api::transport::ensure_crypto_provider();
    let server = MockServer::start().await;
    let client = SiteManagerClient::from_reqwest(&server.uri(), reqwest::Client::new()).unwrap();
    (server, client)
}

#[tokio::test]
async fn test_list_hosts_paginates_with_next_token() {
    let (server, client) = setup().await;

    Mock::given(method("GET"))
        .and(path("/v1/hosts"))
        .and(query_param_is_missing("nextToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{
                "id": "host-1",
                "name": "Home Console",
                "isOwner": true,
                "reportedState": { "status": "ONLINE" }
            }],
            "nextToken": "page-2",
            "traceId": "trace-1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/v1/hosts"))
        .and(query_param("nextToken", "page-2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{
                "id": "host-2",
                "name": "Lab Console",
                "reportedState": { "status": "OFFLINE" }
            }],
            "traceId": "trace-1"
        })))
        .mount(&server)
        .await;

    let hosts = client.list_hosts().await.unwrap();
    assert_eq!(hosts.len(), 2);
    assert_eq!(hosts[0].display_name(), "Home Console");
    assert!(hosts[0].is_owner_host());
    assert_eq!(hosts[1].status(), "OFFLINE");
}

#[tokio::test]
async fn test_get_host_accepts_single_object_payload() {
    let (server, client) = setup().await;

    Mock::given(method("GET"))
        .and(path("/v1/hosts/host-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "id": "host-1",
                "name": "Home Console",
                "model": "UDM Pro Max"
            }
        })))
        .mount(&server)
        .await;

    let host = client.get_host("host-1").await.unwrap();
    assert_eq!(host.id, "host-1");
    assert_eq!(host.display_name(), "Home Console");
    assert_eq!(host.model_name(), "UDM Pro Max");
}

#[tokio::test]
async fn test_list_devices_passes_host_filter() {
    let (server, client) = setup().await;

    Mock::given(method("GET"))
        .and(path("/v1/devices"))
        .and(query_param("hostIds", "host-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{
                "id": "device-1",
                "hostId": "host-1",
                "siteId": "site-1",
                "displayName": "Core Switch",
                "model": "USW-Pro-24",
                "status": "ONLINE"
            }]
        })))
        .mount(&server)
        .await;

    let devices = client.list_devices(&["host-1".into()]).await.unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].display_name(), "Core Switch");
    assert_eq!(devices[0].status(), "ONLINE");
}

#[tokio::test]
async fn test_query_isp_metrics_posts_site_ids_and_status() {
    let (server, client) = setup().await;

    Mock::given(method("POST"))
        .and(path("/v1/isp-metrics/5m/query"))
        .and(body_json(json!({ "siteIds": ["site-1", "site-2"] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "partialSuccess",
            "data": [{
                "siteId": "site-1",
                "timestamp": "2026-04-08T12:00:00Z",
                "latencyMs": 21.5,
                "downloadMbps": 941.2,
                "uploadMbps": 38.8
            }]
        })))
        .mount(&server)
        .await;

    let page = client
        .query_isp_metrics(
            IspMetricInterval::FiveMinutes,
            &["site-1".into(), "site-2".into()],
        )
        .await
        .unwrap();

    assert_eq!(page.status.as_deref(), Some("partialSuccess"));
    assert_eq!(page.data.len(), 1);
    assert_eq!(page.data[0].timestamp_text(), "2026-04-08T12:00:00Z");
    assert_eq!(page.data[0].latency_text(), "21.5");
}

#[tokio::test]
async fn test_rate_limit_maps_retry_after_header() {
    let (server, client) = setup().await;

    Mock::given(method("GET"))
        .and(path("/v1/hosts"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "2.5")
                .set_body_json(json!({ "message": "slow down" })),
        )
        .mount(&server)
        .await;

    let result = client.list_hosts().await;
    assert!(
        matches!(
            result,
            Err(Error::RateLimited {
                retry_after_secs: 3
            })
        ),
        "expected rate limit error, got {result:?}"
    );
}
