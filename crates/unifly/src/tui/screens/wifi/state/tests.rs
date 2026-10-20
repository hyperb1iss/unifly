//! Ordering and selection tests for the WiFi screen state.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::json;
use unifly_api::{Client, Device};

use crate::tui::action::{Action, WifiSortField, WifiSubTab};
use crate::tui::component::Component;
use crate::tui::screens::wifi::WifiScreen;
use crate::tui::screens::wifi::state::CLIENT_DETAIL_RETRY_INTERVAL;
use crate::tui::table::render_lines;

fn access_point(id: &str, name: &str) -> Arc<Device> {
    Arc::new(
        serde_json::from_value(json!({
            "id": id,
            "mac": format!("aa:bb:cc:dd:ee:{id}"),
            "ip": null,
            "wan_ipv6": null,
            "name": name,
            "model": "U7 Pro",
            "device_type": "AccessPoint",
            "state": "Online",
            "firmware_version": null,
            "firmware_updatable": false,
            "adopted_at": null,
            "provisioned_at": null,
            "last_seen": null,
            "serial": null,
            "supported": true,
            "ports": [],
            "radios": [],
            "uplink_device_id": null,
            "uplink_device_mac": null,
            "uplink_port_idx": null,
            "has_switching": false,
            "has_access_point": true,
            "stats": {},
            "client_count": null,
            "origin": null
        }))
        .expect("access point fixture should deserialize"),
    )
}

fn ap_names(screen: &WifiScreen) -> Vec<String> {
    screen
        .ap_devices()
        .iter()
        .map(|device| device.name.clone().unwrap_or_default())
        .collect()
}

#[test]
fn health_ties_fall_back_to_the_cli_name_order() {
    let mut screen = WifiScreen::new();
    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![
        access_point("03", "ap-roof"),
        access_point("01", "AP-Atrium"),
        access_point("02", "ap-beacon"),
    ])));

    assert_eq!(screen.sort_field, WifiSortField::Health);
    assert_eq!(ap_names(&screen), vec!["AP-Atrium", "ap-beacon", "ap-roof"]);
}

#[test]
fn ap_selection_follows_the_ap_across_refreshes() {
    let mut screen = WifiScreen::new();
    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![
        access_point("01", "ap-atrium"),
        access_point("02", "ap-beacon"),
        access_point("03", "ap-roof"),
    ])));
    screen.ap_table_state.select(Some(1));

    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![
        access_point("03", "ap-roof"),
        access_point("04", "ap-aaa"),
        access_point("02", "ap-beacon"),
        access_point("01", "ap-atrium"),
    ])));

    assert_eq!(ap_names(&screen)[0], "ap-aaa");
    assert_eq!(
        screen.selected_ap().and_then(|ap| ap.name.clone()),
        Some("ap-beacon".to_owned())
    );
}

fn wireless_client(id: u128, ap_mac: &str, satisfaction: u8) -> Arc<Client> {
    Arc::new(
        serde_json::from_value(json!({
            "id": uuid::Uuid::from_u128(id).to_string(),
            "mac": format!("cc:00:00:00:00:{id:02x}"),
            "ip": format!("10.40.10.{id}"),
            "name": format!("client-{id}"),
            "hostname": null,
            "client_type": "Wireless",
            "connected_at": null,
            "uplink_device_id": null,
            "uplink_device_mac": ap_mac,
            "network_id": null,
            "vlan": null,
            "wireless": {
                "ssid": "Studio", "bssid": null, "channel": 36, "frequency_ghz": 5.0,
                "signal_dbm": -55, "noise_dbm": null, "satisfaction": satisfaction,
                "tx_rate_kbps": null, "rx_rate_kbps": null
            },
            "guest_auth": null,
            "is_guest": false,
            "tx_bytes": null,
            "rx_bytes": null,
            "bandwidth": null,
            "os_name": null,
            "device_class": null,
            "use_fixedip": false,
            "fixed_ip": null,
            "blocked": false
        }))
        .expect("client fixture should deserialize"),
    )
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn ap_selection_survives_the_first_client_snapshot() {
    let mut screen = WifiScreen::new();
    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![
        access_point("01", "ap-a"),
        access_point("02", "ap-b"),
    ])));
    let _ = screen.handle_key_input(key(KeyCode::Char('j')));
    assert_eq!(screen.ap_table_state.selected(), Some(1));
    assert_eq!(
        screen.selected_ap().and_then(|ap| ap.name.clone()),
        Some("ap-b".to_owned())
    );

    // A client joins ap-b with health 90, so health order becomes [b, a].
    screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![wireless_client(
        1,
        "aa:bb:cc:dd:ee:02",
        90,
    )])));

    assert_eq!(ap_names(&screen), vec!["ap-b", "ap-a"]);
    assert_eq!(screen.ap_table_state.selected(), Some(0));
    assert!(matches!(
        screen.handle_key_input(key(KeyCode::Char('R'))),
        Some(Action::RequestRestart(id)) if id == access_point("02", "ap-b").id
    ));
}

#[test]
fn sorting_from_the_keyboard_keeps_the_selected_ap() {
    let mut screen = WifiScreen::new();
    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![
        access_point("01", "ap-a"),
        access_point("02", "ap-b"),
        access_point("03", "ap-c"),
    ])));
    // ap-a: three clients at 80; ap-b: one client at 95; ap-c: none.
    screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![
        wireless_client(1, "aa:bb:cc:dd:ee:01", 80),
        wireless_client(2, "aa:bb:cc:dd:ee:01", 80),
        wireless_client(3, "aa:bb:cc:dd:ee:01", 80),
        wireless_client(4, "aa:bb:cc:dd:ee:02", 95),
    ])));
    let _ = screen.handle_key_input(key(KeyCode::Char('g')));
    assert_eq!(ap_names(&screen), vec!["ap-b", "ap-a", "ap-c"]);
    assert_eq!(screen.ap_table_state.selected(), Some(0));

    let sort = screen.handle_key_input(key(KeyCode::Char('s')));
    assert_eq!(
        screen.sort_field,
        WifiSortField::Health,
        "key must not sort"
    );
    let sort = sort.expect("s yields a sort action");
    screen.apply_action(&sort);

    assert_eq!(screen.sort_field, WifiSortField::Clients);
    assert_eq!(ap_names(&screen), vec!["ap-a", "ap-b", "ap-c"]);
    assert_eq!(screen.ap_table_state.selected(), Some(1));
    assert_eq!(
        screen.selected_ap().and_then(|ap| ap.name.clone()),
        Some("ap-b".to_owned())
    );
}

const FIRST_IP: &str = "10.40.10.1";
const SECOND_IP: &str = "10.40.10.2";

/// The Clients tab with two clients on one AP, the first selected and its
/// detail open (and requested).
fn client_detail_screen() -> WifiScreen {
    let mut screen = WifiScreen::new();
    screen.focused = true;
    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![access_point(
        "01", "ap-a",
    )])));
    screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![
        wireless_client(1, "aa:bb:cc:dd:ee:01", 80),
        wireless_client(2, "aa:bb:cc:dd:ee:01", 70),
    ])));
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Clients));
    let request = screen.handle_key_input(key(KeyCode::Enter));
    assert!(
        matches!(request, Some(Action::RequestWifiClientDetail(ref ip)) if ip == FIRST_IP),
        "{request:?}"
    );
    screen
}

fn detail_loaded(ip: &str, signal: i32) -> Action {
    Action::WifiClientDetailLoaded {
        ip: ip.to_owned(),
        data: Arc::new(json!({ "signal": signal })),
    }
}

fn rendered(screen: &WifiScreen) -> String {
    render_lines(120, 40, |frame, area| screen.render(frame, area)).join("\n")
}

#[test]
fn the_client_detail_shows_only_the_selected_clients_data() {
    let mut screen = client_detail_screen();
    screen.apply_action(&detail_loaded(FIRST_IP, -42));
    assert!(rendered(&screen).contains("-42 dBm"));

    screen.handle_key_input(key(KeyCode::Char('j')));

    let shown = rendered(&screen);
    assert!(!shown.contains("-42 dBm"), "{shown}");
    assert!(shown.contains("Loading WiFi detail"), "{shown}");
}

#[test]
fn a_late_detail_for_the_previous_client_leaves_the_new_request_in_flight() {
    let mut screen = client_detail_screen();
    screen.handle_key_input(key(KeyCode::Char('j')));
    let request = screen.apply_action(&Action::Tick);
    assert!(
        matches!(request, Some(Action::RequestWifiClientDetail(ref ip)) if ip == SECOND_IP),
        "{request:?}"
    );

    screen.apply_action(&detail_loaded(FIRST_IP, -42));

    let again = screen.apply_action(&Action::Tick);
    assert!(
        again.is_none(),
        "requested the second client twice: {again:?}"
    );
}

#[test]
fn a_failed_client_detail_fetch_is_retried() {
    let mut screen = client_detail_screen();
    let failed = || Action::WifiClientDetailFailed {
        ip: FIRST_IP.to_owned(),
        error: "wifiman unavailable".to_owned(),
    };

    let notice = screen.apply_action(&failed());
    assert!(matches!(notice, Some(Action::Notify(_))), "{notice:?}");
    assert!(rendered(&screen).contains("WiFi detail unavailable"));
    assert!(
        screen.apply_action(&Action::Tick).is_none(),
        "a failure is not retried at once"
    );

    screen.client_detail_failure = screen.client_detail_failure.take().map(|(ip, at)| {
        let earlier = at.checked_sub(CLIENT_DETAIL_RETRY_INTERVAL);
        (
            ip,
            earlier.expect("the clock reaches back one retry interval"),
        )
    });
    let retry = screen.apply_action(&Action::Tick);
    assert!(
        matches!(retry, Some(Action::RequestWifiClientDetail(ref ip)) if ip == FIRST_IP),
        "{retry:?}"
    );

    let repeat = screen.apply_action(&failed());
    assert!(
        repeat.is_none(),
        "a repeated failure toasts again: {repeat:?}"
    );
    screen.apply_action(&detail_loaded(FIRST_IP, -42));
    assert!(rendered(&screen).contains("-42 dBm"));
}

#[test]
fn a_kept_search_filter_stays_visible() {
    let mut screen = client_detail_screen();
    screen.apply_action(&Action::SearchInput("client-2".to_owned()));
    assert!(rendered(&screen).contains("Filter: \"client-2\""));

    // The roaming history is not filtered, so its tab claims no filter.
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Roaming));
    assert!(!rendered(&screen).contains("Filter:"));
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Clients));

    screen.apply_action(&Action::CloseSearch);
    assert!(!rendered(&screen).contains("Filter:"));
}
