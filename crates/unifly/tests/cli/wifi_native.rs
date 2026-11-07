use super::*;

use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test]
async fn wifi_native_binding_has_no_specific_network_id() {
    let server = mock_api_key_controller().await;
    let route = format!("/proxy/network/integration/v1/sites/{MOCK_SITE_ID}/wifi/broadcasts");
    Mock::given(method("POST"))
        .and(path(&route))
        .and(body_partial_json(json!({"network": {"type": "NATIVE"}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": MOCK_WIFI_ID, "name": "Native", "enabled": true, "type": "STANDARD",
            "securityConfiguration": {"type": "OPEN"}, "metadata": {}
        })))
        .expect(1)
        .mount(&server)
        .await;
    mock_cmd(&server)
        .args([
            "wifi",
            "create",
            "--name",
            "Native",
            "--network",
            "native",
            "--security",
            "open",
        ])
        .assert()
        .success();
    let requests = server.received_requests().await.expect("requests");
    let request = requests
        .iter()
        .find(|request| request.method == "POST" && request.url.path() == route)
        .expect("Wi-Fi create");
    assert_eq!(
        request.body_json::<serde_json::Value>().expect("body")["network"],
        json!({"type": "NATIVE"})
    );
}

#[tokio::test]
async fn wifi_invalid_network_identifier_fails_without_a_write() {
    let server = mock_api_key_controller().await;
    mock_cmd(&server)
        .args([
            "wifi",
            "create",
            "--name",
            "Invalid",
            "--network",
            "not-a-uuid",
            "--security",
            "open",
        ])
        .assert()
        .failure();
    let requests = server.received_requests().await.expect("requests");
    assert!(requests.iter().all(|request| request.method == "GET"));
}
