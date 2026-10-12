#![allow(clippy::unwrap_used)]
// Integration tests for `SessionClient` using wiremock.

use serde_json::json;
use url::Url;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use unifly_api::{ControllerPlatform, Error, SessionClient, TransportConfig, session::SessionAuth};

// ── Helpers ─────────────────────────────────────────────────────────

async fn setup() -> (MockServer, SessionClient) {
    unifly_api::transport::ensure_crypto_provider();
    let server = MockServer::start().await;
    let base_url = Url::parse(&server.uri()).unwrap();
    let client = SessionClient::with_client(
        reqwest::Client::new(),
        base_url,
        "default".into(),
        ControllerPlatform::ClassicController,
        SessionAuth::Cookie,
    );
    (server, client)
}

async fn setup_api_key() -> (MockServer, SessionClient) {
    unifly_api::transport::ensure_crypto_provider();
    let server = MockServer::start().await;
    let base_url = Url::parse(&server.uri()).unwrap();
    let client = SessionClient::with_client(
        reqwest::Client::new(),
        base_url,
        "default".into(),
        ControllerPlatform::ClassicController,
        SessionAuth::ApiKey,
    );
    (server, client)
}

/// Setup with a cookie jar — required for MFA tests (cookie injection).
async fn setup_with_jar() -> (MockServer, SessionClient) {
    let server = MockServer::start().await;
    let base_url = Url::parse(&server.uri()).unwrap();
    let transport = TransportConfig::default().with_cookie_jar();
    let client = SessionClient::new(
        base_url,
        "default".into(),
        ControllerPlatform::ClassicController,
        &transport,
    )
    .unwrap();
    (server, client)
}

/// Cookie-jar client pre-seeded with a stale session cookie, as after a
/// cache restore whose token has since expired.
async fn setup_with_stale_cookie() -> (MockServer, SessionClient) {
    let server = MockServer::start().await;
    let base_url = Url::parse(&server.uri()).unwrap();
    let transport = TransportConfig::default().with_cookie_jar();
    transport
        .cookie_jar
        .as_ref()
        .unwrap()
        .add_cookie_str("TOKEN=stale", &base_url);
    let client = SessionClient::new(
        base_url,
        "default".into(),
        ControllerPlatform::ClassicController,
        &transport,
    )
    .unwrap();
    (server, client)
}

fn site_path(suffix: &str) -> String {
    format!("/api/s/default/{suffix}")
}

fn site_path_v2(suffix: &str) -> String {
    format!("/v2/api/site/default/{suffix}")
}

// ── Authentication tests ────────────────────────────────────────────

#[tokio::test]
async fn test_login_success() {
    let (server, client) = setup().await;

    Mock::given(method("POST"))
        .and(path("/api/login"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;

    let secret: secrecy::SecretString = "test-password".to_string().into();
    client.login("admin", &secret, None).await.unwrap();
}

#[tokio::test]
async fn test_login_failure() {
    let (server, client) = setup().await;

    Mock::given(method("POST"))
        .and(path("/api/login"))
        .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
        .mount(&server)
        .await;

    let secret: secrecy::SecretString = "wrong-password".to_string().into();
    let result = client.login("admin", &secret, None).await;

    assert!(
        matches!(result, Err(Error::Authentication { .. })),
        "expected Authentication error, got: {result:?}"
    );
}

#[tokio::test]
async fn detect_platform_prefers_classic_login_probe() {
    let server = MockServer::start().await;
    let base_url = Url::parse(&server.uri()).unwrap();

    Mock::given(method("GET"))
        .and(path("/api/login"))
        .respond_with(ResponseTemplate::new(400))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/auth/login"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let platform = SessionClient::detect_platform(&base_url).await.unwrap();
    assert_eq!(platform, ControllerPlatform::ClassicController);
}

#[tokio::test]
async fn detect_platform_falls_back_to_unifi_os_when_classic_path_missing() {
    let server = MockServer::start().await;
    let base_url = Url::parse(&server.uri()).unwrap();

    Mock::given(method("GET"))
        .and(path("/api/login"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/auth/login"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let platform = SessionClient::detect_platform(&base_url).await.unwrap();
    assert_eq!(platform, ControllerPlatform::UnifiOs);
}

#[tokio::test]
async fn detect_platform_treats_dual_401_login_probes_as_unifi_os() {
    let server = MockServer::start().await;
    let base_url = Url::parse(&server.uri()).unwrap();

    Mock::given(method("GET"))
        .and(path("/api/login"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/auth/login"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let platform = SessionClient::detect_platform(&base_url).await.unwrap();
    assert_eq!(platform, ControllerPlatform::UnifiOs);
}

// ── Device tests ────────────────────────────────────────────────────

#[tokio::test]
async fn test_list_devices() {
    let (server, client) = setup().await;

    let envelope = json!({
        "meta": { "rc": "ok" },
        "data": [{
            "_id": "abc123",
            "mac": "aa:bb:cc:dd:ee:ff",
            "type": "usw",
            "name": "Switch-24",
            "model": "US24",
            "adopted": true,
            "state": 1
        }]
    });

    Mock::given(method("GET"))
        .and(path(site_path("stat/device")))
        .respond_with(ResponseTemplate::new(200).set_body_json(&envelope))
        .mount(&server)
        .await;

    let devices = client.list_devices().await.unwrap();

    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].mac, "aa:bb:cc:dd:ee:ff");
    assert_eq!(devices[0].name.as_deref(), Some("Switch-24"));
    assert_eq!(devices[0].device_type, "usw");
    assert!(devices[0].adopted);
    assert_eq!(devices[0].state, 1);
}

// ── Event tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_list_events() {
    let (server, client) = setup().await;

    let envelope = json!({
        "meta": { "rc": "ok" },
        "data": [
            {
                "_id": "evt001",
                "key": "EVT_WU_Connected",
                "msg": "User connected",
                "datetime": "2024-06-15T10:30:00Z",
                "subsystem": "wlan"
            },
            {
                "_id": "evt002",
                "key": "EVT_LU_Disconnected",
                "msg": "User disconnected",
                "datetime": "2024-06-15T10:35:00Z",
                "subsystem": "lan"
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path(site_path("stat/event")))
        .respond_with(ResponseTemplate::new(200).set_body_json(&envelope))
        .mount(&server)
        .await;

    let events = client.list_events(None).await.unwrap();

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].id, "evt001");
    assert_eq!(events[0].key.as_deref(), Some("EVT_WU_Connected"));
    assert_eq!(events[1].subsystem.as_deref(), Some("lan"));
}

#[tokio::test]
async fn test_list_events_with_limit() {
    let (server, client) = setup().await;

    let envelope = json!({
        "meta": { "rc": "ok" },
        "data": [{
            "_id": "evt001",
            "key": "EVT_WU_Connected"
        }]
    });

    Mock::given(method("GET"))
        .and(path(site_path("stat/event")))
        .and(query_param("_limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&envelope))
        .mount(&server)
        .await;

    let events = client.list_events(Some(5)).await.unwrap();

    assert_eq!(events.len(), 1);
}

// ── Wi-Fi observability tests ──────────────────────────────────────

#[tokio::test]
async fn test_list_rogue_aps() {
    let (server, client) = setup().await;

    let envelope = json!({
        "meta": { "rc": "ok" },
        "data": [{
            "bssid": "aa:bb:cc:dd:ee:01",
            "essid": "NeighborWifi",
            "channel": 6,
            "freq": 2437,
            "signal": -72,
            "radio": "ng",
            "is_rogue": false,
            "ap_mac": "ff:ff:ff:ff:ff:01"
        }]
    });

    Mock::given(method("GET"))
        .and(path(site_path("stat/rogueap")))
        .respond_with(ResponseTemplate::new(200).set_body_json(&envelope))
        .mount(&server)
        .await;

    let aps = client.list_rogue_aps(None).await.unwrap();

    assert_eq!(aps.len(), 1);
    assert_eq!(aps[0].bssid, "aa:bb:cc:dd:ee:01");
    assert_eq!(aps[0].essid.as_deref(), Some("NeighborWifi"));
    assert_eq!(aps[0].channel, Some(6));
    assert!(!aps[0].is_rogue);
}

#[tokio::test]
async fn test_list_rogue_aps_with_within_param() {
    let (server, client) = setup().await;

    Mock::given(method("GET"))
        .and(path(site_path("stat/rogueap")))
        .and(query_param("within", "3600"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "meta": { "rc": "ok" },
            "data": []
        })))
        .mount(&server)
        .await;

    let aps = client.list_rogue_aps(Some(3600)).await.unwrap();
    assert!(aps.is_empty());
}

#[tokio::test]
async fn test_list_channels() {
    let (server, client) = setup().await;

    let envelope = json!({
        "meta": { "rc": "ok" },
        "data": [{
            "code": "840",
            "key": "US",
            "name": "United States",
            "channels_ng": [1, 6, 11],
            "channels_na": [36, 40, 44, 48, 149, 153, 157, 161, 165],
            "channels_na_dfs": [52, 56, 60, 64],
            "channels_6e": [1, 5, 9]
        }]
    });

    Mock::given(method("GET"))
        .and(path(site_path("stat/current-channel")))
        .respond_with(ResponseTemplate::new(200).set_body_json(&envelope))
        .mount(&server)
        .await;

    let channels = client.list_channels().await.unwrap();

    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].key.as_deref(), Some("US"));
    assert_eq!(channels[0].name.as_deref(), Some("United States"));
    let na = channels[0].channels_na.as_ref().unwrap();
    assert!(na.contains(&36));
    assert!(na.contains(&165));
    let ng = channels[0].channels_ng.as_ref().unwrap();
    assert!(ng.contains(&1));
    assert!(ng.contains(&11));
}

#[tokio::test]
async fn test_get_client_roams() {
    let (server, client) = setup().await;

    let response = json!([
        {
            "timestamp": 1_700_000_000_000_i64,
            "event_type": "CONNECTED",
            "ap_mac": "aa:bb:cc:00:00:01",
            "ssid": "HomeWiFi",
            "signal": -55,
            "band": "5g"
        },
        {
            "timestamp": 1_700_000_060_000_i64,
            "event_type": "ROAMED",
            "ap_mac": "aa:bb:cc:00:00:02",
            "ssid": "HomeWiFi",
            "signal": -48,
            "band": "5g"
        }
    ]);

    Mock::given(method("GET"))
        .and(path(
            "/v2/api/site/default/system-log/client-connection/aa:bb:cc:dd:ee:ff",
        ))
        .and(query_param("mac", "aa:bb:cc:dd:ee:ff"))
        .and(query_param("separateConnectionSignalParam", "false"))
        .and(query_param("limit", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response))
        .mount(&server)
        .await;

    let events = client
        .get_client_roams("aa:bb:cc:dd:ee:ff", Some(50))
        .await
        .unwrap();

    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["event_type"], "CONNECTED");
    assert_eq!(events[1]["event_type"], "ROAMED");
    assert_eq!(events[1]["signal"], -48);
}

#[tokio::test]
async fn test_get_client_wifi_experience() {
    let (server, client) = setup().await;

    let response = json!({
        "signal": -52,
        "noise": -95,
        "channel": 36,
        "channel_width": 80,
        "band": "5g",
        "radio_protocol": "ax",
        "link_download_rate_kbps": 866_700,
        "link_upload_rate_kbps": 866_700,
        "wifi_experience": 87,
        "nearest_neighbors": [
            { "bssid": "aa:bb:cc:00:00:01", "channel": 36, "signal": -40 }
        ],
        "uplink_devices": [
            { "device_name": "Living Room AP", "wifi_experience": 92 }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/v2/api/site/default/wifiman/10.0.0.50/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response))
        .mount(&server)
        .await;

    let data = client
        .get_client_wifi_experience("10.0.0.50")
        .await
        .unwrap();

    assert_eq!(data["wifi_experience"], 87);
    assert_eq!(data["signal"], -52);
    assert_eq!(data["band"], "5g");
    assert_eq!(data["nearest_neighbors"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn test_list_ipsec_sa() {
    let (server, client) = setup().await;

    let envelope = json!({
        "meta": { "rc": "ok" },
        "data": [{
            "name": "Office-to-DC",
            "remote_ip": "203.0.113.50",
            "local_ip": "10.4.21.1",
            "state": "ESTABLISHED",
            "tx_bytes": 1_048_576,
            "rx_bytes": 2_097_152,
            "uptime": 86_400,
            "ike_version": "2"
        }]
    });

    Mock::given(method("GET"))
        .and(path(site_path("stat/ipsec-sa")))
        .respond_with(ResponseTemplate::new(200).set_body_json(&envelope))
        .mount(&server)
        .await;

    let sas = client.list_ipsec_sa().await.unwrap();

    assert_eq!(sas.len(), 1);
    assert_eq!(sas[0].name.as_deref(), Some("Office-to-DC"));
    assert_eq!(sas[0].state.as_deref(), Some("ESTABLISHED"));
    assert_eq!(sas[0].tx_bytes, Some(1_048_576));
    assert_eq!(sas[0].uptime, Some(86_400));
}

#[tokio::test]
async fn test_list_ipsec_sa_404_returns_empty() {
    let (server, client) = setup().await;

    Mock::given(method("GET"))
        .and(path(site_path("stat/ipsec-sa")))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "meta": { "rc": "error", "msg": "api.err.NotFound" },
            "data": []
        })))
        .mount(&server)
        .await;

    let sas = client.list_ipsec_sa().await.unwrap();

    assert!(sas.is_empty());
}

// ── Error tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_unauthorized_without_cookie_jar_reports_invalid_api_key() {
    let (server, client) = setup_api_key().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let result = client.list_devices().await;

    assert!(
        matches!(result, Err(Error::InvalidApiKey)),
        "expected InvalidApiKey, got: {result:?}"
    );
}

#[tokio::test]
async fn test_unauthorized_with_cookie_jar_reports_session_expired() {
    let (server, client) = setup_with_jar().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let result = client.list_devices().await;

    assert!(
        matches!(result, Err(Error::SessionExpired)),
        "expected SessionExpired, got: {result:?}"
    );
}

#[tokio::test]
async fn test_legacy_api_error() {
    let (server, client) = setup().await;

    let envelope = json!({
        "meta": { "rc": "error", "msg": "api.err.InvalidObject" },
        "data": []
    });

    Mock::given(method("GET"))
        .and(path(site_path("stat/device")))
        .respond_with(ResponseTemplate::new(200).set_body_json(&envelope))
        .mount(&server)
        .await;

    let result = client.list_devices().await;

    match result {
        Err(Error::SessionApi { ref message }) => {
            assert!(
                message.contains("InvalidObject"),
                "expected 'InvalidObject' in message, got: {message}"
            );
        }
        other => panic!("expected SessionApi error, got: {other:?}"),
    }
}

// ── NAT rule tests ──────────────────────────────────────────────────

#[tokio::test]
async fn test_list_nat_rules() {
    let (server, client) = setup().await;

    Mock::given(method("GET"))
        .and(path("/v2/api/site/default/nat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"_id": "nat-1", "type": "DNAT", "description": "web"},
        ])))
        .mount(&server)
        .await;

    let rules = client.list_nat_rules().await.unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0]["_id"], "nat-1");
}

#[tokio::test]
async fn test_list_nat_rules_rejects_non_array_body() {
    let (server, client) = setup().await;

    Mock::given(method("GET"))
        .and(path("/v2/api/site/default/nat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "error": {"code": 500, "message": "internal error"},
        })))
        .mount(&server)
        .await;

    let result = client.list_nat_rules().await;
    match result {
        Err(Error::Deserialization { ref body, .. }) => {
            assert!(
                body.contains("internal error"),
                "expected the raw body in the error, got: {body}"
            );
        }
        other => panic!("expected Deserialization error, got: {other:?}"),
    }
}

// ── WireGuard peer tests ────────────────────────────────────────────

#[tokio::test]
async fn test_list_wireguard_peers() {
    let (server, client) = setup().await;

    Mock::given(method("GET"))
        .and(path(site_path_v2("wireguard/server123/users")))
        .and(query_param("networkId", "server123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "_id": "peer1", "name": "Laptop" }
        ])))
        .mount(&server)
        .await;

    let peers = client.list_wireguard_peers("server123").await.unwrap();

    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0]["_id"], "peer1");
}

#[tokio::test]
async fn test_list_all_wireguard_peers() {
    let (server, client) = setup().await;

    Mock::given(method("GET"))
        .and(path(site_path_v2("wireguard/users")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "_id": "peer1", "name": "Laptop" }
        ])))
        .mount(&server)
        .await;

    let peers = client.list_all_wireguard_peers().await.unwrap();

    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0]["name"], "Laptop");
}

#[tokio::test]
async fn test_get_wireguard_peer_existing_subnets() {
    let (server, client) = setup().await;

    Mock::given(method("GET"))
        .and(path(site_path_v2("wireguard/users/existing-subnets")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "subnets": ["10.0.0.0/24"]
        })))
        .mount(&server)
        .await;

    let body = client.get_wireguard_peer_existing_subnets().await.unwrap();

    assert_eq!(body["subnets"][0], "10.0.0.0/24");
}

#[tokio::test]
async fn test_create_wireguard_peers() {
    let (server, client) = setup().await;
    let payload = json!([
        {
            "name": "Laptop",
            "interface_ip": "192.168.42.2",
            "public_key": "pubkey",
            "allowed_ips": ["10.0.0.0/24"],
            "preshared_key": ""
        }
    ]);

    Mock::given(method("POST"))
        .and(path(site_path_v2("wireguard/server123/users/batch")))
        .and(body_json(payload.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    client
        .create_wireguard_peers("server123", &payload)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_update_wireguard_peers() {
    let (server, client) = setup().await;
    let payload = json!([
        {
            "_id": "peer1",
            "name": "Laptop",
            "interface_ip": "192.168.42.2",
            "public_key": "pubkey",
            "allowed_ips": ["10.0.0.0/24"],
            "preshared_key": ""
        }
    ]);

    Mock::given(method("PUT"))
        .and(path(site_path_v2("wireguard/server123/users/batch")))
        .and(body_json(payload.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    client
        .update_wireguard_peers("server123", &payload)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_delete_wireguard_peers() {
    let (server, client) = setup().await;
    let payload = json!(["peer1"]);

    Mock::given(method("POST"))
        .and(path(site_path_v2("wireguard/server123/users/batch_delete")))
        .and(body_json(payload.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    client
        .delete_wireguard_peers("server123", &payload)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_list_vpn_client_connections() {
    let (server, client) = setup().await;

    Mock::given(method("GET"))
        .and(path(site_path_v2("vpn/connections")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "connections": [
                { "network_id": "vpn1", "name": "Branch Client" }
            ]
        })))
        .mount(&server)
        .await;

    let connections = client.list_vpn_client_connections().await.unwrap();

    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0]["network_id"], "vpn1");
}

#[tokio::test]
async fn test_get_openvpn_port_suggestions() {
    let (server, client) = setup().await;

    Mock::given(method("GET"))
        .and(path(site_path_v2("network/port-suggest")))
        .and(query_param("service", "openvpn"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "available_ports": [1194]
        })))
        .mount(&server)
        .await;

    let body = client.get_openvpn_port_suggestions().await.unwrap();

    assert_eq!(body["available_ports"][0], 1194);
}

#[tokio::test]
async fn test_restart_vpn_client_connection() {
    let (server, client) = setup().await;

    Mock::given(method("POST"))
        .and(path(site_path_v2("vpn/vpn1/restart")))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    client.restart_vpn_client_connection("vpn1").await.unwrap();
}

#[tokio::test]
async fn test_download_openvpn_configuration() {
    let (server, client) = setup().await;
    let body = b"client\nremote example.com 1194\n";

    Mock::given(method("GET"))
        .and(path(site_path_v2("vpn/openvpn/server123/configuration")))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body.to_vec(), "text/plain"))
        .mount(&server)
        .await;

    let bytes = client
        .download_openvpn_configuration("server123")
        .await
        .unwrap();

    assert_eq!(bytes, body);
}

#[tokio::test]
async fn test_list_magic_site_to_site_vpn_configs() {
    let (server, client) = setup().await;

    Mock::given(method("GET"))
        .and(path(site_path_v2("magicsitetositevpn/configs")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": "magic1", "name": "HQ <-> Branch" }
        ])))
        .mount(&server)
        .await;

    let configs = client.list_magic_site_to_site_vpn_configs().await.unwrap();

    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0]["id"], "magic1");
}

// ── MFA/TOTP tests ─────────────────────────────────────────────────

#[tokio::test]
async fn test_mfa_challenge_without_totp_returns_two_factor_required() {
    let (server, client) = setup_with_jar().await;

    Mock::given(method("POST"))
        .and(path("/api/login"))
        .respond_with(ResponseTemplate::new(499).set_body_json(json!({
            "errors": ["ubic_2fa_token required"],
            "token": "UBIC_2FA=eyJhbGciOiJIUzI1NiJ9"
        })))
        .mount(&server)
        .await;

    let secret: secrecy::SecretString = "password".to_string().into();
    let result = client.login("admin", &secret, None).await;

    assert!(
        matches!(result, Err(Error::TwoFactorRequired)),
        "expected TwoFactorRequired, got: {result:?}"
    );
}

#[tokio::test]
async fn test_mfa_login_with_valid_totp_succeeds() {
    let (server, client) = setup_with_jar().await;

    // Use body matching to distinguish the two login attempts:
    // - First POST has no ubic_2fa_token → 499 challenge
    // - Second POST has ubic_2fa_token → 200 success
    //
    // wiremock matches most-recently-mounted first, so mount
    // the success (TOTP) mock first, then the challenge mock.
    Mock::given(method("POST"))
        .and(path("/api/login"))
        .and(wiremock::matchers::body_string_contains("ubic_2fa_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .named("mfa-complete")
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/login"))
        .respond_with(ResponseTemplate::new(499).set_body_json(json!({
            "errors": ["ubic_2fa_token required"],
            "token": "UBIC_2FA=eyJhbGciOiJIUzI1NiJ9"
        })))
        .named("mfa-challenge")
        .mount(&server)
        .await;

    let secret: secrecy::SecretString = "password".to_string().into();
    let totp: secrecy::SecretString = "123456".to_string().into();
    client.login("admin", &secret, Some(&totp)).await.unwrap();
}

#[tokio::test]
async fn test_mfa_rejects_invalid_totp_format() {
    let (server, client) = setup_with_jar().await;

    Mock::given(method("POST"))
        .and(path("/api/login"))
        .respond_with(ResponseTemplate::new(499).set_body_json(json!({
            "errors": ["ubic_2fa_token required"],
            "token": "UBIC_2FA=eyJhbGciOiJIUzI1NiJ9"
        })))
        .mount(&server)
        .await;

    let secret: secrecy::SecretString = "password".to_string().into();
    let bad_totp: secrecy::SecretString = "not6digits".to_string().into();
    let result = client.login("admin", &secret, Some(&bad_totp)).await;

    assert!(
        matches!(result, Err(Error::Authentication { .. })),
        "expected Authentication error for bad TOTP, got: {result:?}"
    );
}

// ── Session re-authentication ───────────────────────────────────────

/// A minimal successful `stat/health` body, used as the post-re-login
/// response in the re-authentication tests.
fn health_ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "meta": {"rc": "ok"},
        "data": [{"subsystem": "wan", "status": "ok"}]
    }))
}

#[tokio::test]
async fn expired_cookie_session_relogs_in_and_retries_once() {
    let (server, client) = setup().await;
    client
        .enable_reauth(unifly_api::session::ReauthCredentials {
            username: "admin".into(),
            password: secrecy::SecretString::from("pw".to_string()),
            totp_provider: None,
            cache: None,
        })
        .await;

    // First call: cookie rejected. Then a login, then the retry succeeds.
    Mock::given(method("GET"))
        .and(path("/api/s/default/stat/health"))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/login"))
        .and(body_json(json!({"username": "admin", "password": "pw"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"meta": {"rc": "ok"}})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/s/default/stat/health"))
        .respond_with(health_ok())
        .expect(1)
        .mount(&server)
        .await;

    let value = client.raw_get("api/s/default/stat/health").await.unwrap();
    assert_eq!(value["meta"]["rc"], "ok");
    server.verify().await;
}

#[tokio::test]
async fn expired_cookie_session_without_credentials_surfaces_error() {
    let (server, client) = setup().await;
    Mock::given(method("GET"))
        .and(path("/api/s/default/stat/health"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/login"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    let error = client
        .raw_get("api/s/default/stat/health")
        .await
        .unwrap_err();
    assert!(matches!(error, Error::SessionExpired), "got {error:?}");
    server.verify().await;
}

#[tokio::test]
async fn failed_relogin_is_rate_limited() {
    let (server, client) = setup().await;
    client
        .enable_reauth(unifly_api::session::ReauthCredentials {
            username: "admin".into(),
            password: secrecy::SecretString::from("wrong".to_string()),
            totp_provider: None,
            cache: None,
        })
        .await;
    Mock::given(method("GET"))
        .and(path("/api/s/default/stat/health"))
        .respond_with(ResponseTemplate::new(401))
        .expect(2)
        .mount(&server)
        .await;
    // Two expired requests in quick succession must produce exactly one
    // login attempt: the second is inside the cooldown after a failure.
    Mock::given(method("POST"))
        .and(path("/api/login"))
        .respond_with(ResponseTemplate::new(401).set_body_string("bad credentials"))
        .expect(1)
        .mount(&server)
        .await;

    for _ in 0..2 {
        let error = client
            .raw_get("api/s/default/stat/health")
            .await
            .unwrap_err();
        assert!(matches!(error, Error::SessionExpired), "got {error:?}");
    }
    server.verify().await;
}

#[tokio::test]
async fn relogin_with_cookie_jar_replaces_stale_cookie_and_retries() {
    let (server, client) = setup_with_stale_cookie().await;
    client
        .enable_reauth(unifly_api::session::ReauthCredentials {
            username: "admin".into(),
            password: secrecy::SecretString::from("pw".to_string()),
            totp_provider: None,
            cache: None,
        })
        .await;

    Mock::given(method("GET"))
        .and(path("/api/s/default/stat/health"))
        .and(wiremock::matchers::header("cookie", "TOKEN=stale"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    // The login must arrive WITHOUT the stale cookie and hands out a new one.
    Mock::given(method("POST"))
        .and(path("/api/login"))
        .and(wiremock::matchers::header_exists("cookie"))
        .respond_with(ResponseTemplate::new(500).set_body_string("stale cookie sent to login"))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/login"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("set-cookie", "TOKEN=fresh; Path=/")
                .set_body_json(json!({"meta": {"rc": "ok"}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/s/default/stat/health"))
        .and(wiremock::matchers::header("cookie", "TOKEN=fresh"))
        .respond_with(health_ok())
        .expect(1)
        .mount(&server)
        .await;

    let value = client.raw_get("api/s/default/stat/health").await.unwrap();
    assert_eq!(value["meta"]["rc"], "ok");
    assert_eq!(client.cookie_header().as_deref(), Some("TOKEN=fresh"));
    server.verify().await;
}

#[tokio::test]
async fn relogin_that_returns_no_cookie_is_treated_as_failure() {
    let (server, client) = setup_with_stale_cookie().await;
    client
        .enable_reauth(unifly_api::session::ReauthCredentials {
            username: "admin".into(),
            password: secrecy::SecretString::from("pw".to_string()),
            totp_provider: None,
            cache: None,
        })
        .await;
    Mock::given(method("GET"))
        .and(path("/api/s/default/stat/health"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    // "Successful" login that never sets a cookie: must not be counted as
    // a recovery, and the request must not be retried with nothing.
    Mock::given(method("POST"))
        .and(path("/api/login"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"meta": {"rc": "ok"}})))
        .expect(1)
        .mount(&server)
        .await;

    let error = client
        .raw_get("api/s/default/stat/health")
        .await
        .unwrap_err();
    assert!(matches!(error, Error::SessionExpired), "got {error:?}");
    server.verify().await;
}

// ── MFA session re-authentication ───────────────────────────────────

/// A provider that hands out `codes` in order, then `None`, and counts how
/// often it was asked.
fn sequence_provider(
    codes: &'static [&'static str],
) -> (
    unifly_api::TotpProvider,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = std::sync::Arc::clone(&calls);
    let provider = unifly_api::TotpProvider::new(move || {
        let index = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        codes
            .get(index)
            .map(|code| secrecy::SecretString::from((*code).to_owned()))
    });
    (provider, calls)
}

/// The 499 challenge a login without a code receives from an MFA account.
fn mfa_challenge() -> ResponseTemplate {
    ResponseTemplate::new(499).set_body_json(json!({
        "errors": ["ubic_2fa_token required"],
        "token": "UBIC_2FA=eyJhbGciOiJIUzI1NiJ9"
    }))
}

#[tokio::test]
async fn provider_backed_relogin_completes_the_mfa_challenge_with_a_fresh_code() {
    let (server, client) = setup_with_stale_cookie().await;
    let (provider, calls) = sequence_provider(&["654321"]);
    client
        .enable_reauth(unifly_api::session::ReauthCredentials {
            username: "admin".into(),
            password: secrecy::SecretString::from("pw".to_string()),
            totp_provider: Some(provider),
            cache: None,
        })
        .await;

    Mock::given(method("GET"))
        .and(path("/api/s/default/stat/health"))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    // The second step carries the provider's code, not anything from before.
    Mock::given(method("POST"))
        .and(path("/api/login"))
        .and(body_json(json!({
            "username": "admin",
            "password": "pw",
            "ubic_2fa_token": "654321"
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("set-cookie", "TOKEN=fresh; Path=/")
                .set_body_json(json!({"meta": {"rc": "ok"}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/login"))
        .and(body_json(json!({"username": "admin", "password": "pw"})))
        .respond_with(mfa_challenge())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/s/default/stat/health"))
        .and(wiremock::matchers::header_regex("cookie", "TOKEN=fresh"))
        .respond_with(health_ok())
        .expect(1)
        .mount(&server)
        .await;

    let value = client.raw_get("api/s/default/stat/health").await.unwrap();
    assert_eq!(value["meta"]["rc"], "ok");
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    server.verify().await;
}

/// A provider that only ever returns the code the connect login used is a
/// static code in disguise. Re-sending that spent code would fail while
/// presenting the correct password, so no login may go out at all.
#[tokio::test]
async fn provider_repeating_the_spent_code_sends_no_login() {
    let (server, client) = setup_with_jar().await;

    Mock::given(method("POST"))
        .and(path("/api/login"))
        .and(wiremock::matchers::body_string_contains("ubic_2fa_token"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("set-cookie", "TOKEN=first; Path=/")
                .set_body_json(json!({})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/login"))
        .respond_with(mfa_challenge())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/s/default/stat/health"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;

    let password = secrecy::SecretString::from("pw".to_string());
    let code = secrecy::SecretString::from("123456".to_string());
    client
        .login("admin", &password, Some(&code))
        .await
        .expect("connect-time MFA login succeeds");

    let (provider, calls) = sequence_provider(&["123456"]);
    client
        .enable_reauth(unifly_api::session::ReauthCredentials {
            username: "admin".into(),
            password,
            totp_provider: Some(provider),
            cache: None,
        })
        .await;

    let error = client
        .raw_get("api/s/default/stat/health")
        .await
        .unwrap_err();
    assert!(matches!(error, Error::SessionExpired), "got {error:?}");
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    server.verify().await;
}
