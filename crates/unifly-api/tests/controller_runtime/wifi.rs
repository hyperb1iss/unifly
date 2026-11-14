use super::*;

use unifly_api::{Command, EntityId, UpdateWifiBroadcastRequest};

const SSID_ID: &str = "7a1d0c7e-52c7-4b7e-9d0a-2f3b9d6a4c11";
const NETWORK_ID: &str = "3f0a6f0c-9f55-4b0e-8a57-0d6f1f2f7b21";

/// One SSID as `GET wifi/broadcasts` lists it: the overview of the
/// 11.0.97 spec, which has no `hideName` and none of the other switches.
fn overview(security: &serde_json::Value) -> serde_json::Value {
    json!({
        "type": "STANDARD", "id": SSID_ID, "name": "Studio", "enabled": true,
        "metadata": {"origin": "USER_DEFINED"},
        "network": {"type": "SPECIFIC", "networkId": NETWORK_ID},
        "securityConfiguration": security,
        "broadcastingFrequenciesGHz": [2.4, 5]
    })
}

/// The same SSID as `GET wifi/broadcasts/{id}` returns it, with every
/// field the spec requires of a standard broadcast and a few it allows.
fn detail(hide_name: bool) -> serde_json::Value {
    json!({
        "type": "STANDARD", "id": SSID_ID, "name": "Studio", "enabled": true,
        "metadata": {"origin": "USER_DEFINED"},
        "network": {"type": "SPECIFIC", "networkId": NETWORK_ID},
        "securityConfiguration": {
            "type": "WPA2_PERSONAL", "passphrase": "the old passphrase",
            "pmfMode": "OPTIONAL", "fastRoamingEnabled": true,
            "groupRekeyIntervalSeconds": 3600
        },
        "broadcastingDeviceFilter": {"type": "DEVICES", "deviceIds": [NETWORK_ID]},
        "broadcastingFrequenciesGHz": [2.4, 5],
        "hideName": hide_name,
        "clientIsolationEnabled": true,
        "multicastToUnicastConversionEnabled": true,
        "uapsdEnabled": false,
        "channel2gLockedTo6": true,
        "dtimPeriod2gLockedTo3": false,
        "advertiseDeviceName": true,
        "arpProxyEnabled": true,
        "bssTransitionEnabled": true,
        "bandSteeringEnabled": true,
        "mloEnabled": false,
        "clientFilteringPolicy": {"action": "BLOCK", "macAddressFilter": ["aa:bb:cc:dd:ee:01"]},
        "dtimPeriodByFrequencyGHzOverride": {"2.4": 1, "5": 3, "6": 3}
    })
}

/// What a write must carry for `detail`: the record without the two
/// fields the create-or-update schema does not have.
fn writable(mut detail: serde_json::Value) -> serde_json::Value {
    let record = detail.as_object_mut().unwrap();
    record.remove("id");
    record.remove("metadata");
    detail
}

fn ssid_id() -> EntityId {
    EntityId::Uuid(uuid::Uuid::parse_str(SSID_ID).unwrap())
}

fn controller(server: &MockServer) -> Controller {
    Controller::new(base_config(
        Url::parse(&server.uri()).unwrap(),
        AuthCredentials::ApiKey(secret("the-key")),
        LEGACY_SITE_NAME,
        false,
    ))
}

async fn mount_detail(server: &MockServer, body: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path(integration_route(&format!(
            "wifi/broadcasts/{SSID_ID}"
        ))))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

/// The body of the one PUT the server received for the SSID.
async fn put_body(server: &MockServer) -> serde_json::Value {
    let requests = server.received_requests().await.unwrap();
    let puts: Vec<_> = requests
        .iter()
        .filter(|request| request.method.as_str() == "PUT")
        .collect();
    assert_eq!(puts.len(), 1, "exactly one write");
    assert!(puts[0].url.path().ends_with(SSID_ID));
    serde_json::from_slice(&puts[0].body).unwrap()
}

#[tokio::test]
async fn a_list_without_hide_name_is_unknown_and_the_detail_decides_the_toggle() {
    let server = MockServer::start().await;
    mock_api_key_with_legacy(&server).await;
    Mock::given(method("GET"))
        .and(path(integration_route("wifi/broadcasts")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(integration_page(&overview(&json!({
                "type": "WPA2_PERSONAL"
            })))),
        )
        .with_priority(1)
        .mount(&server)
        .await;
    mount_detail(&server, detail(true)).await;
    Mock::given(method("PUT"))
        .and(path(integration_route(&format!(
            "wifi/broadcasts/{SSID_ID}"
        ))))
        .respond_with(ResponseTemplate::new(200).set_body_json(detail(false)))
        .expect(1)
        .mount(&server)
        .await;

    let controller = controller(&server);
    controller.connect().await.unwrap();

    let listed = controller.wifi_broadcasts_snapshot();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].hidden, None,
        "the list does not say whether the name is hidden"
    );
    assert_eq!(listed[0].client_isolation, None);
    assert_eq!(listed[0].band_steering, None);
    assert_eq!(listed[0].fast_roaming, None);
    assert_eq!(listed[0].mlo_enabled, None);
    assert_eq!(listed[0].channel2g_locked_to6, None);
    assert!(listed[0].enabled);

    let read = controller
        .get_wifi_broadcast_detail(&ssid_id())
        .await
        .unwrap();
    assert_eq!(read.hidden, Some(true), "the detail does");
    assert_eq!(read.client_isolation, Some(true));
    assert_eq!(read.band_steering, Some(true));
    assert_eq!(read.fast_roaming, Some(true));
    assert_eq!(read.mlo_enabled, Some(false));
    assert_eq!(read.channel2g_locked_to6, Some(true));
    assert_eq!(read.dtim_period2g_locked_to3, Some(false));

    // The toggle flips what was just read: a hidden SSID is shown again.
    let flipped = !read.hidden.unwrap();
    controller
        .execute(Command::UpdateWifiBroadcast {
            id: ssid_id(),
            update: UpdateWifiBroadcastRequest {
                hide_ssid: Some(flipped),
                ..Default::default()
            },
        })
        .await
        .unwrap();

    let mut expected = writable(detail(true));
    expected["hideName"] = json!(false);
    assert_eq!(
        put_body(&server).await,
        expected,
        "only hideName changes; every other field of the record survives"
    );
    controller.disconnect().await;
    server.verify().await;
}

#[tokio::test]
async fn one_field_updates_send_the_whole_record_back_with_that_field_changed() {
    type Change = (UpdateWifiBroadcastRequest, fn(&mut serde_json::Value));
    let changes: [Change; 4] = [
        (
            UpdateWifiBroadcastRequest {
                enabled: Some(false),
                ..Default::default()
            },
            |body| body["enabled"] = json!(false),
        ),
        (
            UpdateWifiBroadcastRequest {
                hide_ssid: Some(true),
                ..Default::default()
            },
            |body| body["hideName"] = json!(true),
        ),
        (
            UpdateWifiBroadcastRequest {
                name: Some("Loft".into()),
                ..Default::default()
            },
            |body| body["name"] = json!("Loft"),
        ),
        (
            UpdateWifiBroadcastRequest {
                passphrase: Some("a brand new one".into()),
                ..Default::default()
            },
            |body| body["securityConfiguration"]["passphrase"] = json!("a brand new one"),
        ),
    ];
    for (update, apply) in changes {
        let server = MockServer::start().await;
        mock_api_key_with_legacy(&server).await;
        mount_detail(&server, detail(false)).await;
        Mock::given(method("PUT"))
            .and(path(integration_route(&format!(
                "wifi/broadcasts/{SSID_ID}"
            ))))
            .respond_with(ResponseTemplate::new(200).set_body_json(detail(false)))
            .expect(1)
            .mount(&server)
            .await;
        let controller = controller(&server);
        controller.connect_lightweight().await.unwrap();

        controller
            .execute(Command::UpdateWifiBroadcast {
                id: ssid_id(),
                update,
            })
            .await
            .unwrap();

        let mut expected = writable(detail(false));
        apply(&mut expected);
        assert_eq!(put_body(&server).await, expected);
        controller.disconnect().await;
        server.verify().await;
    }
}
