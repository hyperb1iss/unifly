use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use unifly_api::{Device, HealthSummary};

use super::focus::tests::{client, device};
use super::{BandwidthSample, DashboardScreen};
use crate::tui::action::{Access, Action};
use crate::tui::component::Component;
use crate::tui::table::render_lines;

fn draw(screen: &DashboardScreen, width: u16, height: u16) -> String {
    render_lines(width, height, |frame, area| screen.render(frame, area)).join("\n")
}

fn connected() -> DashboardScreen {
    let mut screen = DashboardScreen::new();
    screen.update(&Action::Connected).expect("connected");
    screen
        .update(&Action::AccessUpdated(Access {
            integration: true,
            session: true,
            live_events: true,
        }))
        .expect("access");
    screen
}

fn health(subsystem: &str, fields: &serde_json::Value) -> HealthSummary {
    let mut summary = json!({
        "subsystem": subsystem, "status": "ok", "num_adopted": null, "num_sta": null,
        "tx_bytes_r": null, "rx_bytes_r": null, "latency": null, "wan_ip": null,
        "gateways": null, "extra": {}
    });
    if let (Some(summary), Some(fields)) = (summary.as_object_mut(), fields.as_object()) {
        summary.extend(fields.clone());
    }
    serde_json::from_value(summary).expect("health fixture")
}

/// An access point with one radio per `(GHz, channel)`.
fn access_point(name: &str, mac: &str, radios: &[(f32, u32)]) -> Arc<Device> {
    let mut ap = Arc::unwrap_or_clone(device(name, mac, "AccessPoint"));
    ap.radios = radios
        .iter()
        .map(|(frequency_ghz, channel)| unifly_api::model::Radio {
            frequency_ghz: *frequency_ghz,
            channel: Some(*channel),
            channel_width_mhz: None,
            wlan_standard: None,
            tx_retries_pct: None,
            channel_utilization_pct: None,
        })
        .collect();
    Arc::new(ap)
}

#[test]
fn panels_say_connecting_before_they_say_empty() {
    let mut screen = DashboardScreen::new();
    let text = draw(&screen, 120, 40);
    assert!(text.matches("Connecting").count() >= 5, "{text}");
    for claim in [
        "No gateway",
        "No networks",
        "No access points",
        "No clients",
    ] {
        assert!(!text.contains(claim), "{claim} before any data:\n{text}");
    }

    screen.update(&Action::Connected).expect("connected");
    let text = draw(&screen, 120, 40);
    for claim in [
        "No networks",
        "No access points",
        "No clients",
        "No events yet",
    ] {
        assert!(text.contains(claim), "missing {claim}:\n{text}");
    }

    screen
        .update(&Action::Disconnected("timed out".into()))
        .expect("disconnected");
    assert!(draw(&screen, 120, 40).contains("Not connected"));
}

#[test]
fn panels_name_the_access_they_are_missing() {
    let mut screen = connected();
    screen
        .update(&Action::AccessUpdated(Access {
            integration: false,
            session: false,
            live_events: false,
        }))
        .expect("access");

    let text = draw(&screen, 150, 40);
    assert!(text.contains("Networks need the Integration API"), "{text}");
    assert!(text.contains("Events need a controller session"), "{text}");
    assert!(
        text.contains("Site health needs a controller session"),
        "{text}"
    );
}

#[test]
fn missing_health_is_not_drawn_as_measured_zeros() {
    let mut screen = connected();
    screen
        .update(&Action::DevicesUpdated(Arc::new(vec![
            device("gw", "00:00:00:00:00:01", "Gateway"),
            device("sw-a", "00:00:00:00:00:02", "Switch"),
            device("ap-a", "00:00:00:00:00:03", "AccessPoint"),
            device("ap-b", "00:00:00:00:00:04", "AccessPoint"),
        ])))
        .expect("devices");

    let text = draw(&screen, 120, 40);
    // No health at all: the device list still counts the infrastructure,
    // and no rate is printed as if it had been measured at zero.
    assert!(text.contains("AP 2 · SW 1"), "{text}");
    assert!(!text.contains("↑0b"), "{text}");
    assert!(!text.contains("Total"), "{text}");

    // Health that reports a rate for one link draws that link alone.
    screen
        .update(&Action::HealthUpdated(Arc::new(vec![
            health(
                "wan",
                &json!({"tx_bytes_r": 125_000, "rx_bytes_r": 250_000}),
            ),
            health("wlan", &json!({"num_adopted": 7})),
        ])))
        .expect("health");
    let text = draw(&screen, 120, 40);
    assert!(text.contains("Total ↑1.0M  ↓2.0M"), "{text}");
    assert!(text.contains("AP 7 · SW 1"), "health counts win: {text}");
    assert!(
        !text.contains(" LAN  "),
        "no bar for an unreported link: {text}"
    );
}

#[test]
fn a_wan_rate_that_stopped_updating_is_not_sampled_or_shown_as_live() {
    let mut screen = connected();
    let start = Instant::now();
    screen.device_bandwidth = Some(BandwidthSample {
        tx_bps: 10_000,
        rx_bps: 40_000,
        captured_at: start,
    });
    screen.bandwidth_seen_at = Some(start);
    screen.bandwidth_gap = Some(Duration::from_secs(10));

    // Within three refreshes the rate is live and sampled.
    assert!(
        screen
            .sample_bandwidth_if_due(start + Duration::from_secs(20))
            .is_some()
    );
    assert_eq!(screen.bandwidth_tx.len(), 1);

    // Past that it is neither sampled nor reported as the rate now.
    let late = start + Duration::from_secs(40);
    assert!(screen.sample_bandwidth_if_due(late).is_none());
    assert_eq!(screen.bandwidth_tx.len(), 1, "no flat line is invented");
    assert_eq!(screen.live_bandwidth(late), None);
    assert!(screen.bandwidth_stale_for(late).is_some());

    // When rates resume the chart starts over instead of bridging the gap.
    screen.bandwidth_seen_at = Some(late);
    assert!(
        screen
            .sample_bandwidth_if_due(late + Duration::from_secs(1))
            .is_some()
    );
    assert_eq!(screen.bandwidth_tx.len(), 1);
}

#[test]
fn a_lost_link_marks_the_wan_rate_stale_at_once() {
    let mut screen = connected();
    screen
        .update(&Action::HealthUpdated(Arc::new(vec![health(
            "wan",
            &json!({"tx_bytes_r": 125_000, "rx_bytes_r": 250_000}),
        )])))
        .expect("health");
    assert!(draw(&screen, 120, 40).contains("TX 1.0 Mbps"));

    screen
        .update(&Action::Disconnected("lost the connection".into()))
        .expect("disconnected");

    let text = draw(&screen, 120, 40);
    assert!(text.contains("not connected"), "{text}");
    assert!(!text.contains("no rate for"), "{text}");
    assert!(!text.contains("TX 1.0 Mbps"), "{text}");
}

#[test]
fn one_failed_refresh_keeps_the_wan_rate_and_the_chart() {
    let mut screen = connected();
    screen
        .update(&Action::HealthUpdated(Arc::new(vec![health(
            "wan",
            &json!({"tx_bytes_r": 125_000, "rx_bytes_r": 250_000}),
        )])))
        .expect("health");
    screen.update(&Action::Tick).expect("tick");
    let samples = screen.bandwidth_tx.len();
    assert!(samples > 0, "the chart has a sample");

    screen
        .update(&Action::Reconnecting { attempt: 1 })
        .expect("a refresh failed");
    screen.update(&Action::Tick).expect("tick");

    let text = draw(&screen, 120, 40);
    assert!(text.contains("TX 1.0 Mbps"), "{text}");
    assert!(!screen.chart_stalled, "the chart is not marked for a wipe");
    assert!(screen.bandwidth_tx.len() >= samples);

    screen.update(&Action::Connected).expect("recovered");
    screen.update(&Action::Tick).expect("tick");
    assert!(screen.bandwidth_tx.len() >= samples, "nothing was cleared");
}

#[test]
fn a_rate_that_stopped_arriving_reads_its_real_age() {
    let mut screen = connected();
    assert_eq!(
        screen.stale_rate_text(std::time::Duration::from_secs(47)),
        "no rate for 47s"
    );
    assert_eq!(
        screen.stale_rate_text(std::time::Duration::from_secs(150)),
        "no rate for 2m"
    );
    screen
        .update(&Action::Disconnected("gone".into()))
        .expect("disconnected");
    assert_eq!(
        screen.stale_rate_text(std::time::Duration::from_secs(1)),
        "not connected"
    );
}

#[test]
fn the_gateway_panel_shows_the_last_speed_test_from_site_health() {
    let mut screen = connected();
    screen
        .update(&Action::DevicesUpdated(Arc::new(vec![device(
            "gw-harbor",
            "00:00:00:00:00:01",
            "Gateway",
        )])))
        .expect("devices");
    assert!(!draw(&screen, 120, 40).contains(" Spd "));

    let ran = chrono::Utc::now().timestamp() - 2 * 3600;
    screen
        .update(&Action::HealthUpdated(Arc::new(vec![
            health("wan", &json!({})),
            health(
                "www",
                &json!({"extra": {
                    "xput_down": 913.4, "xput_up": 41.2,
                    "speedtest_ping": 9, "speedtest_lastrun": ran
                }}),
            ),
        ])))
        .expect("health");

    let wide = draw(&screen, 160, 40);
    assert!(
        wide.contains("Spd  \u{2193} 913 Mbps  \u{2191} 41 Mbps"),
        "{wide}"
    );
    assert!(wide.contains("2h ago"), "{wide}");
    // The narrow panel keeps both rates.
    let narrow = draw(&screen, 80, 24);
    assert!(narrow.contains("\u{2193}913M \u{2191}41M"), "{narrow}");
}

#[test]
fn health_without_a_wan_rate_reports_none_instead_of_zero() {
    let mut screen = connected();
    screen
        .update(&Action::DevicesUpdated(Arc::new(vec![device(
            "ap-a",
            "00:00:00:00:00:03",
            "AccessPoint",
        )])))
        .expect("devices");
    screen
        .update(&Action::HealthUpdated(Arc::new(vec![health(
            "wan",
            &json!({}),
        )])))
        .expect("health");

    assert_eq!(screen.current_bandwidth(), None);
    let text = draw(&screen, 120, 40);
    assert!(
        text.contains("This controller reports no WAN rate"),
        "{text}"
    );
    assert!(text.contains("TX ─  RX ─"), "{text}");
}

#[test]
fn the_ap_table_writes_every_channel_whole_and_one_way() {
    let mut screen = connected();
    screen
        .update(&Action::DevicesUpdated(Arc::new(vec![
            access_point(
                "ap-lounge",
                "00:00:00:00:00:03",
                &[(2.4, 11), (5.0, 132), (6.0, 197)],
            ),
            access_point(
                "ap-beacon",
                "00:00:00:00:00:04",
                &[(2.4, 1), (5.0, 36), (6.0, 37)],
            ),
        ])))
        .expect("devices");

    // Wide enough for band labels on every row.
    let wide = draw(&screen, 200, 40);
    assert!(wide.contains("2G:11 5G:132 6G:197"), "{wide}");
    assert!(wide.contains("2G:1 5G:36 6G:37"), "{wide}");

    // Narrower, every row drops to bare channels together.
    let medium = draw(&screen, 120, 40);
    assert!(medium.contains("11/132/197"), "{medium}");
    assert!(medium.contains("1/36/37"), "{medium}");
    assert!(!medium.contains("2G:"), "one form per column: {medium}");

    // Too narrow for the widest row, the column goes rather than clipping.
    let narrow = draw(&screen, 80, 40);
    assert!(!narrow.contains("Chan"), "{narrow}");
    assert!(!narrow.contains("11/1"), "{narrow}");
}

#[test]
fn top_clients_keep_a_gap_before_their_bars_and_rank_only_measured_ones() {
    let mut screen = connected();
    screen
        .update(&Action::ClientsUpdated(Arc::new(vec![
            client("dailies-review", "00:00:00:00:10:01", 9_000_000_000),
            client("nas", "00:00:00:00:10:02", 5_000_000_000),
            client("idle-printer", "00:00:00:00:10:03", 0),
        ])))
        .expect("clients");

    let text = draw(&screen, 120, 40);
    assert!(text.contains("dailies-review █"), "{text}");
    assert!(!text.contains("idle-printer"), "{text}");

    screen
        .update(&Action::ClientsUpdated(Arc::new(vec![client(
            "idle-printer",
            "00:00:00:00:10:03",
            0,
        )])))
        .expect("clients");
    assert!(draw(&screen, 120, 40).contains("No client traffic reported"));
}

#[test]
fn a_short_terminal_still_shows_the_isp_latency_and_link_rates() {
    let mut screen = connected();
    screen
        .update(&Action::DevicesUpdated(Arc::new(vec![
            device("gw", "00:00:00:00:00:01", "Gateway"),
            device("ap-a", "00:00:00:00:00:03", "AccessPoint"),
        ])))
        .expect("devices");
    screen
        .update(&Action::HealthUpdated(Arc::new(vec![
            health(
                "wan",
                &json!({
                    "tx_bytes_r": 125_000, "rx_bytes_r": 250_000, "wan_ip": "203.0.113.42",
                    "extra": {"isp_name": "Tidewater Fiber", "nameservers": ["192.0.2.53"]}
                }),
            ),
            health("www", &json!({"latency": 11.0})),
            health("wlan", &json!({"tx_bytes_r": 1_250_000, "rx_bytes_r": 0})),
            health(
                "lan",
                &json!({"tx_bytes_r": 250_000, "rx_bytes_r": 0, "status": "warn"}),
            ),
        ])))
        .expect("health");
    for minute in 0..3 {
        screen
            .update(&Action::EventReceived(
                crate::tui::screens::events::feed::tests::event(minute),
            ))
            .expect("event");
    }

    // 80x24 leaves the screen 80x22 between the tab and status bars.
    let text = draw(&screen, 80, 22);
    for expected in [
        "ISP  Tidewater Fiber",
        "Lat  11ms",
        "DNS  192.0.2.53",
        "WLAN",
        "10.0M",
        "client-02 connected",
    ] {
        assert!(text.contains(expected), "missing {expected:?}:\n{text}");
    }
    // Every subsystem's state is on screen, none cut at the panel edge.
    for subsystem in ["WAN ●", "WWW ●", "WLAN ●", "LAN ●", "VPN ●"] {
        assert!(text.contains(subsystem), "missing {subsystem:?}:\n{text}");
    }
}

#[test]
fn the_age_label_keeps_counting_on_a_dashboard_with_no_traffic_chart() {
    let mut screen = connected();
    screen.focused = true;
    screen
        .update(&Action::DevicesUpdated(Arc::new(vec![device(
            "ap-a",
            "00:00:00:00:00:03",
            "AccessPoint",
        )])))
        .expect("devices");
    draw(&screen, 120, 40);
    assert!(
        screen.update(&Action::Tick).expect("tick").is_none(),
        "nothing changed since the frame"
    );

    screen.last_data_update = Instant::now().checked_sub(Duration::from_secs(30));

    let redraw = screen.update(&Action::Tick).expect("tick");
    assert!(matches!(redraw, Some(Action::Invalidate)), "{redraw:?}");
    assert!(draw(&screen, 120, 40).contains("30s ago"));
}

#[test]
fn a_key_storm_on_a_tiny_terminal_draws_without_panicking() {
    use crossterm::event::{KeyCode, KeyEvent};

    let mut screen = connected();
    screen.focused = true;
    for code in [
        KeyCode::Down,
        KeyCode::Right,
        KeyCode::Char('j'),
        KeyCode::Char('l'),
        KeyCode::Char('G'),
        KeyCode::Char('k'),
        KeyCode::Char('h'),
        KeyCode::Char('g'),
        KeyCode::Enter,
        KeyCode::Char('S'),
    ] {
        screen.handle_key_event(KeyEvent::from(code)).expect("key");
        for (width, height) in [(40, 10), (42, 12), (20, 6), (1, 1)] {
            draw(&screen, width, height);
        }
    }
}

#[test]
fn the_title_counts_unarchived_alarms() {
    let alarm = |id: &str, archived: bool| -> unifly_api::model::Alarm {
        serde_json::from_value(json!({
            "id": id, "timestamp": "2026-10-12T08:00:00Z", "category": "Device",
            "severity": "Error", "message": "ap-dock lost contact",
            "archived": archived, "device_mac": null, "site_id": null
        }))
        .expect("alarm fixture")
    };
    let mut screen = connected();
    assert!(!draw(&screen, 120, 40).contains("alarm"));

    screen
        .update(&Action::AlarmsLoaded(Arc::new(vec![
            alarm("a", false),
            alarm("b", false),
            alarm("c", true),
        ])))
        .expect("alarms");
    assert!(draw(&screen, 120, 40).contains("\u{26A0} 2 alarms"));

    screen
        .update(&Action::AlarmsLoaded(Arc::new(vec![alarm("c", true)])))
        .expect("alarms");
    assert!(!draw(&screen, 120, 40).contains("\u{26A0}"));
}

/// A network on `vlan` with a /24 subnet, with or without IPv6.
fn network(name: &str, vlan: u16, ipv6: bool) -> Arc<unifly_api::Network> {
    let mut network = Arc::unwrap_or_clone(crate::tui::screens::devices::fixtures::network(
        name, name, vlan,
    ));
    network.subnet = Some(format!("10.40.{vlan}.0/24"));
    network.ipv6_enabled = ipv6;
    network.ipv6_prefix = ipv6.then(|| format!("2001:db8:4e48:{vlan}::/64"));
    Arc::new(network)
}

fn with_networks() -> DashboardScreen {
    let mut screen = connected();
    screen
        .update(&Action::NetworksUpdated(Arc::new(vec![
            network("Cameras", 50, false),
            network("Mgmt", 1, true),
            network("Studio", 10, false),
        ])))
        .expect("networks");
    let mut on_studio = Arc::unwrap_or_clone(client("render-03", "00:00:00:00:10:02", 9_000));
    on_studio.vlan = Some(10);
    screen
        .update(&Action::ClientsUpdated(Arc::new(vec![Arc::new(on_studio)])))
        .expect("clients");
    screen
}

/// The rows of the Networks panel, without its frame.
fn networks_panel(screen: &DashboardScreen, width: u16, height: u16) -> Vec<String> {
    let rows = render_lines(width, height, |frame, area| screen.render(frame, area));
    let top = rows
        .iter()
        .position(|row| row.contains("Networks"))
        .expect("the panel is drawn");
    rows[top + 1..]
        .iter()
        .map_while(|row| {
            let row = row.strip_prefix("\u{2502}\u{2502}")?;
            Some(row.split('\u{2502}').next().unwrap_or(row).to_owned())
        })
        .collect()
}

#[test]
fn a_network_is_one_headed_line_and_its_ipv6_is_only_a_mark() {
    let screen = with_networks();
    let panel = networks_panel(&screen, 103, 40);

    assert_eq!(panel[0].trim_end(), " Name  VLAN Subnet  Clients v6");
    assert_eq!(panel[1].trim_end(), " Mgmt     1 10.40.1.0/24  0 on");
    assert_eq!(panel[2].trim_end(), " Studio  10 10.40.10.0/24 1");
    assert_eq!(panel[3].trim_end(), " Cameras 50 10.40.50.0/24 0");
    let text = panel.join("\n");
    for gone in ["2001:db8", "\u{2026}", "\u{2B21}", "1c"] {
        assert!(!text.contains(gone), "{gone:?} is drawn:\n{text}");
    }
}

#[test]
fn a_narrow_networks_panel_drops_columns_whole() {
    let screen = with_networks();
    let headers: Vec<String> = [96, 84, 80, 72]
        .into_iter()
        .map(|width| networks_panel(&screen, width, 40)[0].trim_end().to_owned())
        .collect();
    assert_eq!(
        headers,
        [
            " Name  VLAN Subnet  Clients",
            " Name  VLAN Subnet",
            " Name    Subnet",
            " Name"
        ]
    );
    // Whatever is dropped, what is left is drawn whole.
    for width in [72, 80, 103, 140] {
        let panel = networks_panel(&screen, width, 40);
        let text = panel.join("\n");
        assert!(text.contains("Cameras"), "{width}:\n{text}");
        assert!(!text.contains('\u{2026}'), "{width}:\n{text}");
        if panel[0].contains("Subnet") {
            assert!(text.contains("10.40.50.0/24"), "{width}:\n{text}");
        }
    }
}

#[test]
fn the_lists_take_the_rows_the_status_panels_do_not_fill() {
    let screen = connected();
    let rows = render_lines(103, 40, |frame, area| screen.render(frame, area));
    let top = |title: &str| {
        rows.iter()
            .position(|row| row.contains(title))
            .expect("the panel is drawn")
    };
    assert_eq!(
        top("Networks") - top("Gateway"),
        10,
        "eight lines and a frame"
    );
    assert!(top("Recent Events") - top("Networks") > 10);
}
