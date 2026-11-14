use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEventKind};
use serde_json::json;
use unifly_api::{Device, Event};

use super::EventsScreen;
use super::feed::tests::{event, event_at};
use crate::tui::action::{Access, Action};
use crate::tui::component::Component;
use crate::tui::table::render_lines;
use crate::tui::widgets::click::{click_label, mouse};

const FULL: Access = Access {
    integration: true,
    session: true,
    live_events: true,
};

fn connected(access: Access) -> EventsScreen {
    let mut screen = EventsScreen::new();
    screen.update(&Action::Connected).expect("connected");
    screen
        .update(&Action::AccessUpdated(access))
        .expect("access");
    screen
}

fn receive(screen: &mut EventsScreen, minutes: impl IntoIterator<Item = u32>) {
    for minute in minutes {
        screen
            .update(&Action::EventReceived(event(minute)))
            .expect("update");
    }
}

fn press(screen: &mut EventsScreen, code: KeyCode) -> Option<Action> {
    let action = screen.handle_key_event(KeyEvent::from(code)).expect("key");
    // The app hands a pause toggle back to the screen that asked for it.
    if let Some(Action::ToggleEventPause) = action {
        screen.update(&Action::ToggleEventPause).expect("toggle");
    }
    action
}

fn draw(screen: &EventsScreen) -> Vec<String> {
    render_lines(100, 30, |frame, area| screen.render(frame, area))
}

/// The event rows a 100x30 frame shows, oldest first.
fn shown_events(screen: &EventsScreen) -> Vec<String> {
    draw(screen)
        .into_iter()
        .filter(|row| row.contains(" connected") && !row.contains("Message "))
        .map(|row| row.trim_matches(['│', ' ']).to_owned())
        .collect()
}

/// An event one minute past `minute`, in `category`, naming a device.
fn custom(minute: u32, category: &str, message: &str, device_mac: Option<&str>) -> Arc<Event> {
    Arc::new(
        serde_json::from_value(json!({
            "id": format!("custom-{minute}"),
            "timestamp": format!("2026-10-12T10:{minute:02}:00Z"),
            "category": category,
            "severity": "Warning",
            "event_type": "EVT_SW_PoeDisconnect",
            "message": message,
            "device_mac": device_mac,
            "client_mac": null,
            "site_id": null,
            "raw_key": null
        }))
        .expect("event fixture should deserialize"),
    )
}

fn device(id: &str, mac: &str, name: &str) -> Arc<Device> {
    Arc::new(
        serde_json::from_value(json!({
            "id": id, "mac": mac, "ip": "10.0.0.2", "wan_ipv6": null, "name": name,
            "model": "USW", "device_type": "Switch", "state": "Online",
            "firmware_version": null, "firmware_updatable": false, "adopted_at": null,
            "provisioned_at": null, "last_seen": null, "serial": null, "supported": true,
            "ports": [], "radios": [], "uplink_device_id": null, "uplink_device_mac": null,
            "has_switching": true, "has_access_point": false,
            "stats": {
                "uptime_secs": null, "cpu_utilization_pct": null,
                "memory_utilization_pct": null, "load_average_1m": null,
                "load_average_5m": null, "load_average_15m": null,
                "uplink_bandwidth": null, "last_heartbeat": null, "next_heartbeat": null
            },
            "client_count": 0, "origin": null
        }))
        .expect("device fixture should deserialize"),
    )
}

#[test]
fn a_replayed_backlog_does_not_duplicate_the_feed() {
    let mut screen = connected(FULL);
    receive(&mut screen, 0..5);
    receive(&mut screen, 0..5);

    let rows = draw(&screen);
    assert!(rows[0].contains("Events (5)"), "{}", rows[0]);
    assert_eq!(shown_events(&screen).len(), 5);
}

#[test]
fn a_batch_delivered_newest_first_is_listed_in_time_order() {
    let mut screen = connected(FULL);
    receive(&mut screen, [4, 2, 3, 0, 1]);

    let shown = shown_events(&screen);
    let order: Vec<_> = (0..5).map(|minute| format!("client-{minute:02}")).collect();
    for (row, expected) in shown.iter().zip(&order) {
        assert!(row.contains(expected.as_str()), "{shown:?}");
    }
}

#[test]
fn switching_controllers_drops_every_event_and_selection() {
    let mut screen = connected(FULL);
    receive(&mut screen, 0..5);
    press(&mut screen, KeyCode::Char('k'));
    press(&mut screen, KeyCode::Enter);

    screen.update(&Action::ControllerChanged).expect("update");

    assert!(screen.feed.is_empty());
    assert!(!screen.paused && !screen.detail_open);
    assert!(screen.access.is_none(), "access is the old controller's");
    assert!(draw(&screen).iter().any(|row| row.contains("Connecting")));
}

#[test]
fn the_wheel_leaves_an_empty_live_feed_running() {
    let mut screen = connected(FULL);
    screen
        .handle_mouse_event(mouse(MouseEventKind::ScrollUp, 5, 5))
        .expect("wheel handling");

    assert!(!screen.paused);
}

#[test]
fn scrolling_back_by_wheel_or_key_pauses_a_live_feed() {
    for scroll_back in [
        |screen: &mut EventsScreen| {
            screen
                .handle_mouse_event(mouse(MouseEventKind::ScrollUp, 5, 5))
                .expect("wheel handling");
        },
        |screen: &mut EventsScreen| {
            press(screen, KeyCode::Char('k'));
        },
        |screen: &mut EventsScreen| {
            press(screen, KeyCode::Up);
        },
    ] {
        let mut screen = connected(FULL);
        receive(&mut screen, 0..5);

        scroll_back(&mut screen);

        assert!(screen.paused);
        let selected = screen.selected().expect("a cursor");
        assert_eq!(selected.message, "client-03 connected");
        assert!(draw(&screen).iter().any(|row| row.contains("paused")));
    }

    // Scrolling forward on a live feed has nowhere to go.
    let mut screen = connected(FULL);
    receive(&mut screen, 0..5);
    press(&mut screen, KeyCode::Char('j'));
    assert!(!screen.paused);
}

#[test]
fn paging_keys_scroll_back_from_a_live_feed() {
    let mut screen = connected(FULL);
    receive(&mut screen, 0..40);
    draw(&screen);

    screen
        .handle_key_event(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL))
        .expect("page up");
    assert!(screen.paused);
    let page = screen.page.get();
    assert_eq!(
        screen.selected().expect("cursor").message,
        format!("client-{:02} connected", 39 - page)
    );

    press(&mut screen, KeyCode::Char('g'));
    assert_eq!(
        screen.selected().expect("cursor").message,
        "client-00 connected"
    );
}

#[test]
fn the_oldest_events_fill_the_list_instead_of_collapsing_it() {
    let mut screen = connected(FULL);
    receive(&mut screen, 0..40);
    let live = shown_events(&screen).len();
    assert!(live > 5, "the fixture needs a list of several rows");

    press(&mut screen, KeyCode::Char(' '));
    press(&mut screen, KeyCode::Char('g'));
    let top = shown_events(&screen);
    assert_eq!(top.len(), live, "{top:?}");
    assert!(top[0].contains("client-00"), "{top:?}");

    // Holding `k` at the top changes nothing.
    for _ in 0..5 {
        press(&mut screen, KeyCode::Char('k'));
    }
    assert_eq!(shown_events(&screen), top);
}

#[test]
fn live_list_keeps_the_newest_event_above_the_scroll_marker() {
    let mut screen = connected(FULL);
    receive(&mut screen, 0..40);

    for (width, height) in [(80, 24), (100, 30), (183, 47)] {
        let text = render_lines(width, height, |frame, area| screen.render(frame, area));
        let newest = text
            .iter()
            .position(|row| row.contains("client-39 connected"))
            .unwrap_or_else(|| panic!("newest event missing:\n{}", text.join("\n")));
        assert!(
            text[newest + 1].contains("auto-scrolling"),
            "newest event not last:\n{}",
            text.join("\n")
        );
    }
}

#[test]
fn a_paused_feed_holds_still_and_counts_what_arrived() {
    let mut screen = connected(FULL);
    receive(&mut screen, 0..40);

    press(&mut screen, KeyCode::Char(' '));
    let paused = shown_events(&screen);
    assert!(paused.last().is_some_and(|row| row.contains("client-39")));
    receive(&mut screen, 40..45);
    assert_eq!(shown_events(&screen), paused, "new events moved the view");
    assert!(
        draw(&screen).iter().any(|row| row.contains("5 new")),
        "{}",
        draw(&screen).join("\n")
    );

    // Scrolled back, the view holds its place as well.
    for _ in 0..3 {
        press(&mut screen, KeyCode::Char('k'));
    }
    let scrolled = shown_events(&screen);
    receive(&mut screen, 45..50);
    assert_eq!(shown_events(&screen), scrolled, "new events moved the view");

    // The newest event clears the count, and resuming follows the feed.
    press(&mut screen, KeyCode::Char('G'));
    assert!(
        shown_events(&screen)
            .last()
            .is_some_and(|row| row.contains("client-49"))
    );
    assert_eq!(screen.unseen, 0);
    press(&mut screen, KeyCode::Char('k'));
    press(&mut screen, KeyCode::Char(' '));
    assert!(!screen.paused);
    assert!(
        shown_events(&screen)
            .last()
            .is_some_and(|row| row.contains("client-49"))
    );
}

#[test]
fn a_paused_cursor_follows_its_event_through_evictions_and_late_arrivals() {
    let mut screen = connected(FULL);
    screen.feed = super::feed::Feed::new(5);
    receive(&mut screen, 2..7);
    press(&mut screen, KeyCode::Char(' '));
    press(&mut screen, KeyCode::Char('k'));
    assert_eq!(
        screen.selected().expect("cursor").message,
        "client-05 connected"
    );

    // Newer events push the oldest out from under the cursor's index.
    receive(&mut screen, 7..9);
    assert_eq!(
        screen.selected().expect("cursor").message,
        "client-05 connected"
    );

    // Once its own event is gone, the cursor rests on the oldest left.
    receive(&mut screen, 9..14);
    assert_eq!(
        screen.selected().expect("cursor").message,
        "client-09 connected"
    );
}

#[test]
fn enter_opens_the_event_in_full_and_esc_closes_it() {
    let mut screen = connected(FULL);
    receive(&mut screen, 0..3);
    let long = "sw-core port 12 lost PoE: the camera on the loading dock went dark and \
                has not answered since the breaker tripped at the north panel";
    screen
        .update(&Action::EventReceived(custom(5, "Device", long, None)))
        .expect("update");

    press(&mut screen, KeyCode::Enter);
    assert!(screen.paused && screen.detail_open);
    let text = draw(&screen).join("\n");
    for expected in [
        "EVT_SW_PoeDisconnect",
        "Warning",
        "2026-10-12",
        "north panel",
    ] {
        assert!(text.contains(expected), "missing {expected:?}:\n{text}");
    }

    // The detail follows the cursor.
    press(&mut screen, KeyCode::Char('k'));
    assert!(draw(&screen).join("\n").contains("EVT_WU_Connected"));

    let closed = press(&mut screen, KeyCode::Esc);
    assert!(closed.is_some(), "the screen takes the Esc that closes it");
    assert!(!screen.detail_open);
    let left = press(&mut screen, KeyCode::Esc);
    assert!(left.is_none(), "a second Esc is the app's");
}

#[test]
fn the_detail_jumps_to_the_devices_the_event_names() {
    let mut screen = connected(FULL);
    let core = device("dev-core", "aa:bb:cc:dd:ee:01", "sw-core");
    screen
        .update(&Action::DevicesUpdated(Arc::new(vec![Arc::clone(&core)])))
        .expect("devices");
    screen
        .update(&Action::EventReceived(custom(
            1,
            "Device",
            "sw-core port 12 lost PoE",
            Some("aa:bb:cc:dd:ee:01"),
        )))
        .expect("update");
    screen
        .update(&Action::EventReceived(custom(
            2,
            "Device",
            "a forgotten switch came back",
            Some("aa:bb:cc:dd:ee:99"),
        )))
        .expect("update");

    press(&mut screen, KeyCode::Enter);
    let unknown = press(&mut screen, KeyCode::Char('d'));
    assert!(matches!(unknown, Some(Action::Notify(_))), "{unknown:?}");
    assert!(draw(&screen).join("\n").contains("not on this controller"));

    press(&mut screen, KeyCode::Char('k'));
    let text = draw(&screen).join("\n");
    assert!(
        text.contains("sw-core") && text.contains("d opens it"),
        "{text}"
    );
    let jump = press(&mut screen, KeyCode::Char('d'));
    assert!(
        matches!(&jump, Some(Action::ShowDevice(id)) if *id == core.id),
        "{jump:?}"
    );
    let no_client = press(&mut screen, KeyCode::Char('c'));
    assert!(
        matches!(no_client, Some(Action::Notify(_))),
        "{no_client:?}"
    );
}

#[test]
fn h_and_l_step_the_category_filter() {
    let mut screen = connected(FULL);
    receive(&mut screen, 0..4);
    screen
        .update(&Action::EventReceived(custom(
            1,
            "Device",
            "ap-loft upgraded",
            None,
        )))
        .expect("update");
    screen
        .update(&Action::EventReceived(custom(
            2,
            "Network",
            "WAN latency rose",
            None,
        )))
        .expect("update");

    press(&mut screen, KeyCode::Char('l'));
    let rows = draw(&screen);
    assert!(rows[0].contains("Events (1 of 6)"), "{}", rows[0]);
    assert!(rows.iter().any(|row| row.contains("[Device]")));
    assert!(rows.iter().any(|row| row.contains("ap-loft upgraded")));
    assert_eq!(shown_events(&screen), Vec::<String>::new());

    press(&mut screen, KeyCode::Right);
    assert!(draw(&screen)[0].contains("Events (4 of 6)"));
    press(&mut screen, KeyCode::Char('h'));
    press(&mut screen, KeyCode::Left);
    assert!(draw(&screen)[0].contains("Events (6)"));
    // The strip wraps.
    press(&mut screen, KeyCode::Char('h'));
    assert!(draw(&screen).iter().any(|row| row.contains("[Other]")));
    assert!(
        draw(&screen)
            .iter()
            .any(|row| row.contains("No events match the filter"))
    );
}

#[test]
fn tapping_a_category_filters_by_it() {
    let mut screen = connected(FULL);
    receive(&mut screen, 0..4);
    screen
        .update(&Action::EventReceived(custom(
            2,
            "Network",
            "WAN latency rose",
            None,
        )))
        .expect("update");

    let rows = draw(&screen);
    screen
        .handle_mouse_event(click_label(&rows, "[All]", "Network"))
        .expect("click");
    assert!(draw(&screen)[0].contains("Events (1 of 5)"));
    let rows = draw(&screen);
    screen
        .handle_mouse_event(click_label(&rows, "[Network]", "All"))
        .expect("click");
    assert!(draw(&screen)[0].contains("Events (5)"));
}

#[test]
fn search_filters_by_message_text() {
    let mut screen = connected(FULL);
    receive(&mut screen, 0..40);
    assert_eq!(screen.search_query(), Some(""), "search opens here");

    screen
        .update(&Action::SearchInput("CLIENT-3".into()))
        .expect("search");
    let rows = draw(&screen);
    assert!(rows[0].contains("Events (10 of 40)"), "{}", rows[0]);
    assert!(rows.iter().any(|row| row.contains("/CLIENT-3")));
    assert_eq!(screen.search_query(), Some("CLIENT-3"));

    screen.update(&Action::CloseSearch).expect("close");
    assert!(draw(&screen)[0].contains("Events (40)"));
}

#[test]
fn a_filter_that_hides_the_open_event_closes_its_detail() {
    let mut screen = connected(FULL);
    receive(&mut screen, 0..10);
    press(&mut screen, KeyCode::Enter);
    assert!(screen.detail_open);

    screen
        .update(&Action::SearchInput("client-03".into()))
        .expect("search");

    assert!(!screen.detail_open);
    assert_eq!(
        screen.selected().expect("cursor").message,
        "client-03 connected"
    );
}

#[test]
fn rows_carry_the_date_unless_every_event_is_from_today() {
    let mut screen = connected(FULL);
    receive(&mut screen, 0..3);
    let dated = shown_events(&screen);
    let day = event(0)
        .timestamp
        .with_timezone(&chrono::Local)
        .format("%m-%d")
        .to_string();
    assert!(dated[0].starts_with(&day), "{dated:?}");

    let mut today = connected(FULL);
    let mut now: Event = (*event(1)).clone();
    now.timestamp = chrono::Utc::now();
    today
        .update(&Action::EventReceived(Arc::new(now)))
        .expect("update");
    let rows = shown_events(&today);
    assert!(rows[0].chars().nth(2) == Some(':'), "time only: {rows:?}");
}

#[test]
fn the_status_line_says_how_events_arrive() {
    let none = EventsScreen::new();
    assert!(draw(&none).join("\n").contains("connecting"));

    let live = connected(FULL);
    let text = draw(&live).join("\n");
    assert!(text.contains("\u{25CF} live"), "{text}");
    assert!(text.contains("No events yet"), "{text}");

    let polling = connected(Access {
        live_events: false,
        ..FULL
    });
    assert!(draw(&polling).join("\n").contains("polling"));

    let cloud = connected(Access {
        session: false,
        live_events: false,
        ..FULL
    });
    let text = draw(&cloud).join("\n");
    assert!(text.contains("no event access"), "{text}");
    assert!(text.contains("need a direct controller session"), "{text}");
    assert!(!text.contains("live"), "{text}");

    let mut down = connected(FULL);
    down.update(&Action::Disconnected("timed out".into()))
        .expect("update");
    assert!(draw(&down).join("\n").contains("not connected"));
}

#[test]
fn one_failed_refresh_leaves_the_feed_live() {
    let mut screen = connected(FULL);
    receive(&mut screen, 0..3);
    screen
        .update(&Action::Reconnecting { attempt: 1 })
        .expect("a refresh failed");

    let text = draw(&screen).join("\n");
    assert!(text.contains("live"), "{text}");
    assert!(!text.contains("connecting"), "{text}");
    assert_eq!(shown_events(&screen).len(), 3);
    // History can still be asked for.
    assert!(matches!(
        press(&mut screen, KeyCode::Char('o')),
        Some(Action::RequestEventHistory { .. })
    ));
}

#[test]
fn older_history_loads_in_widening_windows_without_duplicates() {
    let mut screen = connected(FULL);
    receive(&mut screen, 3..6);

    let first = press(&mut screen, KeyCode::Char('o'));
    assert!(
        matches!(
            first,
            Some(Action::RequestEventHistory {
                within_hours: 24,
                limit: 500,
                include_firewall: false
            })
        ),
        "{first:?}"
    );
    assert!(draw(&screen).join("\n").contains("loading 24h"));
    // A second press while the load is in flight asks for nothing.
    let busy = press(&mut screen, KeyCode::Char('o'));
    assert!(matches!(busy, Some(Action::Invalidate)), "{busy:?}");

    screen
        .update(&Action::EventHistoryLoaded {
            within_hours: 24,
            include_firewall: false,
            events: Arc::new((0..6).map(event).collect()),
        })
        .expect("loaded");
    assert_eq!(screen.feed.len(), 6);
    let text = draw(&screen).join("\n");
    assert!(text.contains("3 more events"), "{text}");
    assert!(shown_events(&screen)[0].contains("client-00"));

    let second = press(&mut screen, KeyCode::Char('o'));
    assert!(
        matches!(
            second,
            Some(Action::RequestEventHistory {
                within_hours: 72,
                ..
            })
        ),
        "{second:?}"
    );
    screen
        .update(&Action::EventHistoryFailed("session expired".into()))
        .expect("failed");
    assert!(
        draw(&screen)
            .join("\n")
            .contains("history: session expired")
    );
    // The failed window is asked for again.
    let retry = press(&mut screen, KeyCode::Char('o'));
    assert!(
        matches!(
            retry,
            Some(Action::RequestEventHistory {
                within_hours: 72,
                ..
            })
        ),
        "{retry:?}"
    );
}

#[test]
fn history_needs_session_access() {
    let mut screen = connected(Access {
        session: false,
        live_events: false,
        ..FULL
    });
    for key in ['o', 'f'] {
        let action = press(&mut screen, KeyCode::Char(key));
        assert!(
            matches!(action, Some(Action::Notify(_))),
            "{key}: {action:?}"
        );
    }
    assert!(screen.history.loading.is_none());
}

#[test]
fn firewall_blocks_come_and_go_with_f() {
    let mut screen = connected(FULL);
    receive(&mut screen, 0..3);

    let on = press(&mut screen, KeyCode::Char('f'));
    assert!(
        matches!(
            on,
            Some(Action::RequestEventHistory {
                include_firewall: true,
                ..
            })
        ),
        "{on:?}"
    );
    let block = custom(4, "Firewall", "Blocked 203.0.113.9 to nas-vault:445", None);
    screen
        .update(&Action::EventHistoryLoaded {
            within_hours: 24,
            include_firewall: true,
            events: Arc::new(vec![event(1), block]),
        })
        .expect("loaded");
    assert_eq!(screen.feed.len(), 4);
    assert!(draw(&screen).join("\n").contains("+ firewall blocks"));

    let off = press(&mut screen, KeyCode::Char('f'));
    assert!(matches!(off, Some(Action::Invalidate)), "{off:?}");
    assert_eq!(screen.feed.len(), 3);
    assert!(!draw(&screen).join("\n").contains("Blocked 203.0.113.9"));
}

#[test]
fn a_tapped_row_takes_the_cursor_and_a_second_tap_opens_it() {
    let mut screen = connected(FULL);
    receive(&mut screen, 0..6);

    let rows = draw(&screen);
    let tap = click_label(&rows, "client-02", "client-02");
    screen.handle_mouse_event(tap).expect("tap");
    assert!(screen.paused);
    assert_eq!(
        screen.selected().expect("cursor").message,
        "client-02 connected"
    );
    assert!(!screen.detail_open);

    screen.handle_mouse_event(tap).expect("second tap");
    assert!(screen.detail_open);
}

#[test]
fn the_heatmap_yields_to_the_list_on_short_terminals_and_p_toggles_it() {
    let mut screen = connected(FULL);
    receive(&mut screen, 0..40);
    let heatmap = |screen: &EventsScreen, height| {
        render_lines(100, height, |frame, area| screen.render(frame, area))
            .iter()
            .any(|row| row.contains("Events by hour"))
    };

    assert!(heatmap(&screen, 40));
    assert!(!heatmap(&screen, 22), "a short list keeps its rows");

    press(&mut screen, KeyCode::Char('p'));
    assert!(heatmap(&screen, 22), "asked for, it shows where it fits");
    press(&mut screen, KeyCode::Char('p'));
    assert!(!heatmap(&screen, 40));
}

#[test]
fn the_hint_bar_and_help_follow_the_mode() {
    let mut screen = connected(FULL);
    receive(&mut screen, 0..6);
    let hints = |screen: &EventsScreen| {
        let rows = draw(screen);
        rows[rows.len() - 2].clone()
    };

    assert!(hints(&screen).contains("Space pause"), "{}", hints(&screen));
    assert!(screen.help().iter().any(|(key, _)| *key == "Enter"));

    press(&mut screen, KeyCode::Char(' '));
    assert!(hints(&screen).contains("Space resume"));

    press(&mut screen, KeyCode::Enter);
    let bar = hints(&screen);
    assert!(bar.contains("Esc close"), "{bar}");
    assert!(!bar.contains("d device"), "no device to open: {bar}");
    assert!(screen.help().iter().any(|(key, _)| *key == "Esc"));
    assert!(!screen.help().iter().any(|(key, _)| *key == "Enter"));
}

#[test]
fn a_key_storm_on_a_tiny_terminal_draws_without_panicking() {
    let mut screen = connected(FULL);
    receive(&mut screen, 0..30);
    screen
        .update(&Action::EventReceived(event_at(31, None)))
        .expect("update");
    for code in [
        KeyCode::Char('k'),
        KeyCode::Enter,
        KeyCode::Char('g'),
        KeyCode::Char('l'),
        KeyCode::Char('p'),
        KeyCode::Char('G'),
        KeyCode::Char('h'),
        KeyCode::Char(' '),
        KeyCode::Enter,
        KeyCode::Esc,
    ] {
        press(&mut screen, code);
        for (width, height) in [(40, 10), (20, 6), (8, 3), (1, 1)] {
            render_lines(width, height, |frame, area| screen.render(frame, area));
        }
    }
}

#[test]
fn list_header_lines_up_with_the_rows() {
    let mut screen = connected(FULL);
    receive(&mut screen, [5]);

    let text = draw(&screen);
    let header = text
        .iter()
        .find(|row| row.contains("Category"))
        .expect("list header");
    let row = text
        .iter()
        .find(|row| row.contains("client-05"))
        .expect("event row");
    assert_eq!(header.find("Category"), row.find("Client"));
    assert_eq!(header.find("Message"), row.find("client-05"));
}

fn alarm(id: &str, minute: u32, message: &str, archived: bool) -> unifly_api::model::Alarm {
    serde_json::from_value(json!({
        "id": id,
        "timestamp": format!("2026-10-12T08:{minute:02}:00Z"),
        "category": "Device",
        "severity": "Error",
        "message": message,
        "archived": archived,
        "device_mac": "aa:bb:cc:dd:ee:01",
        "site_id": null
    }))
    .expect("alarm fixture should deserialize")
}

fn with_alarms() -> EventsScreen {
    let mut screen = connected(FULL);
    receive(&mut screen, 0..3);
    screen
        .update(&Action::AlarmsLoaded(Arc::new(vec![
            alarm("alarm-old", 5, "ap-dock lost contact", false),
            alarm("alarm-new", 40, "sw-core port 12 lost PoE", false),
            alarm("alarm-done", 50, "WAN2 came back", true),
        ])))
        .expect("alarms");
    screen
}

#[test]
fn v_opens_the_alarms_view_and_reads_them_again() {
    let mut screen = with_alarms();
    let feed = draw(&screen).join("\n");
    assert!(
        feed.contains("\u{26A0} 2 alarms"),
        "the feed counts them: {feed}"
    );

    let opened = press(&mut screen, KeyCode::Char('v'));
    assert!(matches!(opened, Some(Action::RequestAlarms)), "{opened:?}");
    let rows = draw(&screen);
    assert!(rows[0].contains("Alarms (2)"), "{}", rows[0]);
    let text = rows.join("\n");
    assert!(
        text.contains("2 active") && text.contains("1 archived"),
        "{text}"
    );
    assert!(
        !text.contains("WAN2 came back"),
        "archived alarms are out: {text}"
    );
    let newest = text.find("sw-core port 12").expect("newest alarm");
    let oldest = text.find("ap-dock lost contact").expect("oldest alarm");
    assert!(newest < oldest, "newest first:\n{text}");
    assert!(
        screen.search_query().is_none(),
        "search has nothing to filter"
    );

    // Esc and `v` both return to the feed.
    let back = press(&mut screen, KeyCode::Esc);
    assert!(back.is_some() && screen.view == super::View::Feed);
    press(&mut screen, KeyCode::Char('v'));
    press(&mut screen, KeyCode::Char('v'));
    assert!(draw(&screen)[0].contains("Events (3)"));
}

#[test]
fn x_asks_to_archive_the_alarm_under_the_cursor() {
    use unifly_api::Command;

    let mut screen = with_alarms();
    press(&mut screen, KeyCode::Char('v'));
    press(&mut screen, KeyCode::Char('j'));

    let asked = press(&mut screen, KeyCode::Char('x'));
    let Some(Action::ShowConfirm(confirm)) = asked else {
        panic!("expected a confirmation, got {asked:?}");
    };
    let detail = confirm.detail.as_deref().unwrap_or_default();
    assert!(detail.starts_with("ap-dock lost contact"), "{detail}");
    assert!(detail.contains("groups with it"), "{detail}");
    let Action::Run { command, .. } = *confirm.action else {
        panic!("expected a command");
    };
    assert!(
        matches!(&*command, Command::ArchiveAlarm { id } if id.to_string() == "alarm-old"),
        "{command:?}"
    );

    let all = press(&mut screen, KeyCode::Char('X'));
    let Some(Action::ShowConfirm(confirm)) = all else {
        panic!("expected a confirmation, got {all:?}");
    };
    assert!(
        confirm.prompt.contains("all 2 active alarms"),
        "{}",
        confirm.prompt
    );
    assert!(matches!(
        *confirm.action,
        Action::Run { ref command, .. } if matches!(**command, Command::ArchiveAllAlarms)
    ));
}

#[test]
fn the_alarm_cursor_stays_on_its_alarm_when_the_list_changes() {
    let mut screen = with_alarms();
    press(&mut screen, KeyCode::Char('v'));
    press(&mut screen, KeyCode::Char('j'));

    // The newer alarm is archived out from above the cursor.
    screen
        .update(&Action::AlarmsLoaded(Arc::new(vec![
            alarm("alarm-old", 5, "ap-dock lost contact", false),
            alarm("alarm-new", 40, "sw-core port 12 lost PoE", true),
        ])))
        .expect("alarms");

    let rows = draw(&screen);
    let cursor = rows
        .iter()
        .find(|row| row.contains('\u{25B8}'))
        .expect("a cursor row");
    assert!(cursor.contains("ap-dock lost contact"), "{cursor}");

    // With nothing left there is nothing to archive.
    screen
        .update(&Action::AlarmsLoaded(Arc::new(Vec::new())))
        .expect("alarms");
    let text = draw(&screen).join("\n");
    assert!(text.contains("No active alarms"), "{text}");
    assert!(press(&mut screen, KeyCode::Char('x')).is_none());
    assert!(press(&mut screen, KeyCode::Char('X')).is_none());
    assert!(press(&mut screen, KeyCode::Enter).is_none());
    // Keys that do nothing here are not offered.
    assert!(!text.contains("archive"), "{text}");
    assert!(text.contains("reload"), "{text}");
    let help = screen.help();
    assert!(help.iter().all(|(keys, _)| *keys != "x" && *keys != "X"));
}

#[test]
fn enter_opens_the_device_an_alarm_names() {
    let mut screen = with_alarms();
    let core = device("dev-core", "aa:bb:cc:dd:ee:01", "sw-core");
    screen
        .update(&Action::DevicesUpdated(Arc::new(vec![Arc::clone(&core)])))
        .expect("devices");
    press(&mut screen, KeyCode::Char('v'));

    let opened = press(&mut screen, KeyCode::Enter);
    assert!(
        matches!(&opened, Some(Action::ShowDevice(id)) if *id == core.id),
        "{opened:?}"
    );
}

#[test]
fn the_alarms_view_says_why_it_is_empty() {
    let mut cloud = connected(Access {
        session: false,
        live_events: false,
        ..FULL
    });
    let opened = press(&mut cloud, KeyCode::Char('v'));
    assert!(
        matches!(opened, Some(Action::Invalidate)),
        "nothing is read without session access: {opened:?}"
    );
    assert!(
        draw(&cloud)
            .join("\n")
            .contains("need a direct controller session")
    );

    let mut loading = connected(FULL);
    press(&mut loading, KeyCode::Char('v'));
    assert!(draw(&loading).join("\n").contains("Loading alarms"));
    loading
        .update(&Action::AlarmsFailed("forbidden".into()))
        .expect("failed");
    let text = draw(&loading).join("\n");
    assert!(text.contains("could not read alarms: forbidden"), "{text}");
    assert!(text.contains("Press r to try again"), "{text}");
    let retry = press(&mut loading, KeyCode::Char('r'));
    assert!(matches!(retry, Some(Action::RequestAlarms)), "{retry:?}");
}
