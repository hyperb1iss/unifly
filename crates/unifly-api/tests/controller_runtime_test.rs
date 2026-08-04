#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue};
use secrecy::SecretString;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, Notify};
use tokio::time::timeout;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use unifly_api::model::DnsPolicyType;
use unifly_api::{
    AuthCredentials, Controller, ControllerConfig, ControllerPlatform, CoreError, Error,
    MacAddress, SessionClient, TlsVerification, TransportConfig,
};

const LEGACY_SITE_ID: &str = "site-001";
const LEGACY_SITE_NAME: &str = "default";
const LEGACY_SITE_LABEL: &str = "Main Site";
const API_KEY_SITE_ID: &str = "550e8400-e29b-41d4-a716-446655440000";

fn base_config(
    url: Url,
    auth: AuthCredentials,
    site: &str,
    websocket_enabled: bool,
) -> ControllerConfig {
    ControllerConfig {
        url,
        auth,
        site: site.to_owned(),
        tls: TlsVerification::DangerAcceptInvalid,
        timeout: Duration::from_secs(5),
        refresh_interval_secs: 0,
        websocket_enabled,
        polling_interval_secs: 1,
        totp_token: None,
        profile_name: None,
        no_session_cache: true,
    }
}

fn secret(value: &str) -> SecretString {
    SecretString::from(value.to_owned())
}

fn empty_legacy_envelope() -> serde_json::Value {
    legacy_envelope(&json!([]))
}

fn legacy_envelope(data: &serde_json::Value) -> serde_json::Value {
    json!({
        "meta": { "rc": "ok" },
        "data": data,
    })
}

fn legacy_site_envelope() -> serde_json::Value {
    json!({
        "meta": { "rc": "ok" },
        "data": [{
            "_id": LEGACY_SITE_ID,
            "name": LEGACY_SITE_NAME,
            "desc": LEGACY_SITE_LABEL,
        }],
    })
}

fn empty_integration_page(limit: i32) -> serde_json::Value {
    json!({
        "offset": 0,
        "limit": limit,
        "count": 0,
        "totalCount": 0,
        "data": [],
    })
}

async fn mock_legacy_connect(server: &MockServer, site_envelope: serde_json::Value) {
    mock_legacy_connect_with_events(server, site_envelope, empty_legacy_envelope()).await;
}

async fn mock_legacy_connect_with_events(
    server: &MockServer,
    site_envelope: serde_json::Value,
    event_envelope: serde_json::Value,
) {
    Mock::given(method("GET"))
        .and(path("/api/login"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/auth/login"))
        .respond_with(ResponseTemplate::new(404))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/login"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Set-Cookie", "unifly_session=session-cookie; Path=/")
                .set_body_json(json!({})),
        )
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/logout"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(server)
        .await;

    for route in ["/api/s/default/stat/device", "/api/s/default/stat/sta"] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(empty_legacy_envelope()))
            .mount(server)
            .await;
    }

    Mock::given(method("GET"))
        .and(path("/api/s/default/stat/event"))
        .respond_with(ResponseTemplate::new(200).set_body_json(event_envelope))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/self/sites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(site_envelope))
        .mount(server)
        .await;
}

fn api_key_legacy_client(base_url: Url, site: &str, api_key: &str) -> SessionClient {
    let mut headers = HeaderMap::new();
    let mut key_value = HeaderValue::from_str(api_key).unwrap();
    key_value.set_sensitive(true);
    headers.insert("X-API-KEY", key_value);

    let http = TransportConfig::default()
        .build_client_with_headers(headers)
        .unwrap();

    SessionClient::with_client(
        http,
        base_url,
        site.to_owned(),
        ControllerPlatform::ClassicController,
        unifly_api::session::client::SessionAuth::ApiKey,
    )
}

fn session_legacy_client(base_url: Url, site: &str) -> SessionClient {
    SessionClient::new(
        base_url,
        site.to_owned(),
        ControllerPlatform::ClassicController,
        &TransportConfig::default(),
    )
    .unwrap()
}

async fn mount_api_key_integration_routes(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/proxy/network/integration/v1/sites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "offset": 0,
            "limit": 50,
            "count": 1,
            "totalCount": 1,
            "data": [{
                "id": API_KEY_SITE_ID,
                "internalReference": LEGACY_SITE_NAME,
                "name": LEGACY_SITE_LABEL,
            }],
        })))
        .mount(server)
        .await;

    for (route, body) in [
        (
            format!("/proxy/network/integration/v1/sites/{API_KEY_SITE_ID}/devices"),
            empty_integration_page(200),
        ),
        (
            format!("/proxy/network/integration/v1/sites/{API_KEY_SITE_ID}/clients"),
            json!({
                "offset": 0,
                "limit": 200,
                "count": 1,
                "totalCount": 1,
                "data": [{
                    "id": "c56a4180-65aa-42ec-a945-5fd21dec0538",
                    "name": "Office Laptop",
                    "type": "WIRELESS",
                    "ipAddress": "10.0.0.50",
                    "connectedAt": null,
                    "macAddress": "aa:bb:cc:dd:ee:ff",
                    "access": {},
                }],
            }),
        ),
        (
            format!("/proxy/network/integration/v1/sites/{API_KEY_SITE_ID}/networks"),
            empty_integration_page(200),
        ),
        (
            format!("/proxy/network/integration/v1/sites/{API_KEY_SITE_ID}/wifi/broadcasts"),
            empty_integration_page(200),
        ),
        (
            format!("/proxy/network/integration/v1/sites/{API_KEY_SITE_ID}/firewall/policies"),
            empty_integration_page(200),
        ),
        (
            format!("/proxy/network/integration/v1/sites/{API_KEY_SITE_ID}/firewall/zones"),
            empty_integration_page(200),
        ),
        (
            format!("/proxy/network/integration/v1/sites/{API_KEY_SITE_ID}/acl-rules"),
            empty_integration_page(200),
        ),
        (
            format!("/proxy/network/integration/v1/sites/{API_KEY_SITE_ID}/dns/policies"),
            dns_policies_page(),
        ),
        (
            format!("/proxy/network/integration/v1/sites/{API_KEY_SITE_ID}/vouchers"),
            empty_integration_page(200),
        ),
        (
            format!("/proxy/network/integration/v1/sites/{API_KEY_SITE_ID}/traffic-matching-lists"),
            empty_integration_page(200),
        ),
    ] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }
}

fn dns_policies_page() -> serde_json::Value {
    json!({
        "offset": 0,
        "limit": 200,
        "count": 2,
        "totalCount": 2,
        "data": [
            {
                "type": "A_RECORD",
                "id": "0ce73be5-f734-4b24-aac2-d91dbe88202d",
                "enabled": true,
                "metadata": {"origin": "USER_DEFINED"},
                "domain": "unifi.example.net",
                "ipv4Address": "10.0.1.1",
                "ttlSeconds": 0,
            },
            {
                "type": "FORWARD_DOMAIN",
                "id": "1df84cf6-0845-4c35-bbd3-e02ecf99313e",
                "enabled": true,
                "metadata": {"origin": "USER_DEFINED"},
                "domain": "corp.example.net",
                "ipAddress": "10.0.9.53",
            },
        ],
    })
}

fn api_key_event_envelope() -> serde_json::Value {
    legacy_envelope(&json!([{
        "_id": "evt-api-key-1",
        "key": "EVT_GW_Connected",
        "msg": "Gateway connected",
        "datetime": "2025-01-01T00:00:00Z",
        "subsystem": "device",
        "site_id": LEGACY_SITE_ID,
    }]))
}

async fn mount_api_key_legacy_routes_with_event_response(
    server: &MockServer,
    event_response: ResponseTemplate,
) {
    use wiremock::matchers::header;

    Mock::given(method("GET"))
        .and(path("/proxy/network/api/s/default/stat/device"))
        .and(header("X-API-KEY", "the-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_legacy_envelope()))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/proxy/network/api/s/default/stat/sta"))
        .and(header("X-API-KEY", "the-key"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(legacy_envelope(&json!([{
                "_id": "client-1",
                "mac": "aa:bb:cc:dd:ee:ff",
                "ip": "10.0.0.50",
                "hostname": "test-host",
                "is_wired": false,
                "tx_bytes": 1234,
                "rx_bytes": 5678,
            }]))),
        )
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/proxy/network/api/s/default/stat/event"))
        .and(header("X-API-KEY", "the-key"))
        .respond_with(event_response)
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/proxy/network/api/s/default/stat/health"))
        .and(header("X-API-KEY", "the-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_legacy_envelope()))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/proxy/network/api/s/default/rest/user"))
        .and(header("X-API-KEY", "the-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_legacy_envelope()))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/proxy/network/v2/api/site/default/nat"))
        .and(header("X-API-KEY", "the-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(server)
        .await;
}

async fn mock_api_key_with_legacy(server: &MockServer) {
    mock_api_key_with_legacy_event_response(
        server,
        ResponseTemplate::new(200).set_body_json(api_key_event_envelope()),
    )
    .await;
}

async fn mock_api_key_with_legacy_event_response(
    server: &MockServer,
    event_response: ResponseTemplate,
) {
    Mock::given(method("GET"))
        .and(path("/api/auth/login"))
        .respond_with(ResponseTemplate::new(401))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/login"))
        .respond_with(ResponseTemplate::new(404))
        .mount(server)
        .await;

    mount_api_key_integration_routes(server).await;
    mount_api_key_legacy_routes_with_event_response(server, event_response).await;
}

#[tokio::test]
async fn legacy_only_site_listing_remains_available() {
    let server = MockServer::start().await;
    mock_legacy_connect(&server, legacy_site_envelope()).await;

    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::Credentials {
            username: "admin".into(),
            password: secret("password"),
        },
        LEGACY_SITE_NAME,
        false,
    ));

    controller.connect().await.unwrap();

    assert!(controller.has_session_access().await);
    assert!(!controller.has_integration_access().await);
    assert!(controller.take_warnings().await.is_empty());

    let sites = controller.sites_snapshot();
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].internal_name, LEGACY_SITE_NAME);
    assert_eq!(sites[0].name, LEGACY_SITE_LABEL);

    controller.disconnect().await;
}

#[tokio::test]
async fn legacy_mode_rejects_integration_only_surfaces_clearly() {
    let server = MockServer::start().await;
    mock_legacy_connect(&server, empty_legacy_envelope()).await;

    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::Credentials {
            username: "admin".into(),
            password: secret("password"),
        },
        LEGACY_SITE_NAME,
        false,
    ));

    controller.connect().await.unwrap();

    let err = controller.list_countries().await.unwrap_err();
    match err {
        CoreError::Unsupported {
            operation,
            required,
        } => {
            assert_eq!(operation, "list_countries");
            assert_eq!(required, "Integration API");
        }
        other => panic!("expected Unsupported error, got {other:?}"),
    }

    controller.disconnect().await;
}

#[tokio::test]
async fn api_key_mode_has_legacy_and_integration_access() {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;

    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("the-key")),
        LEGACY_SITE_NAME,
        false,
    ));

    controller.connect().await.unwrap();

    assert!(controller.has_session_access().await);
    assert!(!controller.has_live_event_access().await);
    assert!(controller.has_integration_access().await);
    assert_eq!(controller.store().client_count(), 1);
    let events = controller.events_snapshot();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "EVT_GW_Connected");
    assert_eq!(events[0].message, "Gateway connected");

    let client = controller
        .store()
        .client_by_mac(&MacAddress::new("aa:bb:cc:dd:ee:ff"))
        .expect("expected merged client in store");
    assert_eq!(client.hostname.as_deref(), Some("test-host"));
    assert_eq!(client.tx_bytes, Some(1234));
    assert_eq!(client.rx_bytes, Some(5678));

    let dns = controller.dns_policies_snapshot();
    assert_eq!(dns.len(), 2);
    let a_record = dns
        .iter()
        .find(|d| d.domain == "unifi.example.net")
        .expect("A record present");
    assert_eq!(a_record.policy_type, DnsPolicyType::ARecord);
    assert_eq!(a_record.value, "10.0.1.1");
    assert_eq!(a_record.ttl_seconds, Some(0));
    let forward = dns
        .iter()
        .find(|d| d.domain == "corp.example.net")
        .expect("forward domain present");
    assert_eq!(forward.policy_type, DnsPolicyType::ForwardDomain);
    assert_eq!(forward.value, "10.0.9.53");

    let received = server.received_requests().await.unwrap();
    let legacy_reqs: Vec<_> = received
        .iter()
        .filter(|req| {
            req.url.path().contains("/api/s/default/")
                || req.url.path().contains("/v2/api/site/default/")
        })
        .collect();
    assert!(
        !legacy_reqs.is_empty(),
        "expected api-key mode to exercise session HTTP routes"
    );
    for req in legacy_reqs {
        assert_eq!(
            req.headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok()),
            Some("the-key"),
            "session request {} missing X-API-KEY",
            req.url.path()
        );
        assert!(
            req.headers.get("cookie").is_none(),
            "session request {} should not send Cookie in api-key mode",
            req.url.path()
        );
        assert!(
            req.headers.get("x-csrf-token").is_none(),
            "session request {} should not send X-CSRF-Token in api-key mode",
            req.url.path()
        );
    }

    controller.disconnect().await;
}

#[tokio::test]
async fn api_key_mode_treats_missing_event_endpoint_as_empty() {
    let server = MockServer::start().await;
    mock_api_key_with_legacy_event_response(&server, ResponseTemplate::new(404)).await;

    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("the-key")),
        LEGACY_SITE_NAME,
        false,
    ));

    controller.connect().await.unwrap();

    assert!(controller.has_session_access().await);
    assert!(!controller.has_live_event_access().await);
    assert!(controller.has_integration_access().await);
    assert_eq!(controller.events_snapshot().len(), 0);
    assert_eq!(controller.store().client_count(), 1);

    controller.disconnect().await;
}

#[tokio::test]
async fn api_key_legacy_raw_mutations_skip_csrf_header() {
    use wiremock::matchers::header;

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/s/default/cmd/devmgr"))
        .and(header("X-API-KEY", "the-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .mount(&server)
        .await;

    Mock::given(method("PUT"))
        .and(path("/api/s/default/rest/user/user-1"))
        .and(header("X-API-KEY", "the-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .mount(&server)
        .await;

    Mock::given(method("DELETE"))
        .and(path("/v2/api/site/default/nat/rule-1"))
        .and(header("X-API-KEY", "the-key"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let session = api_key_legacy_client(
        Url::parse(&server.uri()).unwrap(),
        LEGACY_SITE_NAME,
        "the-key",
    );

    session
        .raw_post(
            "api/s/default/cmd/devmgr",
            &json!({"cmd": "restart", "mac": "aa:bb:cc:dd:ee:ff"}),
        )
        .await
        .unwrap();
    session
        .raw_put(
            "api/s/default/rest/user/user-1",
            &json!({"use_fixedip": true, "fixed_ip": "10.0.0.60"}),
        )
        .await
        .unwrap();
    session
        .raw_delete("v2/api/site/default/nat/rule-1")
        .await
        .unwrap();

    let received = server.received_requests().await.unwrap();
    let mutated: Vec<_> = received
        .iter()
        .filter(|req| {
            matches!(req.method.as_str(), "POST" | "PUT" | "DELETE")
                && (req.url.path().starts_with("/api/s/default/")
                    || req.url.path().starts_with("/v2/api/site/default/"))
        })
        .collect();
    assert_eq!(mutated.len(), 3);

    for req in mutated {
        assert_eq!(
            req.headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok()),
            Some("the-key"),
            "mutation {} {} missing X-API-KEY",
            req.method,
            req.url.path()
        );
        assert!(
            req.headers.get("cookie").is_none(),
            "mutation {} {} should not send Cookie in api-key mode",
            req.method,
            req.url.path()
        );
        assert!(
            req.headers.get("x-csrf-token").is_none(),
            "mutation {} {} should not send X-CSRF-Token in api-key mode",
            req.method,
            req.url.path()
        );
    }
}

#[tokio::test]
async fn legacy_401_without_cookie_jar_reports_invalid_api_key() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/s/default/stat/device"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "meta": { "rc": "error", "msg": "api.err.NoPermission" },
            "data": [],
        })))
        .mount(&server)
        .await;

    let session = api_key_legacy_client(
        Url::parse(&server.uri()).unwrap(),
        LEGACY_SITE_NAME,
        "the-key",
    );

    let listed = session.list_devices().await;
    assert!(
        matches!(listed, Err(Error::InvalidApiKey)),
        "expected InvalidApiKey for envelope request, got: {listed:?}"
    );

    let raw = session.raw_get("api/s/default/stat/device").await;
    assert!(
        matches!(raw, Err(Error::InvalidApiKey)),
        "expected InvalidApiKey for raw request, got: {raw:?}"
    );
}

#[tokio::test]
async fn legacy_401_with_cookie_jar_reports_session_expired() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/s/default/stat/device"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "meta": { "rc": "error", "msg": "api.err.NoPermission" },
            "data": [],
        })))
        .mount(&server)
        .await;

    let session = session_legacy_client(Url::parse(&server.uri()).unwrap(), LEGACY_SITE_NAME);

    let listed = session.list_devices().await;
    assert!(
        matches!(listed, Err(Error::SessionExpired)),
        "expected SessionExpired for envelope request, got: {listed:?}"
    );

    let raw = session.raw_get("api/s/default/stat/device").await;
    assert!(
        matches!(raw, Err(Error::SessionExpired)),
        "expected SessionExpired for raw request, got: {raw:?}"
    );
}

#[tokio::test]
async fn websocket_enabled_for_events_watch_path() {
    let server = spawn_ws_probe_server().await;
    let controller = Controller::new(base_config(
        server.base_url.clone(),
        AuthCredentials::Credentials {
            username: "admin".into(),
            password: secret("password"),
        },
        LEGACY_SITE_NAME,
        true,
    ));

    controller.connect().await.unwrap();

    timeout(Duration::from_secs(5), server.probe.notified())
        .await
        .expect("websocket handshake did not arrive in time");

    let path = server.handshake_path.lock().await.clone().unwrap();
    let cookie = server.cookie_header.lock().await.clone().unwrap();

    assert_eq!(path, "/wss/s/default/events");
    assert!(
        cookie.contains("unifly_session=session-cookie"),
        "expected websocket cookie header to carry the session cookie, got: {cookie}"
    );

    controller.disconnect().await;
}

#[tokio::test]
async fn full_refresh_does_not_rebroadcast_duplicate_legacy_events() {
    let server = MockServer::start().await;
    mock_legacy_connect_with_events(
        &server,
        legacy_site_envelope(),
        legacy_envelope(&json!([{
            "_id": "evt-1",
            "key": "EVT_TEST",
            "msg": "Switch lost contact",
            "datetime": "2025-01-01T00:00:00Z",
            "subsystem": "device",
            "site_id": LEGACY_SITE_ID,
        }])),
    )
    .await;

    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::Credentials {
            username: "admin".into(),
            password: secret("password"),
        },
        LEGACY_SITE_NAME,
        false,
    ));
    let mut events = controller.events();

    controller.connect().await.unwrap();

    let first_event = timeout(Duration::from_secs(1), events.recv())
        .await
        .expect("initial refresh should broadcast session events")
        .expect("broadcast channel should stay open");
    assert_eq!(first_event.message, "Switch lost contact");
    assert_eq!(controller.events_snapshot().len(), 1);

    controller.full_refresh().await.unwrap();

    assert!(
        timeout(Duration::from_millis(250), events.recv())
            .await
            .is_err(),
        "refresh should not rebroadcast already-seen session events"
    );
    assert_eq!(controller.events_snapshot().len(), 1);

    controller.disconnect().await;
}

struct WsProbeServer {
    base_url: Url,
    probe: Arc<Notify>,
    handshake_path: Arc<Mutex<Option<String>>>,
    cookie_header: Arc<Mutex<Option<String>>>,
}

async fn spawn_ws_probe_server() -> WsProbeServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
    let probe = Arc::new(Notify::new());
    let handshake_path = Arc::new(Mutex::new(None));
    let cookie_header = Arc::new(Mutex::new(None));

    let probe_task = Arc::clone(&probe);
    let path_task = Arc::clone(&handshake_path);
    let cookie_task = Arc::clone(&cookie_header);

    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let probe = Arc::clone(&probe_task);
            let handshake_path = Arc::clone(&path_task);
            let cookie_header = Arc::clone(&cookie_task);

            tokio::spawn(async move {
                let _ = handle_connection(stream, probe, handshake_path, cookie_header).await;
            });
        }
    });

    WsProbeServer {
        base_url,
        probe,
        handshake_path,
        cookie_header,
    }
}

struct HttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
}

#[allow(clippy::too_many_lines)]
async fn handle_connection(
    mut stream: TcpStream,
    probe: Arc<Notify>,
    handshake_path: Arc<Mutex<Option<String>>>,
    cookie_header: Arc<Mutex<Option<String>>>,
) -> std::io::Result<()> {
    let Ok(Some(request)) = read_request(&mut stream).await else {
        return Ok(());
    };

    if request.method == "GET" && request.path == "/api/auth/login" {
        write_http_response(&mut stream, 404, &[], b"not found").await?;
        return Ok(());
    }

    if request.method == "GET" && request.path == "/api/login" {
        write_http_response(&mut stream, 200, &[], b"{}").await?;
        return Ok(());
    }

    if request.method == "POST" && request.path == "/api/login" {
        write_http_response(
            &mut stream,
            200,
            &[("Set-Cookie", "unifly_session=session-cookie; Path=/")],
            b"{}",
        )
        .await?;
        return Ok(());
    }

    if request.method == "POST" && request.path == "/api/logout" {
        write_http_response(&mut stream, 200, &[], b"{}").await?;
        return Ok(());
    }

    if request.method == "GET" && request.path.starts_with("/api/s/default/stat/device") {
        write_http_response(
            &mut stream,
            200,
            &[("Content-Type", "application/json")],
            br#"{"meta":{"rc":"ok"},"data":[]}"#,
        )
        .await?;
        return Ok(());
    }

    if request.method == "GET" && request.path.starts_with("/api/s/default/stat/sta") {
        write_http_response(
            &mut stream,
            200,
            &[("Content-Type", "application/json")],
            br#"{"meta":{"rc":"ok"},"data":[]}"#,
        )
        .await?;
        return Ok(());
    }

    if request.method == "GET" && request.path.starts_with("/api/s/default/stat/event") {
        write_http_response(
            &mut stream,
            200,
            &[("Content-Type", "application/json")],
            br#"{"meta":{"rc":"ok"},"data":[]}"#,
        )
        .await?;
        return Ok(());
    }

    if request.method == "GET" && request.path == "/api/self/sites" {
        write_http_response(
            &mut stream,
            200,
            &[("Content-Type", "application/json")],
            br#"{"meta":{"rc":"ok"},"data":[]}"#,
        )
        .await?;
        return Ok(());
    }

    if request.method == "GET"
        && request.path == "/wss/s/default/events"
        && request
            .headers
            .get("upgrade")
            .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
        && let Some(key) = request.headers.get("sec-websocket-key")
    {
        let accept = tokio_tungstenite::tungstenite::handshake::derive_accept_key(key.as_bytes());
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
                 Connection: Upgrade\r\n\
                 Upgrade: websocket\r\n\
                 Sec-WebSocket-Accept: {accept}\r\n\r\n"
        );
        stream.write_all(response.as_bytes()).await?;
        stream.flush().await?;

        *handshake_path.lock().await = Some(request.path.clone());
        *cookie_header.lock().await = request.headers.get("cookie").cloned();
        probe.notify_one();

        let mut scratch = [0u8; 1024];
        loop {
            match stream.read(&mut scratch).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
        return Ok(());
    }

    write_http_response(&mut stream, 404, &[], b"not found").await
}

async fn write_http_response(
    stream: &mut TcpStream,
    status: u16,
    headers: &[(&str, &str)],
    body: &[u8],
) -> std::io::Result<()> {
    let reason = match status {
        404 => "Not Found",
        101 => "Switching Protocols",
        _ => "OK",
    };

    let mut response = format!(
        "HTTP/1.1 {status} {reason}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");

    stream.write_all(response.as_bytes()).await?;
    if !body.is_empty() {
        stream.write_all(body).await?;
    }
    stream.flush().await?;
    let _ = stream.shutdown().await;
    Ok(())
}

async fn read_request(stream: &mut TcpStream) -> std::io::Result<Option<HttpRequest>> {
    let mut buf = Vec::new();
    let mut scratch = [0u8; 1024];

    let header_end = loop {
        let read = stream.read(&mut scratch).await?;
        if read == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&scratch[..read]);
        if let Some(pos) = find_header_end(&buf) {
            break pos;
        }
    };

    let header_text = std::str::from_utf8(&buf[..header_end]).map_err(std::io::Error::other)?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| std::io::Error::other("missing request line"))?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or_else(|| std::io::Error::other("missing request method"))?
        .to_owned();
    let path = request_parts
        .next()
        .ok_or_else(|| std::io::Error::other("missing request path"))?
        .to_owned();

    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }

    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let body_start = header_end + 4;
    while buf.len() < body_start + content_length {
        let read = stream.read(&mut scratch).await?;
        if read == 0 {
            break;
        }
        buf.extend_from_slice(&scratch[..read]);
    }

    Ok(Some(HttpRequest {
        method,
        path,
        headers,
    }))
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

#[tokio::test]
async fn api_key_mode_reports_unsupported_when_integration_api_missing() {
    let server = MockServer::start().await;

    // Platform detection: respond to /api/auth/login so detect_platform
    // identifies a UniFi OS controller.
    Mock::given(method("GET"))
        .and(path("/api/auth/login"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    // Integration API sites endpoint returns 404 — the controller
    // doesn't have it (older self-hosted UNA).
    Mock::given(method("GET"))
        .and(path("/proxy/network/integration/v1/sites"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("the-key")),
        "default",
        false,
    ));

    let err = controller.connect().await.unwrap_err();
    match err {
        CoreError::Unsupported {
            ref operation,
            ref required,
        } => {
            assert!(
                operation.contains("API-key"),
                "operation should mention API-key auth, got: {operation}"
            );
            assert!(
                required.contains("Integration API"),
                "required should mention Integration API, got: {required}"
            );
        }
        other => panic!("expected CoreError::Unsupported, got: {other:?}"),
    }
}
