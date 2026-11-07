use serde_json::{Value, json};
use unifly_api::{ControllerPlatform, SessionClient, TransportConfig};
use url::Url;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SITE: &str = "11111111-1111-4111-8111-111111111111";
const START: i64 = 1_700_000_000_000;
const END: i64 = START + 3_600_000;
const PREFIX: &str = "/proxy/network/v2/api/stats";

fn millis(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .expect("fixture timestamp")
        .timestamp_millis()
}

async fn setup() -> (MockServer, SessionClient) {
    let server = MockServer::start().await;
    let client = SessionClient::new(
        Url::parse(&server.uri()).expect("mock URL"),
        "default".into(),
        ControllerPlatform::UnifiOs,
        &TransportConfig::default(),
    )
    .expect("session client");
    (server, client)
}

async fn modern(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path(format!("{PREFIX}/source")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!("MODERN")))
        .expect(1)
        .mount(server)
        .await;
    Mock::given(method("GET")).and(path("/proxy/network/api/self/sites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"meta":{"rc":"ok"},"data":[{"_id":SITE,"name":"default","external_id":"22222222-2222-4222-8222-222222222222"}]}))).mount(server).await;
}

async fn series(server: &MockServer, schema: &str, body: Value) {
    Mock::given(method("GET"))
        .and(path(format!("{PREFIX}/series")))
        .and(query_param("schema", schema))
        .and(query_param("filter", format!("siteId.eq('{SITE}')")))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

#[tokio::test]
async fn client_reports_preserve_native_buckets_identity_and_legacy_direction() {
    let (server, client) = setup().await;
    modern(&server).await;
    series(&server, "activityBySiteByClient", json!([
        {"siteId":SITE,"clientId":"02:11:22:33:44:01","series":[{"timestamp":START,"duration":160_276,"rxBytes":210_000.0,"txBytes":90_000.0,"durationMs":31_843.0}]},
        {"siteId":SITE,"clientId":"02:11:22:33:44:02","series":[{"timestamp":START,"duration":300_000,"rxBytes":4.5,"txBytes":1.5}]},
        {"siteId":"other-site","clientId":"02:11:22:33:44:03","series":[{"timestamp":START,"duration":300_000,"rxBytes":999}]}
    ])).await;
    let rows = client
        .get_client_stats_in_range("hourly", Some(START), Some(END), None, None)
        .await
        .expect("modern clients");
    assert_eq!(rows.len(), 2);
    let first = rows.first().expect("first client");
    assert_eq!(first["mac"], "02:11:22:33:44:01");
    assert_eq!(first["rx_bytes"], 90_000.0);
    assert_eq!(first["tx_bytes"], 210_000.0);
    assert_eq!(first["bytes"], 300_000.0);
    assert_eq!(first["duration"], 160_276);
    assert!(first.get("durationMs").is_none());
    let requests = server.received_requests().await.expect("recorded requests");
    let request = requests
        .iter()
        .find(|request| request.url.path().ends_with("/series"))
        .expect("series request");
    assert!(
        request
            .url
            .query_pairs()
            .any(|(key, value)| key == "bucket" && value == "OF_1_HOUR")
    );
    assert!(
        request
            .url
            .query_pairs()
            .any(|(key, value)| key == "start" && value == START.to_string())
    );
}

#[tokio::test]
async fn device_traffic_families_and_units_do_not_collapse_or_double_count() {
    let (server, client) = setup().await;
    modern(&server).await;
    series(&server, "systemBySiteByDevice", json!([
        {"siteId":SITE,"deviceMac":"02:00:00:00:00:01","unitId":null,"series":[{"timestamp":START,"duration":300_000,"avgCpuUtilizationPct":25}]},
        {"siteId":SITE,"deviceMac":"02:00:00:00:00:02","unitId":1,"series":[{"timestamp":START,"duration":300_000,"avgCpuUtilizationPct":10}]}
    ])).await;
    series(&server, "clientCountBySiteByDevice", json!([])).await;
    series(&server, "wanBySiteByDevice", json!([])).await;
    series(&server, "wirelessBySiteByDevice", json!([
        {"siteId":SITE,"deviceMac":"02:00:00:00:00:01","series":[{"timestamp":START,"duration":300_000,"rxBytes":100,"txBytes":200,"distinctClientCount":1}]}
    ])).await;
    series(&server, "portTrafficBySiteByDevice", json!([
        {"siteId":SITE,"deviceMac":"02:00:00:00:00:01","series":[{"timestamp":START,"duration":300_000,"rxBytes":100,"txBytes":200}]},
        {"siteId":SITE,"deviceMac":"02:00:00:00:00:02","series":[{"timestamp":START,"duration":300_000,"rxBytes":1000,"txBytes":2000}]}
    ])).await;
    let attrs = ["time", "bytes", "cpu", "num_sta"].map(str::to_owned);
    let rows = client
        .get_device_stats_in_range("hourly", Some(START), Some(END), None, Some(&attrs))
        .await
        .expect("modern devices");
    assert_eq!(rows.len(), 3);
    let ap = rows
        .iter()
        .find(|row| row["mac"] == "02:00:00:00:00:01")
        .expect("AP row");
    assert_eq!(ap["bytes"], 300);
    assert_eq!(ap["cpu"], 25);
    let unit = rows
        .iter()
        .find(|row| row.get("unit_id").is_some())
        .expect("unit row");
    assert_eq!(unit["unit_id"], 1);
    assert!(unit.get("bytes").is_none());
}

#[tokio::test]
async fn source_is_shared_by_concurrent_reports_and_empty_is_success() {
    let (server, client) = setup().await;
    modern(&server).await;
    series(&server, "wanBySite", json!([])).await;
    let (first, second) = tokio::join!(
        client.get_gateway_stats("hourly", Some(START), Some(END), None),
        client.get_gateway_stats("hourly", Some(START), Some(END), None)
    );
    assert_eq!(first.expect("first empty history"), Vec::<Value>::new());
    assert_eq!(second.expect("second empty history"), Vec::<Value>::new());
}

#[tokio::test]
async fn explicit_long_window_is_not_capped_and_preserves_controller_error() {
    let (server, client) = setup().await;
    modern(&server).await;
    let start = END - 90 * 86_400_000;
    Mock::given(method("GET"))
        .and(path(format!("{PREFIX}/series")))
        .and(query_param(
            "start",
            millis("2022-01-02T00:00:00Z").to_string(),
        ))
        .and(query_param(
            "end",
            millis("2024-01-02T00:00:00Z").to_string(),
        ))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(
                json!({"errorCode":400,"message":"monthly timezone alignment failed"}),
            ),
        )
        .expect(1)
        .mount(&server)
        .await;
    let error = client
        .get_gateway_stats("monthly", Some(start), Some(END), None)
        .await
        .expect_err("controller rejection");
    assert!(
        error
            .to_string()
            .contains("monthly timezone alignment failed")
    );
    let requests = server.received_requests().await.expect("recorded requests");
    let request = requests.last().expect("series request");
    assert!(!request.url.query_pairs().any(|(key, _)| key == "period"));
}

#[tokio::test]
async fn modern_source_failures_never_fall_back_to_legacy() {
    for (status, body) in [
        (403, json!({"meta":{"rc":"error","msg":"api.err.NotFound"}})),
        (500, json!({"error":"not found"})),
        (200, json!("UNKNOWN")),
        (200, json!([])),
    ] {
        let (server, client) = setup().await;
        Mock::given(path(format!("{PREFIX}/source")))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .expect(1)
            .mount(&server)
            .await;
        assert!(
            client
                .get_site_stats("hourly", None, None, None)
                .await
                .is_err()
        );
        assert_eq!(
            server
                .received_requests()
                .await
                .expect("recorded requests")
                .len(),
            1
        );
    }
}

#[tokio::test]
async fn missing_source_uses_legacy_exactly_once() {
    let (server, client) = setup().await;
    Mock::given(path(format!("{PREFIX}/source")))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/proxy/network/api/s/default/stat/report/hourly.user"))
        .and(body_json(
            json!({"attrs":["bytes","time","rx_bytes","tx_bytes"]}),
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"meta":{"rc":"ok"},"data":[]})),
        )
        .expect(2)
        .mount(&server)
        .await;
    client
        .get_client_stats("hourly", None, None)
        .await
        .expect("legacy read");
    client
        .get_client_stats("hourly", None, None)
        .await
        .expect("cached legacy read");
}

#[tokio::test]
async fn malformed_series_fail_instead_of_returning_partial_data() {
    for point in [
        json!({"timestamp":START}),
        json!({"timestamp":"wrong","duration":300_000}),
        json!({"timestamp":START,"duration":0}),
        json!({"timestamp":START,"duration":300_000,"rxBytes":"wrong"}),
    ] {
        let (server, client) = setup().await;
        modern(&server).await;
        series(&server, "activityBySiteByClient", json!([{"siteId":SITE,"clientId":"02:11:22:33:44:01","series":[{"timestamp":START,"duration":300_000,"rxBytes":1}, point]}])).await;
        assert!(
            client
                .get_client_stats_in_range("hourly", Some(START), Some(END), None, None)
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn native_client_buckets_are_not_reaggregated_and_mac_filter_is_applied() {
    let (server, client) = setup().await;
    modern(&server).await;
    series(&server, "activityBySiteByClient", json!([
        {"siteId":SITE,"clientId":"02:11:22:33:44:01","series":[{"timestamp":START,"duration":300_000,"rxBytes":1},{"timestamp":START+300_000,"duration":300_000,"rxBytes":2}]},
        {"siteId":SITE,"clientId":"02:11:22:33:44:02","series":[{"timestamp":START,"duration":300_000,"rxBytes":99}]}
    ])).await;
    let macs = ["0211.2233.4401".to_owned()];
    let attrs = ["tx_bytes".to_owned()];
    let rows = client
        .get_client_stats_in_range("daily", Some(START), Some(END), Some(&macs), Some(&attrs))
        .await
        .expect("filtered native buckets");
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row["mac"] == "02:11:22:33:44:01"
        && row.get("rx_bytes").is_none()
        && row.get("time").is_none()));
}

#[tokio::test]
async fn site_counts_retain_native_resolution_and_wan_totals() {
    let (server, client) = setup().await;
    modern(&server).await;
    series(
        &server,
        "clientCountBySite",
        json!([{ "siteId":SITE,"series":[
            {"timestamp":START,"duration":28_800_000,"distinctClientCount":2},
            {"timestamp":START+28_800_000,"duration":160_276,"distinctClientCount":3}
        ]}]),
    )
    .await;
    series(
        &server,
        "wanBySite",
        json!([{ "siteId":SITE,"series":[
            {"timestamp":START,"duration":28_800_000,"rxBytes":900,"txBytes":100}
        ]}]),
    )
    .await;
    let rows = client
        .get_site_stats("daily", Some(START), Some(START + 30 * 86_400_000), None)
        .await
        .expect("native counts");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows.first().expect("first native bucket")["num_sta"], 2);
    assert_eq!(rows.last().expect("second native bucket")["num_sta"], 3);
    assert_eq!(rows.first().expect("WAN bucket")["bytes"], 1000);
    assert_eq!(rows.last().expect("missing WAN bucket").get("bytes"), None);
    assert_eq!(
        rows.first().expect("native duration")["duration"],
        28_800_000
    );
}

#[tokio::test]
async fn failed_source_probe_is_not_cached_as_legacy() {
    let (server, client) = setup().await;
    Mock::given(path(format!("{PREFIX}/source")))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .with_priority(1)
        .expect(1)
        .mount(&server)
        .await;
    modern(&server).await;
    series(&server, "wanBySite", json!([])).await;
    assert!(
        client
            .get_gateway_stats("hourly", Some(START), Some(END), None)
            .await
            .is_err()
    );
    assert_eq!(
        client
            .get_gateway_stats("hourly", Some(START), Some(END), None)
            .await
            .expect("fresh source probe"),
        Vec::<Value>::new()
    );
}

#[tokio::test]
async fn intervals_select_supported_server_buckets_with_full_explicit_bounds() {
    let (server, client) = setup().await;
    modern(&server).await;
    for (interval, bucket) in [
        ("5minutes", "OF_5_MINUTES"),
        ("hourly", "OF_1_HOUR"),
        ("daily", "OF_8_HOURS"),
        ("monthly", "OF_1_MONTH"),
    ] {
        let (start, end) = if interval == "monthly" {
            (
                millis("2022-01-02T00:00:00Z"),
                millis("2024-01-02T00:00:00Z"),
            )
        } else {
            (START, END)
        };
        Mock::given(method("GET"))
            .and(path(format!("{PREFIX}/series")))
            .and(query_param("bucket", bucket))
            .and(query_param("start", start.to_string()))
            .and(query_param("end", end.to_string()))
            .and(query_param("filter", format!("siteId.eq('{SITE}')")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(
            client
                .get_gateway_stats(interval, Some(START), Some(END), None)
                .await
                .expect("supported explicit bucket"),
            Vec::<Value>::new()
        );
    }
    let requests = server.received_requests().await.expect("recorded queries");
    assert!(
        !requests
            .iter()
            .any(|request| request.url.query_pairs().any(|(key, _)| key == "period"))
    );
}

#[tokio::test]
async fn monthly_padding_keeps_old_native_history_and_excludes_padding_only_rows() {
    let (server, client) = setup().await;
    modern(&server).await;
    let first = millis("2025-11-01T07:00:00Z");
    let old = millis("2026-01-01T08:00:00Z");
    let recent = millis("2026-11-01T07:00:00Z");
    let duration = 31 * 86_400_000_u64;
    Mock::given(method("GET"))
        .and(path(format!("{PREFIX}/series")))
        .and(query_param("schema", "wanBySite"))
        .and(query_param("bucket", "OF_1_MONTH"))
        .and(query_param("start", millis("2024-01-02T00:00:00Z").to_string()))
        .and(query_param("end", millis("2027-01-02T00:00:00Z").to_string()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"siteId":SITE,"series":[
                {"timestamp":millis("2024-02-01T08:00:00Z"),"duration":duration,"rxBytes":999,"txBytes":999},
                {"timestamp":first,"duration":duration,"rxBytes":100,"txBytes":200},
                {"timestamp":old,"duration":duration,"rxBytes":300.5,"txBytes":400.5},
                {"timestamp":recent,"duration":526_443_348,"rxBytes":500,"txBytes":600},
                {"timestamp":millis("2026-12-01T08:00:00Z"),"duration":duration,"rxBytes":999,"txBytes":999}
            ]}
        ])))
        .expect(1)
        .mount(&server)
        .await;
    let rows = client
        .get_gateway_stats(
            "monthly",
            Some(millis("2025-11-07T00:00:00Z")),
            Some(millis("2026-11-07T00:00:00Z")),
            None,
        )
        .await
        .expect("full monthly history");
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows.iter()
            .map(|row| row["time"].clone())
            .collect::<Vec<_>>(),
        vec![json!(first), json!(old), json!(recent)]
    );
    let old_row = rows
        .iter()
        .find(|row| row["time"] == old)
        .expect("retained old bucket");
    assert_eq!(old_row["bytes"], 701.0);
    assert_eq!(old_row["duration"], duration);
    assert_eq!(
        rows.last().expect("partial monthly bucket")["duration"],
        526_443_348
    );
}
