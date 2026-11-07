#![allow(clippy::unwrap_used)]

use serde_json::json;
use url::Url;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use unifly_api::{ControllerPlatform, Error, SessionClient, session::SessionAuth};

const CRITICAL_PATH: &str = "/v2/api/site/default/system-log/critical";

async fn setup(platform: ControllerPlatform) -> (MockServer, SessionClient) {
    unifly_api::transport::ensure_crypto_provider();
    let server = MockServer::start().await;
    let client = SessionClient::with_client(
        reqwest::Client::new(),
        Url::parse(&server.uri()).unwrap(),
        "default".into(),
        platform,
        SessionAuth::Cookie,
    );
    (server, client)
}

#[tokio::test]
async fn critical_alerts_accept_empty_array_without_legacy_query() {
    let (server, client) = setup(ControllerPlatform::ClassicController).await;
    Mock::given(method("POST"))
        .and(path(CRITICAL_PATH))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;

    assert!(client.list_critical_alerts().await.unwrap().is_empty());
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn critical_alerts_preserve_modern_record_fields_and_status() {
    let (server, client) = setup(ControllerPlatform::UnifiOs).await;
    Mock::given(method("POST"))
        .and(path(format!("/proxy/network{CRITICAL_PATH}")))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": "672a43a233ca0c08eec56c12",
            "timestamp": 1_793_980_800_000_i64,
            "category": "DEVICES",
            "subcategory": "DEVICE_STATUS",
            "severity": "HIGH",
            "status": "NEW",
            "key": "DEVICE_DISCONNECTED",
            "event": "DEVICE_DISCONNECTED",
            "message": "Lab switch disconnected",
            "message_raw": "{DEVICE} disconnected",
            "title_raw": "Device disconnected",
            "parameters": {"DEVICE": {"name": "Lab switch", "value": "aa:bb:cc:dd:ee:ff"}},
            "show_on_dashboard": true,
            "replace_alerts": []
        }])))
        .mount(&server)
        .await;

    let alerts = client.list_critical_alerts().await.unwrap();
    assert_eq!(alerts.len(), 1);
    let alert = alerts.first().unwrap();
    assert_eq!(alert.id.as_deref(), Some("672a43a233ca0c08eec56c12"));
    assert_eq!(alert.timestamp, 1_793_980_800_000);
    assert_eq!(alert.status.as_deref(), Some("NEW"));
    assert_eq!(alert.category.as_deref(), Some("DEVICES"));
    assert_eq!(alert.severity.as_deref(), Some("HIGH"));
    assert_eq!(alert.message.as_deref(), Some("Lab switch disconnected"));
    assert_eq!(
        alert.parameters.get("DEVICE").unwrap()["name"],
        "Lab switch"
    );
}

#[tokio::test]
async fn critical_alerts_reject_malformed_success_responses() {
    for body in [json!({"data": []}), json!([{"id": "missing-timestamp"}])] {
        let (server, client) = setup(ControllerPlatform::ClassicController).await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        assert!(matches!(
            client.list_critical_alerts().await,
            Err(Error::Deserialization { .. })
        ));
    }
}

#[tokio::test]
async fn critical_alerts_propagate_http_errors_without_fallback() {
    for status in [401, 403, 404, 429, 500] {
        let (server, client) = setup(ControllerPlatform::ClassicController).await;
        Mock::given(method("POST"))
            .and(path(CRITICAL_PATH))
            .respond_with(ResponseTemplate::new(status))
            .expect(1)
            .mount(&server)
            .await;
        let error = client.list_critical_alerts().await.unwrap_err();
        assert_eq!(error.is_not_found(), status == 404);
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn acknowledge_critical_alert_encodes_opaque_id_as_one_segment() {
    let (server, client) = setup(ControllerPlatform::UnifiOs).await;
    let expected_path =
        format!("/proxy/network{CRITICAL_PATH}/..%2Falert%2F%3F%23%25%20value/mark-as-read");
    Mock::given(method("PUT"))
        .and(path(expected_path.clone()))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client
        .acknowledge_critical_alert("../alert/?#% value")
        .await
        .unwrap();
    let requests = server.received_requests().await.unwrap();
    let request = requests.first().unwrap();
    assert_eq!(request.url.path(), expected_path);
    assert!(request.url.query().is_none());
    assert!(request.url.fragment().is_none());
}

#[tokio::test]
async fn acknowledge_critical_alert_rejects_empty_and_dot_ids_without_requests() {
    let (server, client) = setup(ControllerPlatform::ClassicController).await;
    for id in ["", ".", ".."] {
        assert!(matches!(
            client.acknowledge_critical_alert(id).await,
            Err(Error::SessionApi { .. })
        ));
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn acknowledge_all_critical_alerts_accepts_no_content() {
    let (server, client) = setup(ControllerPlatform::ClassicController).await;
    Mock::given(method("PUT"))
        .and(path(format!("{CRITICAL_PATH}/mark-all-as-read")))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client.acknowledge_all_critical_alerts().await.unwrap();
}

#[tokio::test]
async fn acknowledgement_errors_are_not_replayed_or_sent_to_legacy_endpoints() {
    for status in [401, 403, 404, 500] {
        let (server, client) = setup(ControllerPlatform::ClassicController).await;
        Mock::given(method("PUT"))
            .and(path(format!("{CRITICAL_PATH}/alert-1/mark-as-read")))
            .respond_with(ResponseTemplate::new(status))
            .expect(1)
            .mount(&server)
            .await;

        assert!(client.acknowledge_critical_alert("alert-1").await.is_err());
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn error_envelopes_are_not_successful_reads_or_acknowledgements() {
    for body in [
        json!({"error": {"code": 403, "message": "No permission"}}),
        json!({"meta": {"rc": "error", "msg": "api.err.NoPermission"}, "data": []}),
    ] {
        let (server, client) = setup(ControllerPlatform::ClassicController).await;
        Mock::given(path(CRITICAL_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(body.clone()))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        assert!(client.list_critical_alerts().await.is_err());
        assert!(client.acknowledge_critical_alert("alert-1").await.is_err());
        assert!(client.acknowledge_all_critical_alerts().await.is_err());
    }
}

#[tokio::test]
async fn multi_status_acknowledgement_is_reported_as_partial_without_replay() {
    for all in [false, true] {
        let (server, client) = setup(ControllerPlatform::ClassicController).await;
        client
            .enable_reauth(unifly_api::session::ReauthCredentials {
                username: "admin".into(),
                password: "test-password".to_string().into(),
                totp_provider: None,
                cache: None,
            })
            .await;
        let suffix = if all {
            "mark-all-as-read"
        } else {
            "alert-1/mark-as-read"
        };
        Mock::given(method("PUT"))
            .and(path(format!("{CRITICAL_PATH}/{suffix}")))
            .respond_with(ResponseTemplate::new(207))
            .expect(1)
            .mount(&server)
            .await;
        let error = if all {
            client.acknowledge_all_critical_alerts().await.unwrap_err()
        } else {
            client
                .acknowledge_critical_alert("alert-1")
                .await
                .unwrap_err()
        };
        assert!(matches!(error, Error::SessionApi { message }
            if message.starts_with("HTTP 207 ") && message.contains("partial")));
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}
