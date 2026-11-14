//! Behavior tests for the WiFi screen: ordering, selection, focus, search,
//! fetch lifecycle, and what each sub-tab says when it has nothing to show.

use std::sync::Arc;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::json;
use unifly_api::session_models::RogueAp;
use unifly_api::{Client, Device};

use super::fetch::{CLIENT_DETAIL_REFRESH_INTERVAL, CURSOR_SETTLE, RETRY_INTERVAL};
use super::{OverviewPanel, WifiScreen};
use crate::tui::action::{Access, Action, WifiBand, WifiSortField, WifiSubTab};
use crate::tui::component::{Component, LIST_HELP};
use crate::tui::table::render_lines;
use crate::tui::widgets::click::hint_text;

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
            "firmware_version": "8.1.113",
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

fn with<T: Clone>(entity: &Arc<T>, change: impl FnOnce(&mut T)) -> Arc<T> {
    let mut entity = (**entity).clone();
    change(&mut entity);
    Arc::new(entity)
}

fn ap_names(screen: &WifiScreen) -> Vec<String> {
    screen
        .ap_devices()
        .iter()
        .map(|device| device.name.clone().unwrap_or_default())
        .collect()
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

fn neighbor(bssid: &str, essid: Option<&str>, seen_by: &str, signal: i32) -> RogueAp {
    serde_json::from_value(json!({
        "bssid": bssid, "essid": essid, "channel": 36, "signal": signal,
        "radio": "na", "security": "wpa2", "age": 42,
        "ap_mac": format!("aa:bb:cc:dd:ee:{seen_by}")
    }))
    .expect("neighbor fixture should deserialize")
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn press(screen: &mut WifiScreen, ch: char) -> Option<Action> {
    let action = screen.handle_key_input(key(KeyCode::Char(ch)));
    if let Some(action) = &action {
        screen.apply_action(action);
    }
    action
}

fn full_access() -> Action {
    Action::AccessUpdated(Access {
        integration: true,
        session: true,
        live_events: true,
    })
}

fn cloud_access() -> Action {
    Action::AccessUpdated(Access {
        integration: true,
        session: false,
        live_events: false,
    })
}

const AP_A: &str = "aa:bb:cc:dd:ee:01";
const AP_B: &str = "aa:bb:cc:dd:ee:02";

/// Two APs, three clients on the first and one on the second, connected
/// with full access and showing.
fn site() -> WifiScreen {
    let mut screen = WifiScreen::new();
    screen.focused = true;
    screen.apply_action(&full_access());
    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![
        access_point("01", "ap-a"),
        access_point("02", "ap-b"),
    ])));
    screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![
        wireless_client(1, AP_A, 80),
        wireless_client(2, AP_A, 70),
        wireless_client(3, AP_A, 60),
        wireless_client(4, AP_B, 95),
    ])));
    screen
}

fn rendered_at(screen: &WifiScreen, width: u16, height: u16) -> String {
    render_lines(width, height, |frame, area| screen.render(frame, area)).join("\n")
}

fn rendered(screen: &WifiScreen) -> String {
    rendered_at(screen, 120, 40)
}

/// What the status bar would offer, each hint as "key label".
fn hints(screen: &WifiScreen) -> String {
    hint_text(&screen.hints())
}

// ── Ordering and selection ────────────────────────────────────────────

#[test]
fn health_ties_fall_back_to_the_cli_name_order() {
    let mut screen = WifiScreen::new();
    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![
        access_point("03", "ap-roof"),
        access_point("01", "AP-Atrium"),
        access_point("02", "ap-beacon"),
    ])));

    assert_eq!(screen.ap_sort, WifiSortField::Health);
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

#[test]
fn ap_selection_survives_the_first_client_snapshot() {
    let mut screen = WifiScreen::new();
    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![
        access_point("01", "ap-a"),
        access_point("02", "ap-b"),
    ])));
    let _ = screen.handle_key_input(key(KeyCode::Char('j')));
    assert_eq!(screen.ap_table_state.selected(), Some(1));

    // A client joins ap-b with health 90, so health order becomes [b, a].
    screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![wireless_client(
        1, AP_B, 90,
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
    let mut screen = site();
    let _ = screen.handle_key_input(key(KeyCode::Char('g')));
    assert_eq!(ap_names(&screen), vec!["ap-b", "ap-a"]);

    let sort = screen.handle_key_input(key(KeyCode::Char('s')));
    assert_eq!(screen.ap_sort, WifiSortField::Health, "key must not sort");
    screen.apply_action(&sort.expect("s yields a sort action"));

    assert_eq!(screen.ap_sort, WifiSortField::Clients);
    assert_eq!(ap_names(&screen), vec!["ap-a", "ap-b"]);
    assert_eq!(
        screen.selected_ap().and_then(|ap| ap.name.clone()),
        Some("ap-b".to_owned())
    );
}

#[test]
fn each_sub_tab_keeps_its_own_sort() {
    let mut screen = site();
    press(&mut screen, 's');
    assert_eq!(screen.ap_sort, WifiSortField::Clients);

    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Neighbors));
    press(&mut screen, 's');
    assert_eq!(screen.neighbor_sort, WifiSortField::Channel);
    assert_eq!(
        screen.ap_sort,
        WifiSortField::Clients,
        "untouched by Neighbors"
    );
    assert_eq!(screen.client_sort, WifiSortField::Health);

    // Roaming is always newest first: `s` is not its key.
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Roaming));
    assert!(screen.handle_key_input(key(KeyCode::Char('s'))).is_none());
}

#[test]
fn the_sorted_column_carries_an_arrow() {
    let mut screen = site();
    assert!(rendered(&screen).contains("Health \u{25BE}"));

    for _ in 0..3 {
        press(&mut screen, 's');
    }
    assert_eq!(screen.ap_sort, WifiSortField::Name);
    let shown = rendered(&screen);
    assert!(shown.contains("AP \u{25B4}"), "{shown}");
    assert!(!shown.contains("Health \u{25BE}"), "{shown}");
}

#[test]
fn neighbors_sort_by_name_and_every_step_changes_the_order() {
    let mut screen = site();
    screen.apply_action(&Action::WifiBandSelect(WifiBand::FiveGhz));
    let on = |bssid: &str, essid: &str, channel: i32, signal: i32, security: &str| {
        let mut neighbor = neighbor(bssid, Some(essid), "01", signal);
        neighbor.channel = Some(channel);
        neighbor.security = Some(security.to_owned());
        neighbor
    };
    screen.apply_action(&Action::WifiNeighborsUpdated(Arc::new(vec![
        on("11:11:11:11:11:11", "zeta", 149, -40, "wpa3"),
        on("22:22:22:22:22:22", "Alpha", 100, -70, "wpa2"),
        on("33:33:33:33:33:33", "mid", 36, -55, "open"),
    ])));
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Neighbors));
    let order = |screen: &WifiScreen| -> Vec<String> {
        screen
            .visible_neighbors()
            .iter()
            .map(|neighbor| neighbor.essid.clone().unwrap_or_default())
            .collect()
    };

    assert_eq!(order(&screen), ["zeta", "mid", "Alpha"], "signal first");
    press(&mut screen, 's');
    assert_eq!(order(&screen), ["mid", "Alpha", "zeta"], "channel");
    press(&mut screen, 's');
    assert_eq!(order(&screen), ["mid", "Alpha", "zeta"], "security");
    assert_eq!(screen.neighbor_sort, WifiSortField::Security);
    press(&mut screen, 's');
    assert_eq!(order(&screen), ["Alpha", "mid", "zeta"], "name");
    assert!(rendered(&screen).contains("SSID \u{25B4}"));
}

// ── AP focus ──────────────────────────────────────────────────────────

#[test]
fn moving_the_cursor_does_not_narrow_the_other_tabs() {
    let mut screen = site();
    screen.handle_key_input(key(KeyCode::Char('j')));
    screen.handle_key_input(key(KeyCode::Char('k')));

    assert!(screen.focused_ap.is_none());
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Clients));
    assert_eq!(screen.wireless_clients().len(), 4);
}

#[test]
fn space_focuses_the_selected_ap_as_a_chip_that_esc_clears() {
    let mut screen = site();
    let _ = screen.handle_key_input(key(KeyCode::Char('G')));
    assert_eq!(
        screen.selected_ap().and_then(|ap| ap.name.clone()),
        Some("ap-a".to_owned())
    );

    press(&mut screen, ' ');
    assert!(rendered(&screen).contains("focus ap-a \u{2715}"));
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Clients));
    assert_eq!(screen.wireless_clients().len(), 3, "only ap-a's clients");
    let shown = rendered(&screen);
    assert!(shown.contains("focus ap-a \u{2715}"), "{shown}");
    assert!(
        hints(&screen).ends_with("Esc all APs"),
        "{}",
        hints(&screen)
    );

    let cleared = screen.handle_key_input(key(KeyCode::Esc));
    screen.apply_action(&cleared.expect("Esc clears the focus"));
    assert!(screen.focused_ap.is_none());
    assert_eq!(screen.wireless_clients().len(), 4);
    assert!(
        screen.handle_key_input(key(KeyCode::Esc)).is_none(),
        "nothing left to close"
    );
}

#[test]
fn space_on_the_focused_ap_clears_the_focus() {
    let mut screen = site();
    press(&mut screen, ' ');
    assert!(screen.focused_ap.is_some());
    press(&mut screen, ' ');
    assert!(screen.focused_ap.is_none());
}

#[test]
fn the_focus_survives_a_filter_that_hides_its_ap() {
    let mut screen = site();
    press(&mut screen, ' ');
    let focused = screen.focused_ap.as_ref().map(|focus| focus.label.clone());

    screen.apply_action(&Action::SearchInput("no such ap".into()));
    assert!(screen.ap_devices().is_empty());
    assert_eq!(
        screen.focused_ap.as_ref().map(|focus| focus.label.clone()),
        focused
    );
}

// ── Panels and Esc ────────────────────────────────────────────────────

#[test]
fn esc_closes_only_what_the_sub_tab_shows() {
    let mut screen = site();
    press(&mut screen, 'c');
    assert_eq!(screen.overview_panel, OverviewPanel::Channels);

    // The channel map belongs to Overview: Roaming has nothing to close.
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Roaming));
    assert!(screen.handle_key_input(key(KeyCode::Esc)).is_none());
    assert_eq!(screen.overview_panel, OverviewPanel::Channels);

    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Overview));
    assert!(screen.handle_key_input(key(KeyCode::Esc)).is_some());
    assert_eq!(screen.overview_panel, OverviewPanel::None);
    assert!(screen.handle_key_input(key(KeyCode::Esc)).is_none());
}

#[test]
fn the_overview_shows_one_panel_at_a_time() {
    let mut screen = site();
    screen.handle_key_input(key(KeyCode::Enter));
    assert_eq!(screen.overview_panel, OverviewPanel::Detail);

    press(&mut screen, 'c');
    assert_eq!(screen.overview_panel, OverviewPanel::Channels);
    let shown = rendered(&screen);
    assert!(shown.contains(" Channels "), "{shown}");
    assert!(shown.contains("[/] band 2.4 GHz"), "{shown}");

    screen.handle_key_input(key(KeyCode::Enter));
    assert_eq!(screen.overview_panel, OverviewPanel::Detail);
    screen.handle_key_input(key(KeyCode::Esc));
    assert_eq!(
        screen.overview_panel,
        OverviewPanel::None,
        "one Esc, nothing hidden behind it"
    );
}

#[test]
fn the_ap_detail_does_not_open_the_client_detail() {
    let mut screen = site();
    screen.handle_key_input(key(KeyCode::Enter));
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Clients));

    assert!(!screen.client_detail_open);
    assert!(
        screen.apply_action(&Action::Tick).is_none(),
        "no detail fetch"
    );
}

#[test]
fn band_keys_work_only_where_a_band_is_shown() {
    let mut screen = site();
    assert!(
        screen.handle_key_input(key(KeyCode::Char(']'))).is_none(),
        "no channel map, no band to change"
    );
    press(&mut screen, 'c');
    press(&mut screen, ']');
    assert_eq!(screen.selected_band, WifiBand::FiveGhz);

    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Clients));
    assert!(screen.handle_key_input(key(KeyCode::Char(']'))).is_none());
}

// ── Health banner and AP detail ───────────────────────────────────────

#[test]
fn the_banner_claims_nothing_before_data_arrives() {
    let mut screen = WifiScreen::new();
    assert_eq!(screen.health_summary().0, "Waiting for access point data");

    screen.apply_action(&Action::DevicesUpdated(Arc::new(Vec::new())));
    let (message, color) = screen.health_summary();
    assert_eq!(message, "No access points on this site");
    assert!(color.is_none());

    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![access_point(
        "01", "ap-a",
    )])));
    let (message, color) = screen.health_summary();
    assert!(message.contains("no client health to report"), "{message}");
    assert!(color.is_none(), "an AP without clients is not 'healthy'");
}

#[test]
fn the_banner_counts_aps_that_are_not_online() {
    let mut screen = site();
    assert!(screen.health_summary().0.contains("1 needs attention"));

    let offline = with(&access_point("03", "ap-c"), |ap| {
        ap.state = unifly_api::DeviceState::Offline;
    });
    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![
        access_point("01", "ap-a"),
        access_point("02", "ap-b"),
        offline,
    ])));

    let (message, _) = screen.health_summary();
    assert!(message.contains("3 access points"), "{message}");
    assert!(message.contains("1 not online"), "{message}");
    assert!(rendered(&screen).contains("offline"));
}

#[test]
fn an_ap_without_clients_has_no_score_in_its_detail() {
    let mut screen = WifiScreen::new();
    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![access_point(
        "01", "ap-a",
    )])));
    screen.handle_key_input(key(KeyCode::Enter));

    let shown = rendered(&screen);
    assert!(shown.contains("no clients connected"), "{shown}");
    assert!(!shown.contains("0/100"), "{shown}");
    assert!(!shown.contains("Retries: High"), "{shown}");
}

#[test]
fn the_ap_detail_reports_each_radios_own_numbers() {
    let mut screen = site();
    let radios: Vec<unifly_api::model::Radio> = serde_json::from_value(json!([
        { "frequency_ghz": 2.4, "channel": 6, "channel_width_mhz": 20,
          "wlan_standard": "802.11ax", "tx_retries_pct": 12.4,
          "channel_utilization_pct": 71.0 },
        { "frequency_ghz": 5.0, "channel": 36, "channel_width_mhz": 80,
          "wlan_standard": "802.11be", "tx_retries_pct": 1.6,
          "channel_utilization_pct": 9.0 }
    ]))
    .expect("radio fixture should deserialize");
    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![
        with(&access_point("01", "ap-a"), |ap| ap.radios = radios),
        access_point("02", "ap-b"),
    ])));
    let _ = screen.handle_key_input(key(KeyCode::Char('G')));
    screen.handle_key_input(key(KeyCode::Enter));

    let shown = rendered_at(&screen, 160, 40);
    for expected in [
        "ch 6 \u{00B7} 20 MHz",
        "busy 71%",
        "retries 12%",
        "ch 36 \u{00B7} 80 MHz",
        "busy 9%",
        "retries 2%",
        "Clients (3)",
    ] {
        assert!(shown.contains(expected), "missing {expected:?}:\n{shown}");
    }
}

#[test]
fn the_ap_detail_counts_the_clients_it_has_no_room_for() {
    let mut screen = site();
    let clients: Vec<_> = (1..=30).map(|id| wireless_client(id, AP_A, 80)).collect();
    screen.apply_action(&Action::ClientsUpdated(Arc::new(clients)));
    screen.handle_key_input(key(KeyCode::Enter));

    let shown = rendered_at(&screen, 120, 24);
    assert!(shown.contains("Clients (30)"), "{shown}");
    assert!(shown.contains(" more"), "{shown}");
}

// ── Search ────────────────────────────────────────────────────────────

#[test]
fn a_filter_belongs_to_the_sub_tab_it_was_typed_on() {
    let mut screen = site();
    press(&mut screen, ' ');
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Clients));
    screen.apply_action(&Action::SearchInput("client-2".to_owned()));

    assert_eq!(screen.wireless_clients().len(), 1);
    assert_eq!(screen.ap_devices().len(), 2, "the AP table is not filtered");
    assert!(screen.focused_ap.is_some(), "and the AP focus is kept");
    let title = |screen: &WifiScreen| rendered(screen).lines().next().map(str::to_owned);
    assert!(title(&screen).is_some_and(|title| title.contains("WiFi [\"client-2\"] \u{2715}")));

    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Overview));
    assert_eq!(screen.search_query(), Some(""));
    assert!(!rendered(&screen).contains("client-2"));

    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Clients));
    screen.apply_action(&Action::CloseSearch);
    assert!(title(&screen).is_some_and(|title| !title.contains("client-2")));
    assert_eq!(screen.wireless_clients().len(), 3);
}

#[test]
fn a_new_filter_starts_at_its_first_match_and_clearing_it_stays_there() {
    let mut screen = site();
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Clients));
    screen.client_sort = WifiSortField::Name;
    let _ = screen.handle_key_input(key(KeyCode::Char('G')));

    screen.apply_action(&Action::SearchInput("client-3".to_owned()));
    assert_eq!(screen.client_table_state.selected(), Some(0));

    screen.apply_action(&Action::CloseSearch);
    assert_eq!(
        screen.selected_client().and_then(|c| c.name.clone()),
        Some("client-3".to_owned()),
        "the cursor stays on the row the filter found"
    );
}

#[test]
fn search_folds_case_beyond_ascii() {
    let mut screen = WifiScreen::new();
    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![access_point(
        "01",
        "AP-\u{00C9}tage",
    )])));
    screen.apply_action(&Action::SearchInput("\u{00E9}tage".to_owned()));
    assert_eq!(screen.ap_devices().len(), 1);
}

#[test]
fn search_is_not_offered_on_roaming_and_cannot_touch_its_history() {
    let mut screen = roaming_screen();
    screen.apply_action(&roams_loaded(FIRST_MAC, "ap-roof"));
    assert_eq!(screen.search_query(), None);

    screen.apply_action(&Action::SearchInput("nothing matches this".to_owned()));
    assert!(rendered(&screen).contains("ap-roof"));
    assert!(screen.roam_target.is_some());
}

// ── Access ────────────────────────────────────────────────────────────

#[test]
fn nothing_is_fetched_before_the_controller_says_what_it_can_reach() {
    let mut screen = WifiScreen::new();
    screen.focused = true;
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Neighbors));
    assert!(screen.apply_action(&Action::Tick).is_none());

    screen.apply_action(&full_access());
    assert!(matches!(
        screen.apply_action(&Action::Tick),
        Some(Action::RequestWifiNeighbors(_))
    ));
}

#[test]
fn without_a_session_nothing_polls_and_each_sub_tab_says_why() {
    let mut screen = WifiScreen::new();
    screen.focused = true;
    screen.apply_action(&cloud_access());
    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![access_point(
        "01", "ap-a",
    )])));
    screen.apply_action(&Action::ClientsUpdated(Arc::new(Vec::new())));

    for (tab, expected) in [
        (WifiSubTab::Clients, "Wireless client data needs a direct"),
        (WifiSubTab::Neighbors, "Neighbor scans need a direct"),
        (WifiSubTab::Roaming, "Roam history needs a direct"),
    ] {
        screen.apply_action(&Action::WifiSubTab(tab));
        let shown = rendered(&screen);
        assert!(shown.contains(expected), "{tab:?}:\n{shown}");
        assert!(shown.contains("controller session"), "{tab:?}:\n{shown}");
        assert!(
            screen.apply_action(&Action::Tick).is_none(),
            "{tab:?} polled"
        );
    }

    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Overview));
    press(&mut screen, 'c');
    assert!(rendered(&screen).contains("The channel map needs a direct"));
    assert!(screen.apply_action(&Action::Tick).is_none());
    let (message, color) = screen.health_summary();
    assert!(
        color.is_none(),
        "no health claim without client data: {message}"
    );
}

#[test]
fn without_a_session_a_client_count_is_unknown_not_zero() {
    let mut screen = WifiScreen::new();
    screen.apply_action(&cloud_access());
    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![access_point(
        "01", "ap-a",
    )])));
    screen.apply_action(&Action::ClientsUpdated(Arc::new(Vec::new())));
    screen.apply_action(&ssid_fixture());

    let ap = screen.selected_ap().expect("an AP").clone();
    assert_eq!(screen.ap_client_count(&ap), None);
    assert_eq!(screen.ssid_client_count("Studio"), None);
    screen.handle_key_input(key(KeyCode::Enter));
    let shown = rendered(&screen);
    assert!(
        shown.contains("needs a direct controller session"),
        "{shown}"
    );
    assert!(!shown.contains("Clients (0)"), "{shown}");

    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Ssids));
    let confirm = screen.handle_key_input(key(KeyCode::Char('E')));
    assert!(
        matches!(&confirm, Some(Action::ShowConfirm(confirm))
            if confirm.detail.as_deref().is_some_and(|detail| detail.contains("every client"))),
        "{confirm:?}"
    );
}

#[test]
fn without_a_session_the_experience_panel_explains_itself_and_asks_for_nothing() {
    let mut screen = client_detail_screen();
    screen.handle_key_input(key(KeyCode::Esc));
    screen.apply_action(&detail_loaded(FIRST_IP, -42));
    screen.apply_action(&cloud_access());

    let opened = screen.handle_key_input(key(KeyCode::Enter));
    assert!(matches!(opened, Some(Action::Invalidate)), "{opened:?}");
    let shown = rendered(&screen);
    assert!(shown.contains("WiFi experience needs a direct"), "{shown}");
    screen.handle_key_input(key(KeyCode::Char('j')));
    settle(&mut screen);
    assert!(screen.apply_action(&Action::Tick).is_none());
}

#[test]
fn ssids_say_when_the_integration_api_is_missing() {
    let mut screen = WifiScreen::new();
    screen.apply_action(&Action::AccessUpdated(Access {
        integration: false,
        session: true,
        live_events: true,
    }));
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Ssids));
    assert!(rendered(&screen).contains("SSIDs come from the Integration API"));
}

#[test]
fn a_failed_neighbor_scan_is_drawn_in_the_panel_and_never_toasted() {
    let mut screen = site();
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Neighbors));
    assert!(rendered(&screen).contains("Reading the neighbor scan"));

    let follow_up = screen.apply_action(&Action::WifiNeighborsFailed("HTTP 503".to_owned()));
    assert!(follow_up.is_none(), "{follow_up:?}");
    assert!(rendered(&screen).contains("Neighbor scan unavailable: HTTP 503"));

    screen.apply_action(&Action::WifiNeighborsUpdated(Arc::new(vec![neighbor(
        "11:11:11:11:11:11",
        Some("next-door"),
        "01",
        -60,
    )])));
    screen.apply_action(&Action::WifiBandSelect(WifiBand::FiveGhz));
    let shown = rendered(&screen);
    assert!(shown.contains("next-door"), "{shown}");
    assert!(!shown.contains("unavailable"), "{shown}");
}

#[test]
fn a_scan_on_hand_is_not_drawn_once_the_controller_has_no_session() {
    let mut screen = site();
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Neighbors));
    screen.apply_action(&Action::WifiBandSelect(WifiBand::FiveGhz));
    screen.apply_action(&Action::WifiNeighborsUpdated(Arc::new(vec![neighbor(
        "11:11:11:11:11:11",
        Some("next-door"),
        "01",
        -60,
    )])));
    assert!(rendered(&screen).contains("next-door"));

    screen.apply_action(&cloud_access());
    let shown = rendered(&screen);
    assert!(!shown.contains("next-door"), "{shown}");
    assert!(
        shown.contains("need a direct controller session"),
        "{shown}"
    );
}

#[test]
fn a_changed_controller_drops_everything_it_showed() {
    let mut screen = client_detail_screen();
    screen.apply_action(&detail_loaded(FIRST_IP, -42));
    press(&mut screen, ' ');
    screen.apply_action(&Action::SearchInput("client".to_owned()));
    screen.apply_action(&Action::WifiNeighborsUpdated(Arc::new(vec![neighbor(
        "11:11:11:11:11:11",
        Some("next-door"),
        "01",
        -60,
    )])));
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Roaming));
    screen.apply_action(&Action::Tick);

    screen.apply_action(&Action::ControllerChanged);

    assert!(screen.devices.is_empty() && screen.clients.is_empty());
    assert!(screen.neighbors.is_empty() && screen.ssids.is_empty());
    assert!(screen.client_detail.is_none() && screen.roam_target.is_none());
    assert!(screen.client_detail_pending_ip.is_none());
    assert!(screen.roam_history_pending_mac.is_none());
    assert!(screen.focused_ap.is_none() && screen.access.is_none());
    assert!(screen.client_query.is_empty() && !screen.client_detail_open);
    assert_eq!(
        screen.sub_tab,
        WifiSubTab::Roaming,
        "where the user was stays"
    );
    assert!(screen.focused);
    assert!(
        screen.apply_action(&Action::Tick).is_none(),
        "nothing polls the old controller"
    );
}

// ── Client detail ─────────────────────────────────────────────────────

const FIRST_IP: &str = "10.40.10.1";
const SECOND_IP: &str = "10.40.10.2";

/// The Clients tab in name order with two clients on one AP, the first
/// selected and its detail open (and requested).
fn client_detail_screen() -> WifiScreen {
    let mut screen = WifiScreen::new();
    screen.focused = true;
    screen.apply_action(&full_access());
    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![access_point(
        "01", "ap-a",
    )])));
    screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![
        wireless_client(1, AP_A, 80),
        wireless_client(2, AP_A, 70),
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
        data: Arc::new(json!({
            "signal": signal,
            "wlan_band": "5g",
            "uplink_devices": [
                { "display_name": "ap-a", "experience": 97 },
                { "display_name": "sw-core", "experience": 99 }
            ]
        })),
    }
}

/// Let the client cursor count as resting.
fn settle(screen: &mut WifiScreen) {
    screen.client_cursor_moved_at = Instant::now().checked_sub(CURSOR_SETTLE);
}

#[test]
fn the_client_detail_shows_only_the_selected_clients_data() {
    let mut screen = client_detail_screen();
    screen.apply_action(&detail_loaded(FIRST_IP, -42));
    let shown = rendered(&screen);
    assert!(shown.contains("-42 dBm"), "{shown}");
    assert!(
        shown.contains("5 GHz \u{00B7} ch"),
        "band reads like the app: {shown}"
    );
    assert!(
        shown.contains("Uplink ap-a (97) \u{203A} sw-core (99)"),
        "{shown}"
    );

    screen.handle_key_input(key(KeyCode::Char('j')));

    let shown = rendered(&screen);
    assert!(!shown.contains("-42 dBm"), "{shown}");
    assert!(
        shown.contains("Loading WiFi detail for client-2"),
        "{shown}"
    );
}

#[test]
fn scrolling_with_the_detail_open_fetches_only_where_the_cursor_rests() {
    let mut screen = client_detail_screen();
    screen.handle_key_input(key(KeyCode::Char('j')));
    assert!(
        screen.apply_action(&Action::Tick).is_none(),
        "the cursor has only just moved"
    );

    settle(&mut screen);
    let request = screen.apply_action(&Action::Tick);
    assert!(
        matches!(request, Some(Action::RequestWifiClientDetail(ref ip)) if ip == SECOND_IP),
        "{request:?}"
    );
}

#[test]
fn a_late_detail_for_the_previous_client_is_dropped() {
    let mut screen = client_detail_screen();
    screen.handle_key_input(key(KeyCode::Char('j')));
    settle(&mut screen);
    assert!(screen.apply_action(&Action::Tick).is_some());

    screen.apply_action(&detail_loaded(FIRST_IP, -42));

    assert!(
        screen.client_detail.is_none(),
        "the stale answer is not kept"
    );
    assert!(
        screen.apply_action(&Action::Tick).is_none(),
        "and the second client's request is still in flight"
    );

    // Back on the first client, its dropped answer is asked for again.
    screen.apply_action(&detail_loaded(SECOND_IP, -50));
    screen.handle_key_input(key(KeyCode::Char('k')));
    settle(&mut screen);
    assert!(matches!(
        screen.apply_action(&Action::Tick),
        Some(Action::RequestWifiClientDetail(ref ip)) if ip == FIRST_IP
    ));
}

#[test]
fn a_failed_client_detail_is_explained_in_the_panel_and_retried_quietly() {
    let mut screen = client_detail_screen();
    let failed = |ip: &str| Action::WifiClientDetailFailed {
        ip: ip.to_owned(),
        error: "wifiman unavailable".to_owned(),
    };

    assert!(screen.apply_action(&failed(FIRST_IP)).is_none(), "no toast");
    assert!(rendered(&screen).contains("WiFi detail unavailable: wifiman unavailable"));
    assert!(
        screen.apply_action(&Action::Tick).is_none(),
        "a failure is not retried at once"
    );

    // Each client keeps its own failure: the second fails too, and coming
    // back to the first still waits out its retry delay.
    screen.handle_key_input(key(KeyCode::Char('j')));
    settle(&mut screen);
    assert!(screen.apply_action(&Action::Tick).is_some());
    assert!(screen.apply_action(&failed(SECOND_IP)).is_none());
    screen.handle_key_input(key(KeyCode::Char('k')));
    settle(&mut screen);
    assert!(screen.apply_action(&Action::Tick).is_none());

    if let Some(failure) = screen.client_detail_failures.get_mut(FIRST_IP) {
        failure.at = failure
            .at
            .checked_sub(RETRY_INTERVAL)
            .expect("the clock reaches back one retry interval");
    }
    let retry = screen.apply_action(&Action::Tick);
    assert!(
        matches!(retry, Some(Action::RequestWifiClientDetail(ref ip)) if ip == FIRST_IP),
        "{retry:?}"
    );
    screen.apply_action(&detail_loaded(FIRST_IP, -42));
    assert!(rendered(&screen).contains("-42 dBm"));
}

#[test]
fn reopening_the_detail_does_not_stack_requests() {
    let mut screen = client_detail_screen();
    // Close and reopen while the first request is still out.
    assert!(matches!(
        screen.handle_key_input(key(KeyCode::Enter)),
        Some(Action::Invalidate)
    ));
    assert!(matches!(
        screen.handle_key_input(key(KeyCode::Enter)),
        Some(Action::Invalidate)
    ));
    assert!(screen.client_detail_open);
}

#[test]
fn a_detail_left_open_is_refreshed() {
    let mut screen = client_detail_screen();
    screen.apply_action(&detail_loaded(FIRST_IP, -42));
    assert!(screen.apply_action(&Action::Tick).is_none(), "still fresh");

    if let Some(detail) = screen.client_detail.as_mut() {
        detail.loaded_at = detail
            .loaded_at
            .checked_sub(CLIENT_DETAIL_REFRESH_INTERVAL)
            .expect("the clock reaches back one refresh interval");
    }
    assert!(matches!(
        screen.apply_action(&Action::Tick),
        Some(Action::RequestWifiClientDetail(ref ip)) if ip == FIRST_IP
    ));
    assert!(
        rendered(&screen).contains("-42 dBm"),
        "the old numbers stay up while the new ones load"
    );
}

// ── Client commands and jumps ─────────────────────────────────────────

#[test]
fn block_toggles_on_the_clients_blocked_state() {
    let mut screen = client_detail_screen();
    let first = wireless_client(1, AP_A, 80).id.clone();
    assert!(matches!(
        screen.handle_key_input(key(KeyCode::Char('b'))),
        Some(Action::RequestBlockClient(ref id)) if *id == first
    ));
    assert!(hints(&screen).contains("b block"));

    screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![
        with(&wireless_client(1, AP_A, 80), |client| {
            client.blocked = true;
        }),
        wireless_client(2, AP_A, 70),
    ])));
    assert!(matches!(
        screen.handle_key_input(key(KeyCode::Char('b'))),
        Some(Action::RequestUnblockClient(ref id)) if *id == first
    ));
    assert!(hints(&screen).contains("b unblock"), "{}", hints(&screen));
    let shown = rendered(&screen);
    assert!(shown.contains("client-1 (blocked)"), "{shown}");
}

#[test]
fn clients_can_be_kicked_forgotten_and_opened_on_their_own_screen() {
    let mut screen = client_detail_screen();
    let first = wireless_client(1, AP_A, 80).id.clone();
    assert!(matches!(
        screen.handle_key_input(key(KeyCode::Char('x'))),
        Some(Action::RequestKickClient(ref id)) if *id == first
    ));
    assert!(matches!(
        screen.handle_key_input(key(KeyCode::Char('F'))),
        Some(Action::RequestForgetClient(ref id)) if *id == first
    ));
    assert!(matches!(
        screen.handle_key_input(key(KeyCode::Char('o'))),
        Some(Action::ShowClient(ref id)) if *id == first
    ));
    assert!(screen.client_detail_open, "the in-screen panel stays");
}

#[test]
fn an_ap_opens_on_the_devices_screen_and_upgrades_only_when_it_can() {
    let mut screen = site();
    let selected = screen.selected_ap().expect("an AP is selected").id.clone();
    assert!(matches!(
        screen.handle_key_input(key(KeyCode::Char('o'))),
        Some(Action::ShowDevice(ref id)) if *id == selected
    ));

    let note = screen.handle_key_input(key(KeyCode::Char('U')));
    assert!(
        matches!(&note, Some(Action::Notify(note)) if note.message.contains("no firmware update")),
        "{note:?}"
    );
    assert!(!hints(&screen).contains("U upgrade"));

    let updatable =
        |id: &str, name: &str| with(&access_point(id, name), |ap| ap.firmware_updatable = true);
    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![
        updatable("01", "ap-a"),
        updatable("02", "ap-b"),
    ])));
    assert!(matches!(
        screen.handle_key_input(key(KeyCode::Char('U'))),
        Some(Action::RequestUpgrade(ref id)) if *id == selected
    ));
    assert!(hints(&screen).contains("U upgrade"));
}

// ── Roaming ───────────────────────────────────────────────────────────

const FIRST_MAC: &str = "cc:00:00:00:00:01";
const SECOND_MAC: &str = "cc:00:00:00:00:02";

/// The Roaming tab on the first client, its history requested.
fn roaming_screen() -> WifiScreen {
    let mut screen = client_detail_screen();
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Roaming));
    let request = screen.apply_action(&Action::Tick);
    assert!(
        matches!(request, Some(Action::RequestWifiRoamHistory { ref mac, .. }) if mac == FIRST_MAC),
        "{request:?}"
    );
    screen
}

fn roams_loaded(mac: &str, from_ap: &str) -> Action {
    Action::WifiRoamHistoryLoaded {
        mac: mac.to_owned(),
        events: Arc::new(vec![json!({
            "timestamp": 1_760_000_000_000_i64,
            "key": "CLIENT_ROAMED",
            "parameters": {
                "DEVICE_FROM": { "name": from_ap },
                "DEVICE_TO": { "name": "ap-a" },
                "RADIO_BAND": { "name": "na" },
            }
        })]),
    }
}

#[test]
fn r_on_a_client_opens_its_roam_history() {
    let mut screen = client_detail_screen();
    screen.handle_key_input(key(KeyCode::Char('j')));
    let switched = screen.handle_key_input(key(KeyCode::Char('r')));

    assert!(matches!(
        switched,
        Some(Action::WifiSubTab(WifiSubTab::Roaming))
    ));
    assert_eq!(
        screen
            .roam_target
            .as_ref()
            .map(|target| target.mac.as_str()),
        Some(SECOND_MAC)
    );
    let shown = rendered(&screen);
    assert!(shown.contains("client client-2"), "{shown}");
    assert!(
        shown.contains("Loading roam history for client-2"),
        "{shown}"
    );
}

#[test]
fn a_late_roam_history_for_the_previous_client_is_dropped() {
    let mut screen = roaming_screen();

    // Move to the second client and ask for its history.
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Clients));
    screen.handle_key_input(key(KeyCode::Char('j')));
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Roaming));
    let request = screen.apply_action(&Action::Tick);
    assert!(
        matches!(request, Some(Action::RequestWifiRoamHistory { ref mac, .. }) if mac == SECOND_MAC),
        "{request:?}"
    );

    screen.apply_action(&roams_loaded(FIRST_MAC, "ap-first-client"));

    let shown = rendered(&screen);
    assert!(!shown.contains("ap-first-client"), "{shown}");
    let again = screen.apply_action(&Action::Tick);
    assert!(
        again.is_none(),
        "requested the second client twice: {again:?}"
    );
}

#[test]
fn the_roam_history_stays_up_after_its_client_leaves() {
    let mut screen = roaming_screen();
    screen.apply_action(&roams_loaded(FIRST_MAC, "ap-roof"));

    // The client drops off, then the AP cursor moves, then it is back.
    screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![wireless_client(
        2, AP_A, 70,
    )])));
    let shown = rendered(&screen);
    assert!(shown.contains("ap-roof"), "{shown}");
    assert!(shown.contains("client client-1"), "{shown}");
    assert!(
        screen.apply_action(&Action::Tick).is_none(),
        "and it is not fetched again"
    );
}

#[test]
fn a_failed_roam_history_is_explained_in_the_panel_and_retried_quietly() {
    let mut screen = roaming_screen();
    let failed = Action::WifiRoamHistoryFailed {
        mac: FIRST_MAC.to_owned(),
        error: "HTTP 500".to_owned(),
    };

    assert!(screen.apply_action(&failed).is_none(), "no toast");
    assert!(rendered(&screen).contains("Roam history unavailable: HTTP 500"));
    assert!(
        screen.apply_action(&Action::Tick).is_none(),
        "a failure is not retried at once"
    );

    if let Some(failure) = screen.roam_failures.get_mut(FIRST_MAC) {
        failure.at = failure
            .at
            .checked_sub(RETRY_INTERVAL)
            .expect("the clock reaches back one retry interval");
    }
    let retry = screen.apply_action(&Action::Tick);
    assert!(
        matches!(retry, Some(Action::RequestWifiRoamHistory { ref mac, .. }) if mac == FIRST_MAC),
        "{retry:?}"
    );
    screen.apply_action(&roams_loaded(FIRST_MAC, "ap-roof"));
    let shown = rendered(&screen);
    assert!(shown.contains("ap-roof"), "{shown}");
    assert!(shown.contains("Roamed"), "{shown}");
    assert!(shown.contains("5 GHz"), "{shown}");
}

#[test]
fn a_failed_refresh_of_a_history_on_screen_is_said_on_the_toolbar() {
    let mut screen = roaming_screen();
    screen.apply_action(&roams_loaded(FIRST_MAC, "ap-roof"));
    assert!(!rendered(&screen).contains("refresh failed"));

    let asked = screen.handle_key_input(key(KeyCode::Char('r')));
    assert!(
        matches!(asked, Some(Action::RequestWifiRoamHistory { .. })),
        "{asked:?}"
    );
    let follow_up = screen.apply_action(&Action::WifiRoamHistoryFailed {
        mac: FIRST_MAC.to_owned(),
        error: "HTTP 500".to_owned(),
    });
    assert!(follow_up.is_none(), "no toast: {follow_up:?}");
    let shown = rendered(&screen);
    assert!(shown.contains("ap-roof"), "the rows stay:\n{shown}");
    assert!(
        shown.contains("refresh failed"),
        "stale rows must not read as fresh:\n{shown}"
    );

    screen.handle_key_input(key(KeyCode::Char('r')));
    screen.apply_action(&roams_loaded(FIRST_MAC, "ap-roof"));
    assert!(!rendered(&screen).contains("refresh failed"));
}

#[test]
fn a_manual_refresh_waits_for_the_one_in_flight() {
    let mut screen = roaming_screen();
    // The history is already on its way: holding `r` sends nothing more.
    for _ in 0..5 {
        assert!(matches!(
            screen.handle_key_input(key(KeyCode::Char('r'))),
            Some(Action::Invalidate)
        ));
    }
    screen.apply_action(&roams_loaded(FIRST_MAC, "ap-roof"));
    assert!(matches!(
        screen.handle_key_input(key(KeyCode::Char('r'))),
        Some(Action::RequestWifiRoamHistory { .. })
    ));
}

#[test]
fn roaming_says_what_it_is_waiting_for() {
    let mut screen = WifiScreen::new();
    screen.apply_action(&full_access());
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Roaming));
    assert!(rendered(&screen).contains("Pick a client on the Clients tab"));
    assert!(
        screen.handle_key_input(key(KeyCode::Char('r'))).is_none(),
        "nothing to refresh"
    );

    let mut screen = roaming_screen();
    screen.apply_action(&Action::WifiRoamHistoryLoaded {
        mac: FIRST_MAC.to_owned(),
        events: Arc::new(Vec::new()),
    });
    assert!(rendered(&screen).contains("No roam events recorded for client-1"));
}

// ── Neighbors ─────────────────────────────────────────────────────────

#[test]
fn a_neighbor_heard_by_two_aps_is_two_rows_and_the_cursor_keeps_its_row() {
    let mut screen = site();
    screen.apply_action(&Action::WifiBandSelect(WifiBand::FiveGhz));
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Neighbors));
    let scan = |first_signal: i32| {
        Action::WifiNeighborsUpdated(Arc::new(vec![
            neighbor("11:11:11:11:11:11", Some("next-door"), "01", first_signal),
            neighbor("11:11:11:11:11:11", Some("next-door"), "02", -70),
            neighbor("22:22:22:22:22:22", None, "02", -80),
        ]))
    };
    screen.apply_action(&scan(-50));
    screen.handle_key_input(key(KeyCode::Char('j')));
    let seen_by = |screen: &WifiScreen| {
        let index = screen.neighbor_table_state.selected().unwrap_or(0);
        screen.visible_neighbors()[index].ap_mac.clone()
    };
    assert_eq!(seen_by(&screen).as_deref(), Some(AP_B));

    // The other AP's reading weakens and sorts below: the cursor follows
    // the row it was on, not the first row with that BSSID.
    screen.apply_action(&scan(-75));
    assert_eq!(seen_by(&screen).as_deref(), Some(AP_B));

    let shown = rendered_at(&screen, 140, 20);
    for expected in [
        "Heard by", "ap-a", "ap-b", "Age", "42s", "(hidden)", "5 GHz",
    ] {
        assert!(shown.contains(expected), "missing {expected:?}:\n{shown}");
    }
}

// ── Empty states, hints, and help ─────────────────────────────────────

#[test]
fn every_empty_table_says_which_kind_of_empty_it_is() {
    let mut screen = WifiScreen::new();
    screen.apply_action(&full_access());
    assert!(rendered(&screen).contains("Loading access points"));
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Clients));
    assert!(rendered(&screen).contains("Loading wireless clients"));
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Ssids));
    assert!(rendered(&screen).contains("Loading SSIDs"));

    let mut screen = site();
    screen.apply_action(&Action::SearchInput("zzz".into()));
    assert!(rendered(&screen).contains("No access point matching \"zzz\""));
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Clients));
    screen.apply_action(&Action::ClientsUpdated(Arc::new(Vec::new())));
    assert!(rendered(&screen).contains("No wireless clients"));
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Ssids));
    screen.apply_action(&Action::WifiBroadcastsUpdated(Arc::new(Vec::new())));
    assert!(rendered(&screen).contains("No SSIDs"));
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Neighbors));
    screen.apply_action(&Action::WifiNeighborsUpdated(Arc::new(Vec::new())));
    assert!(rendered(&screen).contains("No neighbors heard on 2.4 GHz"));
}

/// The keys the status bar would offer.
fn hint_keys(screen: &WifiScreen) -> Vec<String> {
    hints(screen)
        .split("  ")
        .filter_map(|hint| hint.split(' ').next())
        .filter(|key| !key.is_empty())
        .map(str::to_owned)
        .collect()
}

/// One SSID as the controller lists it, without the settings only its
/// full record carries.
fn ssid_fixture() -> Action {
    let ssid = serde_json::from_value(json!({
        "id": uuid::Uuid::from_u128(9).to_string(), "name": "Studio",
        "enabled": true, "broadcast_type": "Standard", "security": "Wpa2Personal",
        "network_id": null, "frequencies_ghz": [2.4, 5.0],
        "hotspot_enabled": false, "origin": null
    }))
    .expect("SSID fixture should deserialize");
    Action::WifiBroadcastsUpdated(Arc::new(vec![Arc::new(ssid)]))
}

type Mode = (&'static str, fn() -> WifiScreen);

/// One screen per sub-tab and mode the status bar's hints distinguish.
fn modes() -> Vec<Mode> {
    fn tab(tab: WifiSubTab) -> WifiScreen {
        let mut screen = site();
        screen.apply_action(&ssid_fixture());
        screen.apply_action(&Action::WifiNeighborsUpdated(Arc::new(vec![
            neighbor("11:11:11:11:11:11", Some("one"), "01", -50),
            neighbor("22:22:22:22:22:22", Some("two"), "01", -60),
        ])));
        screen.apply_action(&Action::WifiBandSelect(WifiBand::FiveGhz));
        screen.apply_action(&Action::WifiSubTab(tab));
        screen
    }
    fn opened(sub_tab: WifiSubTab) -> WifiScreen {
        let mut screen = tab(sub_tab);
        screen.handle_key_input(key(KeyCode::Enter));
        screen
    }
    vec![
        ("overview", || tab(WifiSubTab::Overview)),
        ("overview detail", || opened(WifiSubTab::Overview)),
        ("overview channels", || {
            let mut screen = tab(WifiSubTab::Overview);
            press(&mut screen, 'c');
            screen
        }),
        ("overview focused", || {
            let mut screen = tab(WifiSubTab::Overview);
            press(&mut screen, ' ');
            screen
        }),
        ("overview empty", || {
            let mut screen = WifiScreen::new();
            screen.apply_action(&full_access());
            screen
        }),
        ("clients", || tab(WifiSubTab::Clients)),
        ("clients detail", || opened(WifiSubTab::Clients)),
        ("clients focused", || {
            let mut screen = tab(WifiSubTab::Overview);
            press(&mut screen, ' ');
            screen.apply_action(&Action::WifiSubTab(WifiSubTab::Clients));
            screen
        }),
        ("ssids", || tab(WifiSubTab::Ssids)),
        ("ssids detail", || opened(WifiSubTab::Ssids)),
        ("neighbors", || tab(WifiSubTab::Neighbors)),
        ("roaming", || tab(WifiSubTab::Roaming)),
        ("roaming without a client", || {
            let mut screen = WifiScreen::new();
            screen.apply_action(&full_access());
            screen.apply_action(&Action::WifiSubTab(WifiSubTab::Roaming));
            screen
        }),
        ("neighbors without a session", || {
            let mut screen = WifiScreen::new();
            screen.apply_action(&cloud_access());
            screen.apply_action(&Action::WifiSubTab(WifiSubTab::Neighbors));
            screen
        }),
    ]
}

/// Whether `action` is one the screen itself carries out. Anything else
/// is work handed to the App: a request, a command, a dialog.
fn stays_on_the_screen(action: &Action) -> bool {
    matches!(
        action,
        Action::Invalidate
            | Action::WifiSubTab(_)
            | Action::WifiFocusAp(_)
            | Action::WifiToggleChannelMap
            | Action::WifiSortColumn(_)
            | Action::WifiBandSelect(_)
    )
}

/// Press `key` and say whether it did anything: work for the App, or a
/// frame that changed. A key that only returns `Invalidate` and leaves
/// the screen as it was has done nothing.
fn has_effect(screen: &mut WifiScreen, key: KeyEvent) -> bool {
    let before = rendered_at(screen, 220, 30);
    let action = screen.handle_key_input(key);
    if let Some(action) = &action {
        screen.apply_action(action);
    }
    match action {
        None => rendered_at(screen, 220, 30) != before,
        Some(action) if stays_on_the_screen(&action) => rendered_at(screen, 220, 30) != before,
        Some(_) => true,
    }
}

/// The keys a hint or help label stands for, each side of a pair.
fn keys_of(label: &str) -> Vec<KeyEvent> {
    match label {
        "Enter" => vec![key(KeyCode::Enter)],
        "Esc" => vec![key(KeyCode::Esc)],
        "Space" => vec![key(KeyCode::Char(' '))],
        "\u{2190}/\u{2192}" => vec![key(KeyCode::Left), key(KeyCode::Right)],
        "\u{2191}/\u{2193}" => vec![key(KeyCode::Down), key(KeyCode::Up)],
        "j/k" => vec![key(KeyCode::Char('j')), key(KeyCode::Char('k'))],
        pair if pair.chars().count() == 3 && pair.chars().nth(1) == Some('/') => pair
            .chars()
            .filter(|ch| *ch != '/')
            .map(|ch| key(KeyCode::Char(ch)))
            .collect(),
        other => vec![key(KeyCode::Char(
            other.chars().next().expect("a label names a key"),
        ))],
    }
}

#[test]
fn every_hinted_key_does_something_in_its_mode_and_is_in_the_help() {
    for (mode, build) in modes() {
        let screen = build();
        let help = screen.help();
        let hinted = hint_keys(&screen);
        // Moving and switching tabs are in the help, never on the bar.
        for navigation in ["h/l", "j/k"] {
            assert!(
                !hinted.contains(&navigation.to_owned()),
                "{mode}: {hinted:?}"
            );
        }

        for label in hinted {
            assert!(
                help.iter()
                    .any(|(keys, _)| keys.split_whitespace().any(|key| key == label)),
                "{mode}: {label:?} is hinted but not in the help: {help:?}"
            );
            // Both sides of a pair are pressed one after the other, in
            // the order that works from here: the cursor may be resting
            // at one end of its list.
            let mut keys = keys_of(&label);
            let works = |keys: &[KeyEvent]| {
                let mut screen = build();
                keys.iter().all(|key| has_effect(&mut screen, *key))
            };
            let forwards = works(&keys);
            keys.reverse();
            assert!(
                forwards || works(&keys),
                "{mode}: {label:?} is hinted but a key of it changes nothing"
            );
        }
    }
}

#[test]
fn the_help_lists_no_key_that_is_dead_in_its_mode() {
    let without_access: [Mode; 4] = [
        ("clients without a session", || {
            let mut screen = site();
            screen.apply_action(&cloud_access());
            screen.apply_action(&Action::WifiSubTab(WifiSubTab::Clients));
            screen
        }),
        ("roaming without a session", || {
            let mut screen = roaming_screen();
            screen.apply_action(&cloud_access());
            screen
        }),
        ("neighbors without a session", || {
            let mut screen = site();
            screen.apply_action(&cloud_access());
            screen.apply_action(&Action::WifiSubTab(WifiSubTab::Neighbors));
            screen
        }),
        ("ssids without the integration api", || {
            let mut screen = site();
            screen.apply_action(&Action::AccessUpdated(Access {
                integration: false,
                session: true,
                live_events: true,
            }));
            screen.apply_action(&Action::WifiSubTab(WifiSubTab::Ssids));
            screen
        }),
    ];
    let list_keys: Vec<&str> = LIST_HELP.iter().map(|(keys, _)| *keys).collect();
    for (mode, build) in modes().into_iter().chain(without_access) {
        for (labels, what) in build().help() {
            // Moving and paging are every list's, with or without rows.
            if list_keys.contains(&labels) {
                continue;
            }
            for label in labels.split_whitespace() {
                for key in keys_of(label) {
                    let mut screen = build();
                    assert!(
                        screen.handle_key_input(key).is_some(),
                        "{mode}: the help lists {label:?} ({what}) but {:?} is dead",
                        key.code
                    );
                }
            }
        }
    }
}

#[test]
fn help_descriptions_fit_the_overlay_column() {
    for (mode, build) in modes() {
        for (keys, label) in build().help() {
            assert!(keys.chars().count() <= 12, "{mode}: {keys:?}");
            assert!(label.chars().count() <= 24, "{mode}: {label:?}");
        }
    }
}

#[test]
fn a_name_too_long_for_its_column_ends_in_an_ellipsis() {
    let mut screen = site();
    let long = "ap-far-side-of-the-warehouse";
    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![
        access_point("01", long),
        access_point("02", "ap-b"),
    ])));

    let shown = rendered_at(&screen, 44, 20);
    assert!(!shown.contains(long), "{shown}");
    assert!(shown.contains("ap-far-side-of\u{2026}"), "{shown}");
    assert!(rendered_at(&screen, 160, 20).contains(long));
}

#[test]
fn a_short_ssid_detail_folds_into_two_columns() {
    let mut screen = site();
    screen.apply_action(&ssid_fixture());
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Ssids));
    screen.handle_key_input(key(KeyCode::Enter));

    let shown = rendered_at(&screen, 80, 20);
    for expected in ["State", "Passphrase", "not shown", "Hotspot", "MLO", "ID"] {
        assert!(shown.contains(expected), "missing {expected:?}:\n{shown}");
    }
    assert!(
        shown
            .lines()
            .any(|line| line.contains("State") && line.contains("Hidden")),
        "{shown}"
    );
}

#[test]
fn the_status_bar_offers_nothing_without_a_row_to_act_on() {
    let mut screen = WifiScreen::new();
    screen.apply_action(&full_access());
    for tab in [WifiSubTab::Clients, WifiSubTab::Ssids, WifiSubTab::Roaming] {
        screen.apply_action(&Action::WifiSubTab(tab));
        assert_eq!(hints(&screen), "", "{tab:?}");
    }
}

#[test]
fn nothing_panics_at_any_size_in_any_mode() {
    for (_, build) in modes() {
        let screen = build();
        for (width, height) in [(0, 0), (1, 1), (10, 3), (20, 6), (40, 10), (80, 24)] {
            rendered_at(&screen, width, height);
        }
    }
}

#[test]
fn clicking_a_sub_tab_switches_to_it() {
    use crate::tui::widgets::click::click_label;

    let mut screen = WifiScreen::new();
    for tab in [
        WifiSubTab::Clients,
        WifiSubTab::Ssids,
        WifiSubTab::Neighbors,
        WifiSubTab::Roaming,
        WifiSubTab::Overview,
    ] {
        let rows = render_lines(120, 30, |frame, area| screen.render(frame, area));
        let action = screen.handle_mouse_input(click_label(&rows, "Roaming", tab.label()));
        assert!(
            matches!(action, Some(Action::WifiSubTab(clicked)) if clicked == tab),
            "{tab:?}: {action:?}"
        );
        assert_eq!(screen.sub_tab, tab);
    }
}

#[test]
fn clicking_the_focus_chip_clears_the_focus() {
    use crate::tui::widgets::click::click_label;

    let mut screen = site();
    press(&mut screen, ' ');
    let rows = render_lines(120, 30, |frame, area| screen.render(frame, area));
    let action = screen.handle_mouse_input(click_label(&rows, "Roaming", "\u{2715}"));
    assert!(
        matches!(action, Some(Action::WifiFocusAp(None))),
        "{action:?}"
    );
}

// ── Chrome: toolbar, frames, and panels ───────────────────────────────

#[test]
fn tables_are_drawn_straight_into_the_screen_frame() {
    for (mode, build) in modes() {
        let screen = build();
        if screen.panel_open() {
            continue;
        }
        let rows = render_lines(103, 40, |frame, area| screen.render(frame, area));
        for row in &rows[1..rows.len() - 1] {
            assert!(
                !row.contains(['\u{256D}', '\u{2570}']),
                "{mode}: a frame inside the frame:\n{}",
                rows.join("\n")
            );
        }
        let text = rows.join("\n");
        for title in [
            "AP Health",
            "Wireless Clients",
            "Neighbor APs",
            "Roaming History",
        ] {
            assert!(!text.contains(title), "{mode}: {title:?} repeats the tab");
        }
    }
}

#[test]
fn the_screen_draws_no_hint_row() {
    for (mode, build) in modes() {
        let screen = build();
        let text = rendered_at(&screen, 220, 30);
        for (_, hint) in screen.hints() {
            let hint = hint_text(&[(0, hint)]);
            assert!(!text.contains(&hint), "{mode}: {hint:?} is drawn:\n{text}");
        }
        for navigation in ["h/l tab", "j/k move"] {
            assert!(!text.contains(navigation), "{mode}:\n{text}");
        }
    }
}

#[test]
fn the_health_line_has_a_row_only_on_the_overview() {
    let mut screen = site();
    let banner = "2 access points";
    let rows = render_lines(103, 20, |frame, area| screen.render(frame, area));
    assert!(rows[2].contains(banner), "{rows:#?}");
    assert!(rows[3].starts_with("\u{2502}AP"), "{rows:#?}");

    for (tab, header) in [
        (WifiSubTab::Clients, "Client"),
        (WifiSubTab::Ssids, "Loading SSIDs"),
        (WifiSubTab::Neighbors, "Reading the neighbor scan"),
        (WifiSubTab::Roaming, "roam history"),
    ] {
        screen.apply_action(&Action::WifiSubTab(tab));
        let rows = render_lines(103, 20, |frame, area| screen.render(frame, area));
        let text = rows.join("\n");
        assert!(!text.contains(banner), "{tab:?}:\n{text}");
        assert!(text.contains(header), "{tab:?}:\n{text}");
    }
    // The table starts right under the toolbar.
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Clients));
    let rows = render_lines(103, 20, |frame, area| screen.render(frame, area));
    assert!(rows[2].starts_with("\u{2502}Client"), "{rows:#?}");
}

/// The toolbar row, without the screen's frame.
fn toolbar_row(screen: &WifiScreen, width: u16) -> String {
    let rows = render_lines(width, 20, |frame, area| screen.render(frame, area));
    rows[1].trim_matches('\u{2502}').to_owned()
}

#[test]
fn the_band_and_the_focus_sit_against_the_right_edge() {
    let mut screen = site();
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Neighbors));
    let row = toolbar_row(&screen, 103);
    assert!(
        row.starts_with(" Overview  Clients  SSIDs  Neighbors  Roaming "),
        "{row:?}"
    );
    assert!(row.ends_with("f band 2.4 GHz "), "{row:?}");

    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Overview));
    press(&mut screen, ' ');
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Neighbors));
    let row = toolbar_row(&screen, 103);
    assert!(
        row.ends_with("f band 2.4 GHz   focus ap-a \u{2715} "),
        "{row:?}"
    );
    // A narrow row keeps what narrows the table and leaves the band out.
    let row = toolbar_row(&screen, 70);
    assert!(row.ends_with("  focus ap-a \u{2715} "), "{row:?}");
    assert!(!row.contains("band"), "{row:?}");
}

#[test]
fn the_focus_stays_in_sight_on_every_sub_tab() {
    let mut screen = roaming_screen();
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Overview));
    press(&mut screen, ' ');
    for tab in WifiSubTab::ALL {
        screen.apply_action(&Action::WifiSubTab(tab));
        let row = toolbar_row(&screen, 103);
        assert!(row.contains("focus ap-"), "{tab:?}: {row:?}");
    }
    // On Roaming the client the tab is about keeps the edge.
    let row = toolbar_row(&screen, 103);
    assert!(row.ends_with("\u{2715}   client client-1 "), "{row:?}");
}

#[test]
fn clicking_a_chip_does_what_its_key_does() {
    use crate::tui::widgets::click::click_label;

    let mut screen = site();
    screen.apply_action(&Action::WifiSubTab(WifiSubTab::Neighbors));
    let rows = render_lines(120, 30, |frame, area| screen.render(frame, area));
    let action = screen.handle_mouse_input(click_label(&rows, "Roaming", "band"));
    assert!(
        matches!(action, Some(Action::PressKey(key)) if key.code == KeyCode::Char('f')),
        "{action:?}"
    );

    screen.apply_action(&Action::SearchInput("one".to_owned()));
    let rows = render_lines(120, 30, |frame, area| screen.render(frame, area));
    let action = screen.handle_mouse_input(click_label(&rows, "WiFi", "\"one\""));
    assert!(matches!(action, Some(Action::CloseSearch)), "{action:?}");
}

#[test]
fn a_panel_under_its_table_takes_what_it_needs_and_leaves_the_rows_in_view() {
    // Under 110 columns a panel sits under its table.
    let frame_of = |screen: &WifiScreen, height: u16| {
        render_lines(103, height, |frame, area| screen.render(frame, area))
    };
    let top_of = |rows: &[String], title: &str| {
        rows.iter()
            .position(|row| row.contains(title))
            .unwrap_or_else(|| panic!("{title} is not drawn:\n{}", rows.join("\n")))
    };

    let mut overview = site();
    overview.handle_key_input(key(KeyCode::Enter));
    let rows = frame_of(&overview, 40);
    let text = rows.join("\n");
    // Title, toolbar, banner, header, and both access points.
    assert_eq!(top_of(&rows, "AP Detail"), 6, "{text}");
    assert!(
        rows[4].contains("ap-b") && rows[5].contains("ap-a"),
        "{text}"
    );
    // Three clients on ap-a: the panel closes under the last of them.
    let last = top_of(&rows, "client-3");
    assert!(rows[last + 1].contains('\u{2570}'), "{text}");
    assert!(last + 2 < rows.len() - 1, "the panel stops short:\n{text}");

    let screen = client_detail_screen();
    let rows = frame_of(&screen, 40);
    assert_eq!(top_of(&rows, "WiFi Experience"), 5, "{}", rows.join("\n"));

    // A screen too short to share goes to a panel that wants it whole.
    let rows = frame_of(&overview, 12);
    assert_eq!(top_of(&rows, "AP Detail"), 3, "{}", rows.join("\n"));
}

// ── Client index ──────────────────────────────────────────────────────

#[test]
fn client_index_deduplicates_associations_and_replaces_each_snapshot() {
    let mut screen = WifiScreen::new();
    let first = access_point("01", "ap-a");
    let second = access_point("02", "ap-b");
    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![
        first.clone(),
        second.clone(),
    ])));
    let mut client = Arc::unwrap_or_clone(wireless_client(1, first.mac.as_str(), 75));
    client.wireless.as_mut().expect("wireless").bssid = Some(first.mac.clone());
    screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![Arc::new(
        client.clone(),
    )])));
    assert_eq!(screen.ap_clients(&first).len(), 1);
    assert_eq!(screen.ap_health(&first), Some(75));
    client.uplink_device_mac = Some(second.mac.clone());
    client.wireless.as_mut().expect("wireless").bssid = None;
    screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![Arc::new(client)])));
    assert!(screen.ap_clients(&first).is_empty());
    assert_eq!(screen.ap_clients(&second).len(), 1);
    assert_eq!(ap_names(&screen), vec!["ap-b", "ap-a"]);
    screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![])));
    assert!(screen.ap_clients(&second).is_empty());
}
