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
use wiremock::matchers::{body_json, body_partial_json, header_regex, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

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
        totp_provider: None,
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
            format!("/proxy/network/integration/v1/sites/{API_KEY_SITE_ID}/hotspot/vouchers"),
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
    assert_eq!(controller.take_warnings().await, Vec::<String>::new());

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
async fn create_dns_policy_returns_created_entity() {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;

    Mock::given(method("POST"))
        .and(path(format!(
            "/proxy/network/integration/v1/sites/{API_KEY_SITE_ID}/dns/policies"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "type": "A_RECORD",
            "id": "7bd0e8b1-0a44-4c15-93a1-4c0e8f5be7aa",
            "enabled": true,
            "metadata": {"origin": "USER_DEFINED"},
            "domain": "printer.example.net",
            "ipv4Address": "10.0.1.42",
            "ttlSeconds": 300,
        })))
        .mount(&server)
        .await;

    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("the-key")),
        LEGACY_SITE_NAME,
        false,
    ));
    controller.connect().await.unwrap();

    let result = controller
        .execute(unifly_api::Command::CreateDnsPolicy(
            unifly_api::CreateDnsPolicyRequest {
                name: "printer.example.net".into(),
                policy_type: DnsPolicyType::ARecord,
                enabled: true,
                domain: Some("printer.example.net".into()),
                domains: None,
                upstream: None,
                value: Some("10.0.1.42".into()),
                ttl_seconds: Some(300),
                priority: None,
                ipv4_address: None,
                ipv6_address: None,
                target_domain: None,
                mail_server_domain: None,
                text: None,
                ip_address: None,
                server_domain: None,
                service: None,
                protocol: None,
                port: None,
                weight: None,
            },
        ))
        .await
        .expect("create should succeed");

    match result {
        unifly_api::CommandResult::DnsPolicy(policy) => {
            assert_eq!(policy.policy_type, DnsPolicyType::ARecord);
            assert_eq!(policy.domain, "printer.example.net");
            assert_eq!(policy.value, "10.0.1.42");
            assert_eq!(policy.ttl_seconds, Some(300));
        }
        other => panic!("expected CommandResult::DnsPolicy, got {other:?}"),
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

// ── MFA session renewal ──────────────────────────────────────────────

/// Mount a classic controller whose account has MFA. A login without a
/// code gets the 499 challenge (`challenges` times in total); each listed
/// code is accepted once and hands out its own session cookie, so a
/// renewal is distinguishable from the connect login.
async fn mock_mfa_legacy_controller(server: &MockServer, codes: &[(&str, &str)], challenges: u64) {
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
    for (code, cookie) in codes {
        Mock::given(method("POST"))
            .and(path("/api/login"))
            .and(body_partial_json(json!({ "ubic_2fa_token": code })))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Set-Cookie", format!("unifly_session={cookie}; Path=/"))
                    .set_body_json(json!({})),
            )
            .expect(1)
            .mount(server)
            .await;
    }
    Mock::given(method("POST"))
        .and(path("/api/login"))
        .and(body_json(
            json!({ "username": "admin", "password": "password" }),
        ))
        .respond_with(ResponseTemplate::new(499).set_body_json(json!({
            "errors": ["ubic_2fa_token required"],
            "token": "UBIC_2FA=challenge",
        })))
        .expect(challenges)
        .mount(server)
        .await;
}

/// A provider that hands out `codes` in order, then `None`, and counts how
/// often it was asked.
fn sequence_provider(
    codes: &'static [&'static str],
) -> (
    unifly_api::TotpProvider,
    Arc<std::sync::atomic::AtomicUsize>,
) {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let provider = unifly_api::TotpProvider::new(move || {
        let index = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        codes.get(index).map(|code| secret(code))
    });
    (provider, calls)
}

fn mfa_config(server: &MockServer) -> ControllerConfig {
    base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::Credentials {
            username: "admin".into(),
            password: secret("password"),
        },
        LEGACY_SITE_NAME,
        false,
    )
}

/// A session opened with a fixed TOTP code must never log in again: the
/// code is spent, and replaying it would fail on every attempt while
/// presenting the correct password.
#[tokio::test]
async fn static_totp_session_does_not_relogin() {
    let server = MockServer::start().await;
    mock_mfa_legacy_controller(&server, &[("123456", "first")], 1).await;
    Mock::given(method("GET"))
        .and(path("/api/s/default/stat/health"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;

    let mut config = mfa_config(&server);
    config.totp_token = Some(secret("123456"));
    let controller = Controller::new(config);
    controller.connect_lightweight().await.unwrap();

    controller
        .raw_get("api/s/default/stat/health")
        .await
        .expect_err("an expired static-code MFA session stays expired");

    controller.disconnect().await;
    server.verify().await;
}

/// With a provider and no fixed code, the connect login asks it for a code
/// at the MFA challenge, and the re-login after an expiry asks it again
/// for a fresh one instead of reusing the first.
#[tokio::test]
async fn provider_backed_mfa_session_renews_with_a_fresh_code() {
    let server = MockServer::start().await;
    mock_mfa_legacy_controller(&server, &[("111111", "first"), ("222222", "second")], 2).await;
    Mock::given(method("GET"))
        .and(path("/api/s/default/stat/health"))
        .and(header_regex("cookie", "unifly_session=first"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/s/default/stat/health"))
        .and(header_regex("cookie", "unifly_session=second"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_legacy_envelope()))
        .expect(1)
        .mount(&server)
        .await;

    let (provider, calls) = sequence_provider(&["111111", "222222"]);
    let mut config = mfa_config(&server);
    config.totp_provider = Some(provider);
    let controller = Controller::new(config);
    controller.connect_lightweight().await.unwrap();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);

    let value = controller
        .raw_get("api/s/default/stat/health")
        .await
        .expect("the expired MFA session renews with the provider's next code");
    assert_eq!(value["meta"]["rc"], "ok");
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);

    controller.disconnect().await;
    server.verify().await;
}

/// A provider may be slow or interactive (a password manager prompt), so a
/// login the controller does not challenge must never run it.
#[tokio::test]
async fn totp_provider_is_not_run_without_an_mfa_challenge() {
    let server = MockServer::start().await;
    mock_legacy_connect(&server, legacy_site_envelope()).await;

    let (provider, calls) = sequence_provider(&["111111"]);
    let mut config = mfa_config(&server);
    config.totp_provider = Some(provider);
    let controller = Controller::new(config);
    controller.connect_lightweight().await.unwrap();

    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    controller.disconnect().await;
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

fn integration_route(suffix: &str) -> String {
    format!("/proxy/network/integration/v1/sites/{API_KEY_SITE_ID}/{suffix}")
}

fn integration_page(item: &serde_json::Value) -> serde_json::Value {
    json!({
        "offset": 0,
        "limit": 200,
        "count": 1,
        "totalCount": 1,
        "data": [item],
    })
}

fn nat_rules_body() -> serde_json::Value {
    json!([{
        "_id": "nat-1",
        "type": "DNAT",
        "description": "web forward",
        "enabled": true,
    }])
}

fn firewall_groups_envelope() -> serde_json::Value {
    legacy_envelope(&json!([{
        "_id": "fwg-1",
        "name": "web-ports",
        "group_type": "port-group",
        "group_members": ["80", "443"],
    }]))
}

/// What every switchable optional route serves on its next request.
#[derive(Clone, Copy)]
enum RoutePhase {
    Populated,
    Failing(u16),
    /// HTTP 200 carrying the error object UniFi OS sometimes sends instead
    /// of a real status code.
    WrappedError,
    Empty,
}

/// An optional endpoint whose response follows a shared [`RoutePhase`].
///
/// Flipping one phase beats scoped mock overrides: wiremock re-sorts its
/// mocks by priority on every request, and a dropped scoped guard then
/// deactivates whichever mock now sits at its stored index.
struct SwitchableRoute {
    phase: Arc<std::sync::Mutex<RoutePhase>>,
    populated: serde_json::Value,
    empty: serde_json::Value,
}

impl Respond for SwitchableRoute {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        let phase = *self.phase.lock().unwrap();
        match phase {
            RoutePhase::Populated => ResponseTemplate::new(200).set_body_json(&self.populated),
            RoutePhase::Failing(status) => ResponseTemplate::new(status),
            RoutePhase::WrappedError => ResponseTemplate::new(200).set_body_json(json!({
                "error": {"code": 500, "message": "internal error"},
            })),
            RoutePhase::Empty => ResponseTemplate::new(200).set_body_json(&self.empty),
        }
    }
}

/// Mount each `(route, populated body, empty body)` ahead of the default
/// mocks and return the phase switch they all share.
async fn mount_switchable_routes(
    server: &MockServer,
    routes: Vec<(String, serde_json::Value, serde_json::Value)>,
) -> Arc<std::sync::Mutex<RoutePhase>> {
    let phase = Arc::new(std::sync::Mutex::new(RoutePhase::Populated));
    for (route, populated, empty) in routes {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(SwitchableRoute {
                phase: Arc::clone(&phase),
                populated,
                empty,
            })
            .with_priority(1)
            .mount(server)
            .await;
    }
    phase
}

fn set_phase(phase: &std::sync::Mutex<RoutePhase>, next: RoutePhase) {
    *phase.lock().unwrap() = next;
}

/// Every optional refresh endpoint in API-key mode, with one healthy
/// populated body and the body a controller returns when it has none.
fn api_key_optional_routes() -> Vec<(String, serde_json::Value, serde_json::Value)> {
    vec![
        (
            integration_route("firewall/policies"),
            integration_page(&json!({
                "id": "6f1c1f4e-9a1c-4c55-9f0e-2b8f6f9c0a01",
                "name": "Allow web",
                "enabled": true,
                "action": {"type": "ALLOW"},
                "metadata": {"origin": "USER_DEFINED"},
            })),
            empty_integration_page(200),
        ),
        (
            integration_route("firewall/zones"),
            integration_page(&json!({
                "id": "6f1c1f4e-9a1c-4c55-9f0e-2b8f6f9c0a02",
                "name": "Internal",
                "networkIds": [],
                "metadata": {"origin": "SYSTEM_DEFINED"},
            })),
            empty_integration_page(200),
        ),
        (
            integration_route("acl-rules"),
            integration_page(&json!({
                "id": "6f1c1f4e-9a1c-4c55-9f0e-2b8f6f9c0a03",
                "name": "Block cameras",
                "type": "IPV4",
                "action": "BLOCK",
                "enabled": true,
                "index": 1,
                "metadata": {"origin": "USER_DEFINED"},
            })),
            empty_integration_page(200),
        ),
        (
            integration_route("dns/policies"),
            dns_policies_page(),
            empty_integration_page(200),
        ),
        (
            integration_route("hotspot/vouchers"),
            integration_page(&json!({
                "id": "6f1c1f4e-9a1c-4c55-9f0e-2b8f6f9c0a04",
                "code": "1234567890",
                "name": "Guest",
                "createdAt": "2025-01-01T00:00:00Z",
                "expired": false,
                "timeLimitMinutes": 60,
                "authorizedGuestCount": 0,
            })),
            empty_integration_page(200),
        ),
        (
            "/proxy/network/api/s/default/stat/event".to_owned(),
            api_key_event_envelope(),
            empty_legacy_envelope(),
        ),
        (
            "/proxy/network/v2/api/site/default/nat".to_owned(),
            nat_rules_body(),
            json!([]),
        ),
        (
            "/proxy/network/api/s/default/rest/firewallgroup".to_owned(),
            firewall_groups_envelope(),
            empty_legacy_envelope(),
        ),
    ]
}

fn optional_collection_counts(controller: &Controller) -> [(&'static str, usize); 8] {
    [
        (
            "firewall policies",
            controller.firewall_policies_snapshot().len(),
        ),
        ("firewall zones", controller.firewall_zones_snapshot().len()),
        ("acl rules", controller.acl_rules_snapshot().len()),
        ("dns policies", controller.dns_policies_snapshot().len()),
        ("vouchers", controller.vouchers_snapshot().len()),
        ("events", controller.events_snapshot().len()),
        ("nat policies", controller.nat_policies_snapshot().len()),
        (
            "firewall groups",
            controller.firewall_groups_snapshot().len(),
        ),
    ]
}

#[tokio::test]
async fn failed_optional_fetches_keep_last_known_collections() {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;
    let phase = mount_switchable_routes(&server, api_key_optional_routes()).await;

    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("the-key")),
        LEGACY_SITE_NAME,
        false,
    ));
    controller.connect().await.unwrap();

    let populated = [
        ("firewall policies", 1),
        ("firewall zones", 1),
        ("acl rules", 1),
        ("dns policies", 2),
        ("vouchers", 1),
        ("events", 1),
        ("nat policies", 1),
        ("firewall groups", 1),
    ];
    assert_eq!(optional_collection_counts(&controller), populated);

    for status in [500, 404] {
        set_phase(&phase, RoutePhase::Failing(status));
        controller.full_refresh().await.unwrap();
        assert_eq!(
            optional_collection_counts(&controller),
            populated,
            "HTTP {status} from optional endpoints must keep last known data"
        );
    }

    set_phase(&phase, RoutePhase::WrappedError);
    controller.full_refresh().await.unwrap();
    assert_eq!(
        optional_collection_counts(&controller),
        populated,
        "an error object served with HTTP 200 must keep last known data"
    );

    set_phase(&phase, RoutePhase::Empty);
    controller.full_refresh().await.unwrap();
    assert_eq!(
        optional_collection_counts(&controller),
        [
            ("firewall policies", 0),
            ("firewall zones", 0),
            ("acl rules", 0),
            ("dns policies", 0),
            ("vouchers", 0),
            ("events", 0),
            ("nat policies", 0),
            ("firewall groups", 0),
        ],
        "a successful empty response is authoritative and prunes"
    );

    controller.disconnect().await;
}

const LAN_NETWORK_ID: &str = "8a1e5f3c-2b4d-4e6f-9a0b-1c2d3e4f5a01";
const GUEST_NETWORK_ID: &str = "8a1e5f3c-2b4d-4e6f-9a0b-1c2d3e4f5a02";

fn network_summary(id: &str, name: &str, vlan_id: u16) -> serde_json::Value {
    json!({
        "id": id,
        "name": name,
        "enabled": true,
        "management": "GATEWAY",
        "vlanId": vlan_id,
        "metadata": {"origin": "USER_DEFINED"},
    })
}

fn network_by_name(controller: &Controller, name: &str) -> Arc<unifly_api::model::Network> {
    controller
        .networks_snapshot()
        .iter()
        .find(|network| network.name == name)
        .cloned()
        .unwrap_or_else(|| panic!("network {name} missing from the store"))
}

#[tokio::test]
async fn failed_network_detail_fetch_keeps_the_network() {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;

    Mock::given(method("GET"))
        .and(path(integration_route("networks")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "offset": 0,
            "limit": 200,
            "count": 2,
            "totalCount": 2,
            "data": [
                network_summary(LAN_NETWORK_ID, "LAN", 1),
                network_summary(GUEST_NETWORK_ID, "Guest", 30),
            ],
        })))
        .with_priority(1)
        .mount(&server)
        .await;

    let mut lan_detail = network_summary(LAN_NETWORK_ID, "LAN", 1);
    lan_detail["ipv4Configuration"] = json!({"hostIpAddress": "10.0.1.1", "prefixLength": 24});
    let lan_phase = mount_switchable_routes(
        &server,
        vec![(
            integration_route(&format!("networks/{LAN_NETWORK_ID}")),
            lan_detail,
            json!({}),
        )],
    )
    .await;

    // The guest network's detail never loads, so the store only ever has
    // its summary.
    Mock::given(method("GET"))
        .and(path(integration_route(&format!(
            "networks/{GUEST_NETWORK_ID}"
        ))))
        .respond_with(ResponseTemplate::new(500))
        .with_priority(1)
        .mount(&server)
        .await;

    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("the-key")),
        LEGACY_SITE_NAME,
        false,
    ));
    controller.connect().await.unwrap();

    assert_eq!(controller.networks_snapshot().len(), 2);
    assert_eq!(
        network_by_name(&controller, "LAN").subnet.as_deref(),
        Some("10.0.1.1/24")
    );
    let guest = network_by_name(&controller, "Guest");
    assert_eq!(guest.vlan_id, Some(30));
    assert_eq!(guest.subnet, None);

    set_phase(&lan_phase, RoutePhase::Failing(500));
    controller.full_refresh().await.unwrap();

    assert_eq!(controller.networks_snapshot().len(), 2);
    assert_eq!(
        network_by_name(&controller, "LAN").subnet.as_deref(),
        Some("10.0.1.1/24"),
        "a failed detail fetch keeps the last known detailed record"
    );
    assert_eq!(network_by_name(&controller, "Guest").vlan_id, Some(30));

    controller.disconnect().await;
}

const SWITCH_MAC: &str = "aa:bb:cc:00:00:01";

#[tokio::test]
async fn failed_session_merge_fetches_keep_merged_fields() {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;

    Mock::given(method("GET"))
        .and(path(integration_route("devices")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(integration_page(&json!({
                "id": "9b2f6c1e-7a3d-4c5b-8e9f-0a1b2c3d4e5f",
                "macAddress": SWITCH_MAC,
                "ipAddress": "10.0.0.2",
                "name": "Core Switch",
                "model": "USW-24",
                "state": "ONLINE",
                "supported": true,
                "firmwareUpdatable": false,
                "features": [],
                "interfaces": {},
            }))),
        )
        .with_priority(1)
        .mount(&server)
        .await;

    // The Integration client (aa:bb:cc:dd:ee:ff at 10.0.0.50) gets its
    // hostname and traffic from stat/sta, its reservation from rest/user,
    // and the switch gets its client count and uplink from stat/device.
    let phase = mount_switchable_routes(
        &server,
        vec![
            (
                "/proxy/network/api/s/default/stat/sta".to_owned(),
                legacy_envelope(&json!([{
                    "_id": "client-1",
                    "mac": "aa:bb:cc:dd:ee:ff",
                    "ip": "10.0.0.50",
                    "hostname": "test-host",
                    "is_wired": false,
                    "tx_bytes": 1234,
                    "rx_bytes": 5678,
                }])),
                empty_legacy_envelope(),
            ),
            (
                "/proxy/network/api/s/default/rest/user".to_owned(),
                legacy_envelope(&json!([{
                    "_id": "user-1",
                    "mac": "aa:bb:cc:dd:ee:ff",
                    "use_fixedip": true,
                    "fixed_ip": "10.0.0.60",
                }])),
                empty_legacy_envelope(),
            ),
            (
                "/proxy/network/api/s/default/stat/device".to_owned(),
                legacy_envelope(&json!([{
                    "_id": "dev-1",
                    "mac": SWITCH_MAC,
                    "type": "usw",
                    "num_sta": 7,
                    "uplink": {"uplink_mac": "11:22:33:44:55:66", "uplink_remote_port": 4},
                }])),
                empty_legacy_envelope(),
            ),
        ],
    )
    .await;

    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("the-key")),
        LEGACY_SITE_NAME,
        false,
    ));
    controller.connect().await.unwrap();

    let assert_merged = |when: &str| {
        let client = controller
            .store()
            .client_by_mac(&MacAddress::new("aa:bb:cc:dd:ee:ff"))
            .expect("client present");
        assert_eq!(client.hostname.as_deref(), Some("test-host"), "{when}");
        assert_eq!(client.tx_bytes, Some(1234), "{when}");
        assert_eq!(client.rx_bytes, Some(5678), "{when}");
        assert!(client.use_fixedip, "{when}");
        assert_eq!(
            client.fixed_ip,
            Some("10.0.0.60".parse().unwrap()),
            "{when}"
        );

        let device = controller
            .store()
            .device_by_mac(&MacAddress::new(SWITCH_MAC))
            .expect("switch present");
        assert_eq!(device.client_count, Some(7), "{when}");
        assert_eq!(
            device.uplink_device_mac,
            Some(MacAddress::new("11:22:33:44:55:66")),
            "{when}"
        );
        assert_eq!(device.uplink_port_idx, Some(4), "{when}");
    };

    assert_merged("after the initial refresh");

    set_phase(&phase, RoutePhase::Failing(500));
    controller.full_refresh().await.unwrap();
    assert_merged("after the session merge fetches failed");

    controller.disconnect().await;
}

#[tokio::test]
async fn session_only_refresh_keeps_nat_and_groups_when_fetch_fails() {
    let server = MockServer::start().await;
    mock_legacy_connect(&server, legacy_site_envelope()).await;
    let phase = mount_switchable_routes(
        &server,
        vec![
            (
                "/v2/api/site/default/nat".to_owned(),
                nat_rules_body(),
                json!([]),
            ),
            (
                "/api/s/default/rest/firewallgroup".to_owned(),
                firewall_groups_envelope(),
                empty_legacy_envelope(),
            ),
        ],
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
    controller.connect().await.unwrap();
    assert_eq!(controller.nat_policies_snapshot().len(), 1);
    assert_eq!(controller.firewall_groups_snapshot().len(), 1);

    set_phase(&phase, RoutePhase::Failing(500));
    controller.full_refresh().await.unwrap();
    assert_eq!(controller.nat_policies_snapshot().len(), 1);
    assert_eq!(controller.firewall_groups_snapshot().len(), 1);

    set_phase(&phase, RoutePhase::Empty);
    controller.full_refresh().await.unwrap();
    assert_eq!(controller.nat_policies_snapshot().len(), 0);
    assert_eq!(controller.firewall_groups_snapshot().len(), 0);

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

async fn mount_speedtest_route(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/api/s/default/cmd/devmgr"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_legacy_envelope()))
        .mount(server)
        .await;
}

async fn speedtest_requests(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|request| request.url.path() == "/api/s/default/cmd/devmgr")
        .count()
}

fn legacy_controller(server: &MockServer) -> Controller {
    Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::Credentials {
            username: "admin".into(),
            password: secret("password"),
        },
        LEGACY_SITE_NAME,
        false,
    ))
}

#[tokio::test]
async fn execute_before_connect_fails_fast_and_never_replays() {
    let server = MockServer::start().await;
    mock_legacy_connect(&server, legacy_site_envelope()).await;
    mount_speedtest_route(&server).await;
    let controller = legacy_controller(&server);

    let early = tokio::spawn({
        let controller = controller.clone();
        async move {
            controller
                .execute(unifly_api::Command::SpeedtestDevice)
                .await
        }
    });
    tokio::task::yield_now().await;

    controller.connect().await.unwrap();
    let early = timeout(Duration::from_secs(2), early)
        .await
        .expect("a command issued before connect must not wait for one")
        .unwrap();
    assert!(
        matches!(early, Err(CoreError::ControllerDisconnected)),
        "expected ControllerDisconnected, got {early:?}"
    );
    assert_eq!(
        speedtest_requests(&server).await,
        0,
        "a command issued before connect must not run once connected"
    );

    controller.disconnect().await;
}

#[tokio::test]
async fn execute_after_disconnect_fails_fast() {
    let server = MockServer::start().await;
    mock_legacy_connect(&server, legacy_site_envelope()).await;
    mount_speedtest_route(&server).await;
    let controller = legacy_controller(&server);

    controller.connect().await.unwrap();
    controller
        .execute(unifly_api::Command::SpeedtestDevice)
        .await
        .expect("a connected controller runs commands");
    controller.disconnect().await;

    let late = timeout(
        Duration::from_secs(2),
        controller.execute(unifly_api::Command::SpeedtestDevice),
    )
    .await
    .expect("a command issued after disconnect must not wait for a connection");
    assert!(
        matches!(late, Err(CoreError::ControllerDisconnected)),
        "expected ControllerDisconnected, got {late:?}"
    );

    controller.connect().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        speedtest_requests(&server).await,
        1,
        "only the command issued while connected may reach the controller"
    );
    controller.disconnect().await;
}

#[tokio::test]
async fn status_channels_hold_their_value_without_subscribers() {
    let server = MockServer::start().await;
    mock_legacy_connect(&server, legacy_site_envelope()).await;
    let controller = legacy_controller(&server);

    controller.connect().await.unwrap();

    assert_eq!(
        *controller.connection_state().borrow(),
        unifly_api::ConnectionState::Connected,
        "a subscriber that arrives after connect must see Connected"
    );
    assert!(
        controller.store().last_full_refresh().is_some(),
        "the initial refresh must be recorded"
    );
    assert!(controller.store().data_age().is_some());

    controller.disconnect().await;
}

#[tokio::test]
async fn failed_connect_publishes_failed_state() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/auth/login"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/proxy/network/integration/v1/sites"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("the-key")),
        LEGACY_SITE_NAME,
        false,
    ));

    controller.connect().await.unwrap_err();

    assert_eq!(
        *controller.connection_state().borrow(),
        unifly_api::ConnectionState::Failed,
        "a failed connect must not leave the state at Connecting"
    );
}

/// Connect in API-key mode against a controller whose LAN network has the
/// given IPv4 configuration, run `update` on it, and return the body the
/// controller received on the network PUT.
async fn update_lan_network(
    ipv4_configuration: serde_json::Value,
    update: unifly_api::UpdateNetworkRequest,
) -> Result<serde_json::Value, CoreError> {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;

    let mut detail = network_summary(LAN_NETWORK_ID, "LAN", 1);
    detail["isolationEnabled"] = json!(false);
    detail["ipv4Configuration"] = ipv4_configuration;
    let route = integration_route(&format!("networks/{LAN_NETWORK_ID}"));
    Mock::given(method("GET"))
        .and(path(route.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(detail.clone()))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(route.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(detail))
        .mount(&server)
        .await;

    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("the-key")),
        LEGACY_SITE_NAME,
        false,
    ));
    controller.connect().await.unwrap();
    let result = controller
        .execute(unifly_api::Command::UpdateNetwork {
            id: unifly_api::EntityId::from(LAN_NETWORK_ID.to_owned()),
            update,
        })
        .await;
    controller.disconnect().await;
    result?;

    let put = server
        .received_requests()
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|request| {
            request.method == wiremock::http::Method::PUT && request.url.path() == route
        })
        .expect("the update must PUT the network");
    Ok(serde_json::from_slice(&put.body).unwrap())
}

fn serving_dhcp_ipv4() -> serde_json::Value {
    json!({
        "autoScaleEnabled": false,
        "hostIpAddress": "10.0.30.1",
        "prefixLength": 24,
        "dhcpConfiguration": {
            "mode": "SERVER",
            "ipAddressRange": {"start": "10.0.30.100", "stop": "10.0.30.254"},
            "leaseTimeSeconds": 3600,
            "pingConflictDetectionEnabled": true,
        },
    })
}

#[tokio::test]
async fn update_network_writes_subnet_and_disables_dhcp() {
    let body = update_lan_network(
        serving_dhcp_ipv4(),
        unifly_api::UpdateNetworkRequest {
            subnet: Some("10.0.40.1/23".into()),
            dhcp_enabled: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let ipv4 = &body["ipv4Configuration"];
    assert_eq!(ipv4["hostIpAddress"], json!("10.0.40.1"));
    assert_eq!(ipv4["prefixLength"], json!(23));
    assert_eq!(ipv4["autoScaleEnabled"], json!(false));
    assert!(
        ipv4.get("dhcpConfiguration").is_none(),
        "turning DHCP off omits the DHCP configuration: {ipv4}"
    );
}

#[tokio::test]
async fn update_network_enables_dhcp_as_a_server() {
    let body = update_lan_network(
        json!({"autoScaleEnabled": false, "hostIpAddress": "10.0.30.1", "prefixLength": 24}),
        unifly_api::UpdateNetworkRequest {
            dhcp_enabled: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(
        body["ipv4Configuration"]["dhcpConfiguration"],
        json!({
            "mode": "SERVER",
            "leaseTimeSeconds": 86400,
            "pingConflictDetectionEnabled": false,
        })
    );
}

#[tokio::test]
async fn update_network_keeps_a_running_dhcp_server_as_is() {
    let body = update_lan_network(
        serving_dhcp_ipv4(),
        unifly_api::UpdateNetworkRequest {
            dhcp_enabled: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(body["ipv4Configuration"], serving_dhcp_ipv4());
}

#[tokio::test]
async fn update_network_rejects_subnet_without_ipv4_configuration() {
    let err = update_lan_network(
        serde_json::Value::Null,
        unifly_api::UpdateNetworkRequest {
            subnet: Some("10.0.40.1/24".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();

    assert!(
        matches!(err, CoreError::ValidationFailed { .. }),
        "expected ValidationFailed, got {err:?}"
    );
}

/// Run a speed test against a controller whose `cmd/devmgr` route stalls
/// past a 1 s request timeout.
async fn stalled_speedtest(api_key: bool) -> CoreError {
    let server = MockServer::start().await;
    let devmgr = if api_key {
        mock_api_key_with_legacy(&server).await;
        "/proxy/network/api/s/default/cmd/devmgr"
    } else {
        mock_legacy_connect(&server, legacy_site_envelope()).await;
        "/api/s/default/cmd/devmgr"
    };
    Mock::given(method("POST"))
        .and(path(devmgr))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(empty_legacy_envelope())
                .set_delay(Duration::from_secs(5)),
        )
        .mount(&server)
        .await;

    let auth = if api_key {
        AuthCredentials::ApiKey(secret("the-key"))
    } else {
        AuthCredentials::Credentials {
            username: "admin".into(),
            password: secret("password"),
        }
    };
    let mut config = base_config(
        Url::parse(&server.uri()).unwrap(),
        auth,
        LEGACY_SITE_NAME,
        false,
    );
    config.timeout = Duration::from_secs(1);
    let controller = Controller::new(config);
    controller.connect().await.unwrap();

    let err = controller
        .execute(unifly_api::Command::SpeedtestDevice)
        .await
        .unwrap_err();
    controller.disconnect().await;
    err
}

#[tokio::test]
async fn session_timeout_reports_the_configured_duration() {
    let err = stalled_speedtest(false).await;
    assert!(
        matches!(err, CoreError::Timeout { timeout_secs: 1 }),
        "expected Timeout after 1s, got {err:?}"
    );
}

#[tokio::test]
async fn api_key_session_timeout_reports_the_configured_duration() {
    let err = stalled_speedtest(true).await;
    assert!(
        matches!(err, CoreError::Timeout { timeout_secs: 1 }),
        "expected Timeout after 1s, got {err:?}"
    );
}
