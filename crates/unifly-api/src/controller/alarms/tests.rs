use serde_json::json;
use url::Url;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::{ControllerPlatform, TransportConfig};

const CRITICAL: &str = "/v2/api/site/default/system-log/critical";
const LEGACY: &str = "/api/s/default/cmd/evtmgr";

fn client(server: &MockServer) -> SessionClient {
    SessionClient::new(
        Url::parse(&server.uri()).expect("mock URL"),
        "default".into(),
        ControllerPlatform::ClassicController,
        &TransportConfig::default(),
    )
    .expect("session client")
}

#[tokio::test]
async fn empty_modern_alerts_do_not_fall_back() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(CRITICAL))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;
    assert!(list(&client(&server)).await.expect("alerts").is_empty());
    assert_eq!(server.received_requests().await.expect("requests").len(), 1);
}

#[tokio::test]
async fn only_missing_collection_selects_legacy_alarm_reads() {
    for status in [404, 401, 403, 500] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(CRITICAL))
            .respond_with(ResponseTemplate::new(status))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/s/default/stat/alarm"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "meta": {"rc": "ok"}, "data": []
            })))
            .expect(u64::from(status == 404))
            .mount(&server)
            .await;
        assert_eq!(list(&client(&server)).await.is_ok(), status == 404);
    }
}

#[tokio::test]
async fn legacy_acknowledgement_payloads_are_preserved() {
    let server = MockServer::start().await;
    let session = client(&server);
    Mock::given(method("POST"))
        .and(path(CRITICAL))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "meta": {"rc": "error", "msg": "api.err.NotFound"}, "data": []
        })))
        .expect(3)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/s/default/stat/alarm"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "meta": {"rc": "ok"}, "data": []
        })))
        .expect(1)
        .mount(&server)
        .await;
    assert!(list(&session).await.expect("legacy list").is_empty());
    for payload in [
        json!({"cmd": "archive-alarm", "_id": "alarm-1"}),
        json!({"cmd": "archive-all-alarms"}),
    ] {
        Mock::given(method("POST"))
            .and(path(LEGACY))
            .and(body_json(payload))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "meta": {"rc": "ok"}, "data": []
            })))
            .expect(1)
            .mount(&server)
            .await;
    }
    archive(&session, "alarm-1").await.expect("legacy archive");
    archive_all(&session).await.expect("legacy archive all");
}

#[tokio::test]
async fn missing_modern_alert_never_replays_a_legacy_mutation() {
    for status in [404, 207] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(CRITICAL))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path(format!("{CRITICAL}/gone/mark-as-read")))
            .respond_with(ResponseTemplate::new(status))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(LEGACY))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        assert!(archive(&client(&server), "gone").await.is_err());
    }
}
