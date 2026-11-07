use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::Notify;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use crate::controller::ports::PortProfileUpdate;
use crate::session::SessionAuth;
use crate::{Controller, ControllerConfig, ControllerPlatform, MacAddress, SessionClient};

async fn controller(server: &MockServer) -> Controller {
    crate::transport::ensure_crypto_provider();
    let controller = Controller::new(ControllerConfig::default());
    *controller.inner.session_client.lock().await = Some(Arc::new(SessionClient::with_client(
        reqwest::Client::new(),
        Url::parse(&server.uri()).expect("URL"),
        "default".into(),
        ControllerPlatform::ClassicController,
        SessionAuth::Cookie,
    )));
    controller
}

#[tokio::test]
async fn slow_session_read_releases_shared_client_before_network_wait() {
    let server = MockServer::start().await;
    let controller = controller(&server).await;
    let started = Arc::new(Notify::new());
    let observed = Arc::clone(&started);
    Mock::given(method("GET"))
        .and(path("/api/s/default/stat/health"))
        .respond_with(move |_: &Request| {
            observed.notify_one();
            ResponseTemplate::new(200)
                .set_body_json(json!({"meta": {"rc": "ok"}, "data": []}))
                .set_delay(Duration::from_secs(30))
        })
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/s/default/stat/sysinfo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "meta": {"rc": "ok"}, "data": [{"version": "11.0.99"}]
        })))
        .mount(&server)
        .await;
    let slow_controller = controller.clone();
    let slow = tokio::spawn(async move { slow_controller.get_site_health().await });
    started.notified().await;
    let lock_available = controller.inner.session_client.try_lock().is_ok();
    let fast = tokio::time::timeout(Duration::from_secs(5), controller.get_system_info()).await;
    slow.abort();
    assert!(slow.await.expect_err("cancelled read").is_cancelled());
    assert!(
        lock_available,
        "network request held the shared client lock"
    );
    assert_eq!(
        fast.expect("independent request").expect("sysinfo").version,
        "11.0.99"
    );
}

#[tokio::test]
async fn concurrent_port_edits_preserve_both_overrides() {
    let server = MockServer::start().await;
    let controller = controller(&server).await;
    let overrides = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let read_overrides = Arc::clone(&overrides);
    Mock::given(method("POST"))
        .and(path("/api/s/default/stat/device"))
        .respond_with(move |_: &Request| {
            let current = read_overrides.lock().expect("overrides").clone();
            ResponseTemplate::new(200)
                .set_body_json(json!({
                    "meta": {"rc": "ok"}, "data": [{"_id": "switch", "port_overrides": current,
                        "port_table": [{"port_idx": 1}, {"port_idx": 2}]}]
                }))
                .set_delay(Duration::from_millis(100))
        })
        .expect(2)
        .mount(&server)
        .await;
    let written = Arc::clone(&overrides);
    Mock::given(method("PUT"))
        .and(path("/api/s/default/rest/device/switch"))
        .respond_with(move |request: &Request| {
            let body: Value = request.body_json().expect("request JSON");
            *written.lock().expect("overrides") =
                body["port_overrides"].as_array().expect("array").clone();
            ResponseTemplate::new(200).set_body_json(json!({"meta": {"rc": "ok"}, "data": []}))
        })
        .expect(2)
        .mount(&server)
        .await;
    let mac = MacAddress::try_new("00:11:22:33:44:55").expect("MAC");
    let first = PortProfileUpdate {
        name: Some("first".into()),
        ..Default::default()
    };
    let second = PortProfileUpdate {
        name: Some("second".into()),
        ..Default::default()
    };
    let (first, second) = tokio::join!(
        controller.update_device_port(&mac, 1, &first),
        controller.update_device_port(&mac, 2, &second),
    );
    first.expect("first edit");
    second.expect("second edit");
    let result = overrides.lock().expect("overrides");
    assert_eq!(result.len(), 2);
    assert!(
        result
            .iter()
            .any(|row| row["port_idx"] == 1 && row["name"] == "first")
    );
    assert!(
        result
            .iter()
            .any(|row| row["port_idx"] == 2 && row["name"] == "second")
    );
}
