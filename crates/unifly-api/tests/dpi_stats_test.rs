use serde_json::json;
use unifly_api::{ControllerPlatform, SessionClient, TransportConfig};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn setup(status: u16) -> (MockServer, SessionClient) {
    let server = MockServer::start().await;
    let client = SessionClient::new(
        Url::parse(&server.uri()).expect("mock URL"),
        "default".into(),
        ControllerPlatform::UnifiOs,
        &TransportConfig::default(),
    )
    .expect("client");
    Mock::given(method("GET"))
        .and(path(
            "/proxy/network/v2/api/site/default/traffic-flow-latest-statistics",
        ))
        .respond_with(ResponseTemplate::new(status).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    (server, client)
}

#[tokio::test]
async fn dpi_does_not_mask_v2_failures_as_empty_legacy_success() {
    for status in [401, 403, 429, 500] {
        let (server, client) = setup(status).await;
        for (verb, suffix) in [("POST", "sitedpi"), ("GET", "dpi")] {
            Mock::given(method(verb))
                .and(path(format!("/proxy/network/api/s/default/stat/{suffix}")))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"meta":{"rc":"ok"},"data":[]})),
                )
                .expect(0)
                .mount(&server)
                .await;
        }
        assert!(
            client.get_dpi_stats("by_app", None).await.is_err(),
            "HTTP {status}"
        );
        server.verify().await;
    }
}

#[tokio::test]
async fn dpi_legacy_fallback_accepts_missing_or_empty_modern_endpoint() {
    for status in [200, 404] {
        let (server, client) = setup(status).await;
        let data = json!([{"by_app": [{"app": 1, "tx_bytes": 12, "rx_bytes": 34}]}]);
        Mock::given(method("POST"))
            .and(path("/proxy/network/api/s/default/stat/sitedpi"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"meta":{"rc":"ok"},"data":data})),
            )
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(
            client
                .get_dpi_stats("by_app", None)
                .await
                .expect("legacy stats"),
            data.as_array().expect("array").clone()
        );
        server.verify().await;
    }
}
