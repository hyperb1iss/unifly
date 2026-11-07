use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, ResponseTemplate};

use super::*;

#[tokio::test]
async fn site_creation_sends_display_name_without_an_internal_reference() {
    for flag in ["--name", "--description"] {
        let server = mock_api_key_controller().await;
        Mock::given(method("POST"))
            .and(path("/proxy/network/api/s/default/cmd/sitemgr"))
            .and(body_json(
                serde_json::json!({"cmd": "add-site", "desc": "Branch Office"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "meta": {"rc": "ok"}, "data": [{"name": "generated", "desc": "Branch Office"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let output = mock_cmd(&server)
            .args(["sites", "create", flag, "Branch Office"])
            .output()
            .expect("CLI");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("Site created"));
    }
}

#[tokio::test]
async fn site_delete_resolves_internal_reference_to_session_id() {
    let session_id = "00000000-0000-0000-0000-000000000012";
    for identifier in ["generated", session_id] {
        let server = mock_api_key_controller().await;
        Mock::given(method("GET"))
            .and(path("/proxy/network/api/self/sites"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "meta": {"rc": "ok"}, "data": [{"_id": session_id,
                    "name": "generated", "desc": "Branch Office"}]
            })))
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/proxy/network/api/s/default/cmd/sitemgr"))
            .and(body_json(
                serde_json::json!({"cmd": "delete-site", "site": session_id}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "meta": {"rc": "ok"}, "data": []
            })))
            .expect(1)
            .mount(&server)
            .await;
        mock_cmd(&server)
            .args(["--yes", "sites", "delete", identifier])
            .assert()
            .success();
        server.verify().await;
    }
}

#[tokio::test]
async fn site_delete_rejects_integration_id_without_posting() {
    let server = mock_api_key_controller().await;
    Mock::given(method("GET"))
        .and(path("/proxy/network/api/self/sites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "meta": {"rc": "ok"}, "data": [{"_id": "session-id", "name": "generated",
                "external_id": MOCK_SITE_ID, "desc": "Branch Office"}]
        })))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/proxy/network/api/s/default/cmd/sitemgr"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    mock_cmd(&server)
        .args(["sites", "delete", MOCK_SITE_ID])
        .assert()
        .failure()
        .stderr(predicate::str::contains("internal reference or Session ID"));
    server.verify().await;
}
