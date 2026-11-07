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
