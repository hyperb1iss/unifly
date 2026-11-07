use serde_json::{Value, json};
use unifly_api::{ControllerPlatform, SessionClient, TransportConfig};
use url::Url;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn setup(platform: ControllerPlatform) -> (MockServer, SessionClient) {
    let server = MockServer::start().await;
    let client = SessionClient::new(
        Url::parse(&server.uri()).expect("mock URL"),
        "default".into(),
        platform,
        &TransportConfig::default(),
    )
    .expect("session client");
    (server, client)
}

async fn report(server: &MockServer, route: &str, body: Value) {
    Mock::given(method("POST"))
        .and(path(route))
        .and(body_json(body))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "meta": {"rc": "ok"}, "data": [{"time": 1_700_000_000_000_i64, "bytes": 42}]
        })))
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn legacy_stats_keep_default_attributes_and_omit_absent_bounds() {
    let (server, client) = setup(ControllerPlatform::ClassicController).await;
    for (kind, attrs) in [
        (
            "site",
            json!(["bytes", "num_sta", "time", "wlan-num_sta", "lan-num_sta"]),
        ),
        (
            "device",
            json!(["bytes", "num_sta", "time", "rx_bytes", "tx_bytes"]),
        ),
        ("user", json!(["bytes", "time", "rx_bytes", "tx_bytes"])),
        (
            "gw",
            json!([
                "bytes",
                "time",
                "wan-tx_bytes",
                "wan-rx_bytes",
                "lan-rx_bytes",
                "lan-tx_bytes"
            ]),
        ),
    ] {
        report(
            &server,
            &format!("/api/s/default/stat/report/hourly.{kind}"),
            json!({"attrs": attrs}),
        )
        .await;
    }
    let reports = [
        client.get_site_stats("hourly", None, None, None).await,
        client.get_device_stats("hourly", None, None).await,
        client.get_client_stats("hourly", None, None).await,
        client.get_gateway_stats("hourly", None, None, None).await,
    ];
    for result in reports {
        assert_eq!(
            result.expect("legacy report"),
            vec![json!({"time": 1_700_000_000_000_i64, "bytes": 42})]
        );
    }
}

#[tokio::test]
async fn device_and_client_ranges_preserve_millisecond_bounds_and_mac_filters() {
    let (server, client) = setup(ControllerPlatform::UnifiOs).await;
    let macs = [
        "aa:bb:cc:dd:ee:ff".to_owned(),
        "11:22:33:44:55:66".to_owned(),
    ];
    let attrs = ["time".to_owned(), "rx_bytes".to_owned()];
    let start = Some(1_700_000_000_123);
    let end = Some(1_700_003_600_123);
    for kind in ["device", "user"] {
        report(
            &server,
            &format!("/proxy/network/api/s/default/stat/report/5minutes.{kind}"),
            json!({
                "attrs": attrs, "macs": macs, "start": start, "end": end
            }),
        )
        .await;
    }
    client
        .get_device_stats_in_range("5minutes", start, end, Some(&macs), Some(&attrs))
        .await
        .expect("bounded device report");
    client
        .get_client_stats_in_range("5minutes", start, end, Some(&macs), Some(&attrs))
        .await
        .expect("bounded client report");
}

#[tokio::test]
async fn legacy_wrappers_preserve_custom_attributes_and_filters_without_bounds() {
    let (server, client) = setup(ControllerPlatform::ClassicController).await;
    let macs = ["aa:bb:cc:dd:ee:ff".to_owned()];
    let attrs = ["time".to_owned(), "custom-attribute".to_owned()];
    for kind in ["device", "user"] {
        report(
            &server,
            &format!("/api/s/default/stat/report/monthly.{kind}"),
            json!({
                "attrs": attrs, "macs": macs
            }),
        )
        .await;
    }
    client
        .get_device_stats("monthly", Some(&macs), Some(&attrs))
        .await
        .expect("device wrapper");
    client
        .get_client_stats("monthly", Some(&macs), Some(&attrs))
        .await
        .expect("client wrapper");
}

#[tokio::test]
async fn site_and_gateway_keep_explicit_millisecond_bounds() {
    let (server, client) = setup(ControllerPlatform::ClassicController).await;
    let attrs = ["time".to_owned()];
    let start = Some(1_700_000_000_123);
    let end = Some(1_700_003_600_123);
    for kind in ["site", "gw"] {
        report(
            &server,
            &format!("/api/s/default/stat/report/daily.{kind}"),
            json!({
                "attrs": attrs, "start": start, "end": end
            }),
        )
        .await;
    }
    client
        .get_site_stats("daily", start, end, Some(&attrs))
        .await
        .expect("site bounds");
    client
        .get_gateway_stats("daily", start, end, Some(&attrs))
        .await
        .expect("gateway bounds");
}
