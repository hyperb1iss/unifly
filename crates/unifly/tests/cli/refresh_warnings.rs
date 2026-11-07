use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

use super::*;

#[tokio::test]
async fn missing_optional_collection_warns_on_stderr_without_corrupting_json() {
    let server = mock_api_key_controller().await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/proxy/network/integration/v1/sites/{MOCK_SITE_ID}/dns/policies"
        )))
        .respond_with(
            ResponseTemplate::new(404)
                .set_body_json(serde_json::json!({"message": "not supported"})),
        )
        .with_priority(1)
        .mount(&server)
        .await;
    let output = mock_cmd(&server)
        .env_remove("RUST_LOG")
        .args(["dns", "list", "-o", "json"])
        .output()
        .expect("CLI");
    assert!(output.status.success());
    let rows: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("clean JSON stdout");
    assert_eq!(rows, serde_json::json!([]));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("optional fetch failed"), "{stderr}");
    assert!(stderr.contains("dns/policies"), "{stderr}");
    assert!(stderr.contains("404"), "{stderr}");
}
