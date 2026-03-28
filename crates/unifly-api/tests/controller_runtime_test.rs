#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use secrecy::SecretString;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, Notify};
use tokio::time::timeout;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use unifly_api::{AuthCredentials, Controller, ControllerConfig, CoreError, TlsVerification};

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
    }
}

fn secret(value: &str) -> SecretString {
    SecretString::from(value.to_owned())
}

fn empty_legacy_envelope() -> serde_json::Value {
    json!({
        "meta": { "rc": "ok" },
        "data": [],
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
    Mock::given(method("GET"))
        .and(path("/api/auth/login"))
        .respond_with(ResponseTemplate::new(404))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/login"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Set-Cookie", "unifly_session=legacy-cookie; Path=/")
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
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_legacy_envelope()))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/self/sites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(site_envelope))
        .mount(server)
        .await;
}

async fn mock_api_key_connect(server: &MockServer, site_id: &str) {
    Mock::given(method("GET"))
        .and(path("/api/auth/login"))
        .respond_with(ResponseTemplate::new(404))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/login"))
        .respond_with(ResponseTemplate::new(404))
        .mount(server)
        .await;

    for route in [
        format!("/integration/v1/sites/{site_id}/devices"),
        format!("/integration/v1/sites/{site_id}/clients"),
        format!("/integration/v1/sites/{site_id}/networks"),
        format!("/integration/v1/sites/{site_id}/wifi/broadcasts"),
        format!("/integration/v1/sites/{site_id}/firewall/policies"),
        format!("/integration/v1/sites/{site_id}/firewall/zones"),
        format!("/integration/v1/sites/{site_id}/acl-rules"),
        format!("/integration/v1/sites/{site_id}/dns/policies"),
        format!("/integration/v1/sites/{site_id}/vouchers"),
        format!("/integration/v1/sites/{site_id}/traffic-matching-lists"),
    ] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(empty_integration_page(200)))
            .mount(server)
            .await;
    }

    Mock::given(method("GET"))
        .and(path("/integration/v1/sites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_integration_page(50)))
        .mount(server)
        .await;
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

    assert!(controller.has_legacy_access().await);
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
async fn api_key_mode_does_not_assume_legacy_access() {
    let server = MockServer::start().await;
    mock_api_key_connect(&server, API_KEY_SITE_ID).await;

    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("api-key")),
        API_KEY_SITE_ID,
        false,
    ));

    controller.connect().await.unwrap();

    assert!(!controller.has_legacy_access().await);
    assert!(controller.has_integration_access().await);
    assert!(controller.take_warnings().await.is_empty());

    let err = controller.list_admins().await.unwrap_err();
    match err {
        CoreError::Unsupported {
            operation,
            required,
        } => {
            assert_eq!(operation, "Legacy API operation");
            assert_eq!(required, "Legacy API credentials");
        }
        other => panic!("expected Unsupported error, got {other:?}"),
    }

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
        cookie.contains("unifly_session=legacy-cookie"),
        "expected websocket cookie header to carry the legacy session, got: {cookie}"
    );

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
            &[("Set-Cookie", "unifly_session=legacy-cookie; Path=/")],
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
