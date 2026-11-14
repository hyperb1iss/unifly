//! What the device and client commands send to a controller, and what
//! the refresh reads back about clients and pending devices.

use super::*;

use unifly_api::{Command, EntityId};

const CLIENT_MAC: &str = "aa:bb:cc:dd:ee:ff";
/// The Integration ID of the Studio network, as `networks list` gives it.
const STUDIO_UUID: &str = "6f1c1f4e-9a1c-4c55-9f0e-2b8f6f9c0a11";
/// The same network's `rest/networkconf` `_id`.
const STUDIO_SESSION_ID: &str = "5f1a00000000000000000001";
const GUEST_SESSION_ID: &str = "5f1a00000000000000000002";

fn network_conf() -> serde_json::Value {
    legacy_envelope(&json!([
        {"_id": "5f1a000000000000000000ff", "name": "Internet", "purpose": "wan"},
        {"_id": STUDIO_SESSION_ID, "name": "Studio", "purpose": "corporate",
            "ip_subnet": "10.0.0.1/24", "external_id": STUDIO_UUID},
        {"_id": GUEST_SESSION_ID, "name": "Guest", "purpose": "guest",
            "ip_subnet": "10.0.60.1/23"},
    ]))
}

/// Mount a Session GET that answers ahead of the connect defaults.
async fn mount_get(server: &MockServer, route: &str, body: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path(route))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .with_priority(1)
        .mount(server)
        .await;
}

/// Accept `verb` on `route`, answering an empty envelope.
async fn mount_write(server: &MockServer, verb: &str, route: &str) {
    Mock::given(method(verb))
        .and(path(route))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_legacy_envelope()))
        .mount(server)
        .await;
}

/// The JSON bodies of every `verb` request whose path ends in `suffix`.
async fn bodies(server: &MockServer, verb: &str, suffix: &str) -> Vec<serde_json::Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.method.as_str() == verb && request.url.path().ends_with(suffix))
        .map(|request| serde_json::from_slice(&request.body).unwrap())
        .collect()
}

async fn api_key_controller(server: &MockServer) -> Controller {
    let controller = Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("the-key")),
        LEGACY_SITE_NAME,
        false,
    ));
    controller.connect().await.unwrap();
    controller
}

fn set_fixed_ip(ip: [u8; 4], network: Option<&str>) -> Command {
    Command::SetClientFixedIp {
        mac: MacAddress::new(CLIENT_MAC),
        ip: ip.into(),
        network_id: network.map(EntityId::from),
    }
}

#[tokio::test]
async fn a_fixed_ip_is_written_under_the_session_network_id() {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;
    let api = "/proxy/network/api/s/default";
    mount_get(&server, &format!("{api}/rest/networkconf"), network_conf()).await;
    // The client is known, with a name and a note the reservation must
    // leave alone, and a stale reservation on another network.
    mount_get(
        &server,
        &format!("{api}/rest/user"),
        legacy_envelope(&json!([{
            "_id": "user-1", "mac": CLIENT_MAC, "name": "Office Laptop",
            "note": "front desk", "use_fixedip": true, "fixed_ip": "10.0.60.9",
            "network_id": GUEST_SESSION_ID,
        }])),
    )
    .await;
    mount_write(&server, "PUT", &format!("{api}/rest/user/user-1")).await;
    let controller = api_key_controller(&server).await;

    // Named by the Integration ID: the body carries the networkconf
    // `_id`, and only the three reservation fields, so the controller
    // keeps the rest of the record.
    controller
        .execute(set_fixed_ip([10, 0, 0, 60], Some(STUDIO_UUID)))
        .await
        .unwrap();
    let expected = json!({
        "use_fixedip": true, "fixed_ip": "10.0.0.60", "network_id": STUDIO_SESSION_ID,
    });
    assert_eq!(
        bodies(&server, "PUT", "/rest/user/user-1").await,
        std::slice::from_ref(&expected)
    );

    // With no network named, the subnet holding the address picks it.
    controller
        .execute(set_fixed_ip([10, 0, 0, 60], None))
        .await
        .unwrap();
    assert_eq!(
        bodies(&server, "PUT", "/rest/user/user-1").await,
        [expected.clone(), expected]
    );

    // An Integration ID no record carries is refused, never written.
    let unknown = controller
        .execute(set_fixed_ip(
            [10, 0, 0, 60],
            Some("6f1c1f4e-9a1c-4c55-9f0e-2b8f6f9c0aff"),
        ))
        .await;
    assert!(
        matches!(unknown, Err(CoreError::NetworkNotFound { .. })),
        "{unknown:?}"
    );
    let stray = controller.execute(set_fixed_ip([192, 0, 2, 9], None)).await;
    assert!(
        matches!(&stray, Err(CoreError::ValidationFailed { message })
            if message.contains("not inside any network's subnet")),
        "{stray:?}"
    );
    assert_eq!(bodies(&server, "PUT", "/rest/user/user-1").await.len(), 2);

    controller.disconnect().await;
}

#[tokio::test]
async fn an_unchanged_reservation_is_not_written_again() {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;
    let api = "/proxy/network/api/s/default";
    mount_get(&server, &format!("{api}/rest/networkconf"), network_conf()).await;
    mount_get(
        &server,
        &format!("{api}/rest/user"),
        legacy_envelope(&json!([{
            "_id": "user-1", "mac": CLIENT_MAC, "use_fixedip": true,
            "fixed_ip": "10.0.0.60", "network_id": STUDIO_SESSION_ID,
        }])),
    )
    .await;
    mount_write(&server, "PUT", &format!("{api}/rest/user/user-1")).await;
    let controller = api_key_controller(&server).await;

    controller
        .execute(set_fixed_ip([10, 0, 0, 60], None))
        .await
        .unwrap();
    assert_eq!(bodies(&server, "PUT", "/rest/user/user-1").await.len(), 0);

    // Another address on the same network is a change.
    controller
        .execute(set_fixed_ip([10, 0, 0, 61], None))
        .await
        .unwrap();
    assert_eq!(bodies(&server, "PUT", "/rest/user/user-1").await.len(), 1);

    controller.disconnect().await;
}

#[tokio::test]
async fn a_session_only_profile_reserves_by_subnet_and_creates_the_user() {
    let server = MockServer::start().await;
    mock_legacy_connect(&server, legacy_site_envelope()).await;
    mount_get(&server, "/api/s/default/rest/networkconf", network_conf()).await;
    mount_get(&server, "/api/s/default/rest/user", empty_legacy_envelope()).await;
    mount_write(&server, "POST", "/api/s/default/rest/user").await;
    let controller = legacy_controller(&server);
    controller.connect().await.unwrap();
    assert!(
        controller.networks_snapshot().is_empty(),
        "a Session-only store lists no networks to pick from"
    );

    controller
        .execute(set_fixed_ip([10, 0, 61, 20], None))
        .await
        .unwrap();
    // A network can also be named the way the controller's UI shows it.
    controller
        .execute(set_fixed_ip([10, 0, 0, 20], Some("studio")))
        .await
        .unwrap();

    assert_eq!(
        bodies(&server, "POST", "/rest/user").await,
        [
            json!({"mac": CLIENT_MAC, "use_fixedip": true, "fixed_ip": "10.0.61.20",
                "network_id": GUEST_SESSION_ID}),
            json!({"mac": CLIENT_MAC, "use_fixedip": true, "fixed_ip": "10.0.0.20",
                "network_id": STUDIO_SESSION_ID}),
        ]
    );

    controller.disconnect().await;
}

#[tokio::test]
async fn removing_a_fixed_ip_on_a_named_network_matches_its_session_id() {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;
    let api = "/proxy/network/api/s/default";
    mount_get(&server, &format!("{api}/rest/networkconf"), network_conf()).await;
    mount_get(
        &server,
        &format!("{api}/rest/user"),
        legacy_envelope(&json!([{
            "_id": "user-1", "mac": CLIENT_MAC, "use_fixedip": true,
            "fixed_ip": "10.0.0.60", "network_id": STUDIO_SESSION_ID,
        }])),
    )
    .await;
    mount_write(&server, "PUT", &format!("{api}/rest/user/user-1")).await;
    let controller = api_key_controller(&server).await;

    controller
        .execute(Command::RemoveClientFixedIp {
            mac: MacAddress::new(CLIENT_MAC),
            network_id: Some(EntityId::from(STUDIO_UUID)),
        })
        .await
        .unwrap();

    assert_eq!(
        bodies(&server, "PUT", "/rest/user/user-1").await,
        [json!({"use_fixedip": false})]
    );
    controller.disconnect().await;
}

#[tokio::test]
async fn guest_and_blocked_state_reach_clients_on_an_api_key_profile() {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;
    let api = "/proxy/network/api/s/default";
    let client = |id: &str, name: &str, mac: &str, ip: &str, access: serde_json::Value| {
        json!({"id": id, "name": name, "type": "WIRELESS", "ipAddress": ip,
            "macAddress": mac, "access": access})
    };
    mount_get(
        &server,
        &integration_route("clients"),
        json!({"offset": 0, "limit": 200, "count": 3, "totalCount": 3, "data": [
            // Integration says guest, not yet through the portal.
            client("c56a4180-65aa-42ec-a945-5fd21dec0001", "Visitor phone",
                "aa:bb:cc:00:00:01", "10.0.60.21", json!({"type": "GUEST", "authorized": false})),
            // Integration reports nothing; only the station list knows.
            client("c56a4180-65aa-42ec-a945-5fd21dec0002", "Lobby tablet",
                "aa:bb:cc:00:00:02", "10.0.60.22", json!({})),
            client("c56a4180-65aa-42ec-a945-5fd21dec0003", "Kiosk",
                "aa:bb:cc:00:00:03", "10.0.0.23", json!({"type": "DEFAULT"})),
        ]}),
    )
    .await;
    // Station addresses differ from Integration's on purpose: the MAC is
    // what ties the two lists together.
    mount_get(
        &server,
        &format!("{api}/stat/sta"),
        legacy_envelope(&json!([
            {"_id": "sta-1", "mac": "AA:BB:CC:00:00:01", "ip": "10.0.60.91",
                "is_guest": true, "authorized": true},
            {"_id": "sta-2", "mac": "aa:bb:cc:00:00:02", "ip": "10.0.60.92",
                "is_guest": true, "authorized": false},
            {"_id": "sta-3", "mac": "aa:bb:cc:00:00:03", "ip": "10.0.0.93"},
        ])),
    )
    .await;
    mount_get(
        &server,
        &format!("{api}/rest/user"),
        legacy_envelope(&json!([
            {"_id": "user-3", "mac": "aa:bb:cc:00:00:03", "blocked": true},
        ])),
    )
    .await;
    let controller = api_key_controller(&server).await;
    let stored = |mac: &str| {
        controller
            .store()
            .client_by_mac(&MacAddress::new(mac))
            .expect("client present")
    };
    let authorized = |mac: &str| stored(mac).guest_auth.as_ref().map(|auth| auth.authorized);

    assert!(stored("aa:bb:cc:00:00:01").is_guest);
    assert_eq!(
        authorized("aa:bb:cc:00:00:01"),
        Some(true),
        "the station list is where an authorization lands"
    );
    assert!(stored("aa:bb:cc:00:00:02").is_guest);
    assert_eq!(authorized("aa:bb:cc:00:00:02"), Some(false));
    let kiosk = stored("aa:bb:cc:00:00:03");
    assert!(!kiosk.is_guest && kiosk.guest_auth.is_none());
    assert!(kiosk.blocked, "the user record says it is blocked");
    assert!(!stored("aa:bb:cc:00:00:01").blocked);

    controller.disconnect().await;
}

#[tokio::test]
async fn block_unblock_and_kick_go_to_the_station_manager_by_mac() {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;
    mount_write(&server, "POST", "/proxy/network/api/s/default/cmd/stamgr").await;
    let controller = api_key_controller(&server).await;
    // A blocked client is off the connected list, so the store has no
    // record of this MAC and no Integration ID to act on.
    let gone = MacAddress::new("aa:bb:cc:00:00:99");
    assert!(controller.store().client_by_mac(&gone).is_none());

    controller
        .execute(Command::UnblockClient { mac: gone.clone() })
        .await
        .unwrap();
    controller
        .execute(Command::BlockClient {
            mac: MacAddress::new(CLIENT_MAC),
        })
        .await
        .unwrap();
    controller
        .execute(Command::KickClient {
            mac: MacAddress::new(CLIENT_MAC),
        })
        .await
        .unwrap();

    assert_eq!(
        bodies(&server, "POST", "/cmd/stamgr").await,
        [
            json!({"cmd": "unblock-sta", "mac": "aa:bb:cc:00:00:99"}),
            json!({"cmd": "block-sta", "mac": CLIENT_MAC}),
            json!({"cmd": "kick-sta", "mac": CLIENT_MAC}),
        ]
    );
    let integration_actions = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.url.path().ends_with("/actions"))
        .count();
    assert_eq!(
        integration_actions, 0,
        "the Integration API documents no block or reconnect action"
    );

    controller.disconnect().await;
}
