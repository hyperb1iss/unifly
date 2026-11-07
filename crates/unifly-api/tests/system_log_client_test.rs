use serde_json::{Value, json};
use unifly_api::session::system_log::SystemLogQuery;
use unifly_api::session::{ReauthCredentials, SessionAuth};
use unifly_api::{ControllerPlatform, Error, SessionClient, TransportConfig};
use url::Url;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MODERN_PATH: &str = "/proxy/network/v2/api/site/default/system-log/all";
const LEGACY_PATH: &str = "/proxy/network/api/s/default/stat/event";

async fn setup() -> (MockServer, SessionClient) {
    let server = MockServer::start().await;
    let client = SessionClient::new(
        Url::parse(&server.uri()).expect("mock server URL should be valid"),
        "default".into(),
        ControllerPlatform::UnifiOs,
        &TransportConfig::default(),
    )
    .expect("session client should initialize");
    (server, client)
}

fn query() -> SystemLogQuery {
    SystemLogQuery {
        timestamp_from: 1_700_000_000_000,
        timestamp_to: 1_700_086_400_000,
        page_number: 1,
        page_size: 1000,
    }
}

fn page() -> Value {
    json!({
        "data": [{
            "id": "event-1", "timestamp": 1_700_000_000_123_i64,
            "category": "CLIENT_DEVICES", "subcategory": "CLIENT_CONNECTION",
            "severity": "LOW", "status": "NEW", "event": "CLIENT_CONNECTED_WIRELESS",
            "key": "CLIENT_CONNECTED_WIRELESS_WIFI", "message_raw": "{CLIENT} joined {WLAN}",
            "parameters": {"CLIENT": {"name": "Laptop", "id": "aa:bb:cc:dd:ee:ff"}}
        }],
        "page_number": 1, "total_element_count": 1001, "total_page_count": 2
    })
}

#[tokio::test]
async fn system_log_posts_explicit_descending_query_and_preserves_page_metadata() {
    let (server, client) = setup().await;
    Mock::given(method("POST"))
        .and(path(MODERN_PATH))
        .and(body_json(json!({
            "timestampFrom": 1_700_000_000_000_i64, "timestampTo": 1_700_086_400_000_i64,
            "pageNumber": 1, "pageSize": 1000,
            "sortByField": "TIMESTAMP", "sortDirection": "DESCENDING"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(page()))
        .expect(1)
        .mount(&server)
        .await;
    let result = client
        .list_system_log(&query())
        .await
        .expect("system log read should succeed");
    assert_eq!(result.page_number, 1);
    assert_eq!(result.total_element_count, 1001);
    assert_eq!(result.total_page_count, 2);
    assert_eq!(result.data.len(), 1);
    let event = result
        .data
        .first()
        .expect("system log page should contain its event");
    assert_eq!(event.id.as_deref(), Some("event-1"));
    assert_eq!(event.timestamp, 1_700_000_000_123);
    assert_eq!(event.parameters["CLIENT"]["name"], "Laptop");
}

#[tokio::test]
async fn system_log_rotates_csrf_between_read_posts() {
    let (server, client) = setup().await;
    Mock::given(method("POST"))
        .and(path(MODERN_PATH))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Updated-CSRF-Token", "rotated-token")
                .set_body_json(page()),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(MODERN_PATH))
        .and(header("X-CSRF-Token", "rotated-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page()))
        .with_priority(2)
        .expect(1)
        .mount(&server)
        .await;
    client
        .list_system_log(&query())
        .await
        .expect("system log read should succeed");
    client
        .list_system_log(&query())
        .await
        .expect("system log read should succeed");
}

#[tokio::test]
async fn legacy_page_uses_post_window_offset_and_sort() {
    let (server, client) = setup().await;
    Mock::given(method("POST"))
        .and(path(LEGACY_PATH))
        .and(body_json(json!({
            "within": 48, "_start": 1000, "_limit": 1000, "_sort": "-time", "type": null
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "meta": {"rc": "ok"}, "data": [{"_id": "old-1", "key": "EVT_AP_Lost_Contact"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let events = client
        .list_events_page(48, 1000, 1000)
        .await
        .expect("legacy event page should decode");
    assert_eq!(
        events
            .first()
            .expect("legacy page should contain its event")
            .id,
        "old-1"
    );
}

#[tokio::test]
async fn malformed_modern_page_fails_instead_of_returning_partial_history() {
    let (server, client) = setup().await;
    let mut missing_metadata = page();
    missing_metadata
        .as_object_mut()
        .expect("page fixture should be an object")
        .remove("total_page_count");
    let mut malformed_record = page();
    malformed_record["data"]
        .as_array_mut()
        .expect("page data fixture should be an array")
        .push(json!({"timestamp": "bad"}));
    for body in [missing_metadata, malformed_record] {
        server.reset().await;
        Mock::given(method("POST"))
            .and(path(MODERN_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        assert!(matches!(
            client.list_system_log(&query()).await,
            Err(Error::Deserialization { .. })
        ));
    }
}

#[tokio::test]
async fn malformed_legacy_record_does_not_change_pagination_offsets() {
    let (server, client) = setup().await;
    Mock::given(method("POST"))
        .and(path(LEGACY_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "meta": {"rc": "ok"}, "data": [{"_id": "valid"}, {"_id": 12}]
        })))
        .mount(&server)
        .await;
    assert!(matches!(
        client.list_events_page(24, 0, 1000).await,
        Err(Error::Deserialization { .. })
    ));
}

#[tokio::test]
async fn read_posts_preserve_http_and_embedded_api_errors() {
    let (server, client) = setup().await;
    for response in [
        ResponseTemplate::new(403),
        ResponseTemplate::new(404),
        ResponseTemplate::new(500),
        ResponseTemplate::new(200)
            .set_body_json(json!({"error": {"code": 403, "message": "denied"}})),
    ] {
        server.reset().await;
        Mock::given(method("POST"))
            .and(path(MODERN_PATH))
            .respond_with(response)
            .mount(&server)
            .await;
        assert!(matches!(
            client.list_system_log(&query()).await,
            Err(Error::SessionApi { .. })
        ));
    }
    server.reset().await;
    Mock::given(method("POST"))
        .and(path(LEGACY_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "meta": {"rc": "error", "msg": "api.err.PermissionDenied"}, "data": []
        })))
        .mount(&server)
        .await;
    assert!(matches!(client.list_events_page(24, 0, 1000).await,
        Err(Error::SessionApi { message }) if message == "api.err.PermissionDenied"));
}

#[tokio::test]
async fn expired_session_replays_both_read_only_posts_after_login() {
    for (modern, wrapped) in [(true, false), (false, false), (true, true), (false, true)] {
        let (server, client) = setup().await;
        client
            .enable_reauth(ReauthCredentials {
                username: "admin".into(),
                password: "test-password".to_string().into(),
                totp_provider: None,
                cache: None,
            })
            .await;
        let endpoint = if modern { MODERN_PATH } else { LEGACY_PATH };
        let expired = if wrapped {
            ResponseTemplate::new(200)
                .set_body_json(json!({"error": {"code": 401, "message": "expired"}}))
        } else {
            ResponseTemplate::new(401)
        };
        Mock::given(method("POST"))
            .and(path(endpoint))
            .respond_with(expired)
            .up_to_n_times(1)
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/auth/login"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("X-CSRF-Token", "renewed-token")
                    .insert_header("Set-Cookie", "TOKEN=renewed; Path=/")
                    .set_body_json(json!({})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let body = if modern {
            page()
        } else {
            json!({"meta": {"rc": "ok"}, "data": []})
        };
        Mock::given(method("POST"))
            .and(path(endpoint))
            .and(header("X-CSRF-Token", "renewed-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .with_priority(2)
            .expect(1)
            .mount(&server)
            .await;
        if modern {
            client
                .list_system_log(&query())
                .await
                .expect("system log read should succeed");
        } else {
            client
                .list_events_page(24, 0, 1000)
                .await
                .expect("legacy read should succeed after login");
        }
        server.verify().await;
    }
}

#[tokio::test]
async fn api_key_read_post_keeps_key_header_and_classifies_401() {
    unifly_api::transport::ensure_crypto_provider();
    let server = MockServer::start().await;
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "X-API-KEY",
        "synthetic-key"
            .parse()
            .expect("synthetic API key should be a valid header"),
    );
    let client = SessionClient::with_client(
        reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .expect("API key HTTP client should build"),
        Url::parse(&server.uri()).expect("mock server URL should be valid"),
        "default".into(),
        ControllerPlatform::UnifiOs,
        SessionAuth::ApiKey,
    );
    Mock::given(method("POST"))
        .and(path(MODERN_PATH))
        .and(header("X-API-KEY", "synthetic-key"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    assert!(matches!(
        client.list_system_log(&query()).await,
        Err(Error::InvalidApiKey)
    ));
}

#[tokio::test]
async fn cached_validation_updates_csrf_before_system_log_post() {
    const ISOLATED_CACHE: &str = "UNIFLY_TEST_ISOLATED_CSRF_CACHE";
    if std::env::var_os(ISOLATED_CACHE).is_none() {
        let directory = tempfile::tempdir().expect("isolated cache directory should be created");
        let result = std::process::Command::new(
            std::env::current_exe().expect("test executable path should be available"),
        )
        .args([
            "--exact",
            "cached_validation_updates_csrf_before_system_log_post",
            "--nocapture",
        ])
        .env(ISOLATED_CACHE, "1")
        .env("XDG_CACHE_HOME", directory.path())
        .env("APPDATA", directory.path())
        .env("LOCALAPPDATA", directory.path())
        .env("USERPROFILE", directory.path())
        .output()
        .expect("isolated cache test subprocess should run");
        assert!(
            result.status.success(),
            "isolated cache check failed:\n{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }

    let (server, client) = setup().await;
    let cache = unifly_api::session::session_cache::SessionCache::new("csrf-test", &server.uri())
        .expect("isolated session cache should initialize");
    cache.save(
        "TOKEN=cached-session",
        Some("stale-token"),
        unifly_api::session::session_cache::fallback_expiry(),
    );
    Mock::given(method("GET"))
        .and(path("/proxy/network/api/s/default/self"))
        .and(header("Cookie", "TOKEN=cached-session"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-CSRF-Token", "older-token")
                .insert_header("X-Updated-CSRF-Token", "validated-token")
                .set_body_json(json!({"meta": {"rc": "ok"}, "data": []})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/auth/login"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(MODERN_PATH))
        .and(header("X-CSRF-Token", "validated-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page()))
        .expect(1)
        .mount(&server)
        .await;
    client
        .login_with_cache("admin", &"test-password".to_string().into(), None, &cache)
        .await
        .expect("cached session validation should succeed");
    client
        .list_system_log(&query())
        .await
        .expect("system log read should succeed");
}

#[tokio::test]
async fn modern_read_envelopes_preserve_exact_not_found_semantics() {
    for modern_history in [true, false] {
        for status in [200, 403, 500] {
            let (server, client) = setup().await;
            let endpoint = if modern_history {
                MODERN_PATH.to_owned()
            } else {
                "/proxy/network/v2/api/site/default/system-log/critical".to_owned()
            };
            Mock::given(method("POST"))
                .and(path(endpoint))
                .respond_with(ResponseTemplate::new(status).set_body_json(json!({
                    "meta": {"rc": "error", "msg": "api.err.NotFound"}, "data": []
                })))
                .expect(1)
                .mount(&server)
                .await;
            let error = if modern_history {
                client
                    .list_system_log(&query())
                    .await
                    .expect_err("history error envelope should fail")
            } else {
                client
                    .list_critical_alerts()
                    .await
                    .expect_err("critical alert error envelope should fail")
            };
            match error {
                Error::SessionApi { message } if status == 200 => {
                    assert_eq!(message, "api.err.NotFound");
                }
                Error::SessionApi { message } => {
                    assert!(message.starts_with(&format!("HTTP {status} ")), "{message}");
                }
                other => panic!("unexpected error: {other:?}"),
            }
        }
    }
}
