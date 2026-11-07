use std::sync::Arc;

use secrecy::SecretString;
use serde_json::json;
use url::Url;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::session::SessionAuth;
use crate::{
    ControllerConfig, ControllerPlatform, IntegrationClient, SessionClient, TransportConfig,
};

async fn hybrid(server: &MockServer) -> Controller {
    crate::transport::ensure_crypto_provider();
    let controller = Controller::new(ControllerConfig::default());
    *controller.inner.integration_client.lock().await = Some(Arc::new(
        IntegrationClient::from_api_key(
            &server.uri(),
            &SecretString::from("test-key"),
            &TransportConfig::default(),
            ControllerPlatform::UnifiOs,
        )
        .expect("Integration client"),
    ));
    *controller.inner.session_client.lock().await = Some(Arc::new(SessionClient::with_client(
        reqwest::Client::new(),
        Url::parse(&server.uri()).expect("URL"),
        "default".into(),
        ControllerPlatform::UnifiOs,
        SessionAuth::Cookie,
    )));
    controller
}

#[tokio::test]
async fn raw_integration_reads_use_api_key_without_session_cookie_auth() {
    let server = MockServer::start().await;
    let controller = hybrid(&server).await;
    Mock::given(method("GET"))
        .and(path("/proxy/network/integration/v1/sites"))
        .and(query_param("limit", "2"))
        .and(header("X-API-KEY", "test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [{"id": "site"}]})))
        .expect(1)
        .mount(&server)
        .await;
    let result = controller
        .raw_get("/integration/v1/sites?limit=2")
        .await
        .expect("raw Integration");
    assert_eq!(result, json!({"data": [{"id": "site"}]}));

    Mock::given(method("GET"))
        .and(path("/proxy/network/api/s/default/stat/health"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"meta": {"rc": "ok"}, "data": []})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let result = controller
        .raw_get("api/s/default/stat/health")
        .await
        .expect("raw Session");
    assert_eq!(result, json!({"meta": {"rc": "ok"}, "data": []}));
    let requests = server.received_requests().await.expect("requests");
    assert!(
        requests
            .last()
            .expect("session request")
            .headers
            .get("X-API-KEY")
            .is_none()
    );
}

#[tokio::test]
async fn raw_integration_writes_use_key_and_accept_empty_success() {
    let server = MockServer::start().await;
    let controller = hybrid(&server).await;
    let payload = json!({"enabled": false});
    for verb in ["POST", "PUT", "PATCH", "DELETE"] {
        let mut mock = Mock::given(method(verb))
            .and(path("/proxy/network/integration/v1/example"))
            .and(header("X-API-KEY", "test-key"));
        if verb != "DELETE" {
            mock = mock.and(body_json(&payload));
        }
        mock.respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
    }
    assert_eq!(
        controller
            .raw_post("integration/v1/example", &payload)
            .await
            .expect("post"),
        Value::Null
    );
    assert_eq!(
        controller
            .raw_put("integration/v1/example", &payload)
            .await
            .expect("put"),
        Value::Null
    );
    assert_eq!(
        controller
            .raw_patch("integration/v1/example", &payload)
            .await
            .expect("patch"),
        Value::Null
    );
    controller
        .raw_delete("integration/v1/example")
        .await
        .expect("delete");
}

#[tokio::test]
async fn failed_integration_mutation_is_never_replayed_through_session() {
    let server = MockServer::start().await;
    let controller = hybrid(&server).await;
    Mock::given(method("POST"))
        .and(path("/proxy/network/integration/v1/example"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    let error = controller
        .raw_post("integration/v1/example", &json!({}))
        .await
        .expect_err("invalid key");
    assert!(error.to_string().contains("Invalid API key"));
    assert_eq!(server.received_requests().await.expect("requests").len(), 1);
}
