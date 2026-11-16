use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::json;
use unifly_api::model::NetworkManagement;
use unifly_api::{EntityId, Network};
use uuid::Uuid;

use super::super::edit::{Field, NetworkForm, Submit};
use super::{NetworksScreen, format_lease_time};
use crate::tui::action::{Access, Action};
use crate::tui::component::Component;
use crate::tui::table::render_lines;
use crate::tui::widgets::click::{self, click_label};
use crate::tui::widgets::line_input::LineInput;

fn test_network() -> Network {
    serde_json::from_value(json!({
        "id": "11111111-1111-1111-1111-111111111111",
        "name": "Primary LAN",
        "vlan_id": 100,
        "enabled": true,
        "management": "Gateway",
        "purpose": "Corporate",
        "is_default": false,
        "subnet": "192.168.100.0/24",
        "gateway_ip": "192.168.100.1",
        "isolation_enabled": false,
        "internet_access_enabled": true,
        "mdns_forwarding_enabled": false,
        "ipv6_enabled": true,
        "ipv6_mode": "Static",
        "ipv6_prefix": null,
        "dhcpv6_enabled": false,
        "slaac_enabled": false,
        "ntp_server": null,
        "pxe_enabled": false,
        "tftp_server": null,
        "firewall_zone_id": null,
        "cellular_backup_enabled": false,
        "origin": null,
        "dhcp": {
            "enabled": true,
            "range_start": "192.168.100.10",
            "range_stop": "192.168.100.250",
            "lease_time_secs": 3600,
            "dns_servers": ["1.1.1.1"],
            "gateway": "192.168.100.1"
        }
    }))
    .expect("network fixture should deserialize")
}

fn network(id: u128, name: &str, vlan: u16) -> Arc<Network> {
    let mut network = test_network();
    network.id = EntityId::Uuid(Uuid::from_u128(id));
    network.name = name.to_owned();
    network.vlan_id = Some(vlan);
    Arc::new(network)
}

fn screen_with(networks: Vec<Arc<Network>>) -> NetworksScreen {
    let mut screen = NetworksScreen::new();
    screen.apply_action(&Action::NetworksUpdated(Arc::new(networks)));
    screen
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn press(screen: &mut NetworksScreen, code: KeyCode) -> Option<Action> {
    screen.handle_key_input(key(code))
}

fn type_text(screen: &mut NetworksScreen, text: &str) {
    for ch in text.chars() {
        press(screen, KeyCode::Char(ch));
    }
}

/// Focus `field` and replace what it holds with `text`.
fn fill(screen: &mut NetworksScreen, field: Field, text: &str) {
    let form = screen.form.as_mut().expect("open form");
    form.focus_on(field);
    form.edit_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    form.edit_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
    type_text(screen, text);
}

fn toggle(screen: &mut NetworksScreen, field: Field) {
    screen.form.as_mut().expect("open form").focus_on(field);
    press(screen, KeyCode::Char(' '));
}

fn draw(screen: &NetworksScreen, width: u16, height: u16) -> Vec<String> {
    render_lines(width, height, |frame, area| screen.render(frame, area))
}

/// What the status bar offers for the screen as it stands.
fn hints(screen: &NetworksScreen) -> String {
    click::hint_text(&screen.hints())
}

fn form(screen: &NetworksScreen) -> &NetworkForm {
    screen.form.as_ref().expect("open form")
}

#[test]
fn tapping_a_row_selects_it_and_a_second_tap_toggles_its_detail() {
    let mut screen = screen_with(vec![
        network(1, "Primary LAN", 100),
        network(2, "Studio", 10),
    ]);

    let rows = draw(&screen, 120, 24);
    screen.handle_mouse_input(click_label(&rows, "Studio", "Studio"));
    assert_eq!(screen.selected_index(), 1);
    assert!(!screen.detail_open);

    let rows = draw(&screen, 120, 24);
    screen.handle_mouse_input(click_label(&rows, "Studio", "Studio"));
    assert!(screen.detail_open);

    press(&mut screen, KeyCode::Char('e'));
    let rows = draw(&screen, 120, 24);
    screen.handle_mouse_input(click_label(&rows, "Primary LAN", "Primary LAN"));
    assert_eq!(screen.selected_index(), 1, "the form holds the input");
    assert!(
        !rows.iter().any(|row| row.contains("toggle")),
        "Space would type into the Name field, so no toggle button"
    );
    assert_eq!(
        hints(&screen),
        "",
        "the list's keys do nothing under the form"
    );

    let save = screen.handle_mouse_input(click_label(&rows, "save", "save"));
    assert!(
        matches!(save, Some(Action::PressKey(key)) if key.code == KeyCode::Enter),
        "{save:?}"
    );
}

#[test]
fn a_tap_focuses_a_field_and_flips_an_on_off_one() {
    let mut screen = screen_with(vec![network(1, "Primary LAN", 100)]);
    press(&mut screen, KeyCode::Char('e'));

    let rows = draw(&screen, 100, 30);
    screen.handle_mouse_input(click_label(&rows, "VLAN ID", "VLAN ID"));
    assert_eq!(form(&screen).focused(), Field::Vlan);

    let rows = draw(&screen, 100, 30);
    screen.handle_mouse_input(click_label(&rows, "Isolation", "Isolation"));
    assert_eq!(form(&screen).focused(), Field::Isolation);
    assert!(
        form(&screen).flag(Field::Isolation),
        "the tap flipped it on"
    );
}

#[test]
fn an_edit_sends_only_what_changed() {
    let mut screen = screen_with(vec![network(1, "Primary LAN", 100)]);
    press(&mut screen, KeyCode::Char('e'));
    toggle(&mut screen, Field::Mdns);

    let Some(Action::NetworkSave(_, request)) = press(&mut screen, KeyCode::Enter) else {
        panic!("a harmless change saves without asking");
    };
    assert_eq!(
        serde_json::to_value(request).expect("request JSON"),
        json!({"mdns_forwarding_enabled": true})
    );
}

/// Isolating a network or cutting its internet access takes effect on
/// every device on it at once, like a VLAN change does.
#[test]
fn isolating_a_network_or_cutting_its_internet_asks_first_and_says_what_it_costs() {
    for (field, cost, request) in [
        (
            Field::Isolation,
            "Devices on it can no longer reach other networks.",
            json!({"isolation_enabled": true}),
        ),
        (
            Field::Internet,
            "Devices on it lose internet access.",
            json!({"internet_access_enabled": false}),
        ),
    ] {
        let mut screen = screen_with(vec![network(1, "Primary LAN", 100)]);
        press(&mut screen, KeyCode::Char('e'));
        toggle(&mut screen, field);

        let Some(Action::ShowConfirm(confirm)) = press(&mut screen, KeyCode::Enter) else {
            panic!("{field:?} must be confirmed");
        };
        let detail = confirm.detail.expect("the cost is said");
        assert!(detail.contains(cost), "{detail}");
        let Action::NetworkSave(_, sent) = *confirm.action else {
            panic!("yes sends the save");
        };
        assert_eq!(serde_json::to_value(sent).expect("request JSON"), request);
    }
}

#[test]
fn a_save_that_can_cut_devices_off_asks_first_and_lists_the_changes() {
    let mut screen = screen_with(vec![network(1, "Primary LAN", 100)]);
    press(&mut screen, KeyCode::Char('e'));
    toggle(&mut screen, Field::Enabled);
    fill(&mut screen, Field::Vlan, "200");

    let Some(Action::ShowConfirm(confirm)) = press(&mut screen, KeyCode::Enter) else {
        panic!("disabling a network and moving its VLAN must be confirmed");
    };
    assert_eq!(confirm.prompt, "Save these changes to Primary LAN?");
    let detail = confirm.detail.expect("the changes are listed");
    assert!(detail.contains("VLAN ID: 100 \u{2192} 200"), "{detail}");
    assert!(detail.contains("Enabled: on \u{2192} off"), "{detail}");
    let Action::NetworkSave(_, request) = *confirm.action else {
        panic!("yes sends the save");
    };
    assert_eq!(
        serde_json::to_value(request).expect("request JSON"),
        json!({"vlan_id": 200, "enabled": false})
    );
    assert!(screen.form.is_some(), "the draft waits behind the question");
}

#[test]
fn text_fields_edit_at_a_cursor_and_never_type_a_chord() {
    let mut screen = screen_with(vec![network(1, "Primary LAN", 100)]);
    press(&mut screen, KeyCode::Char('e'));

    // Ctrl+W deletes a word; it used to type a `w`.
    screen.handle_key_input(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
    assert_eq!(
        form(&screen).input(Field::Name).map(LineInput::text),
        Some("Primary ")
    );
    press(&mut screen, KeyCode::Home);
    type_text(&mut screen, "The ");
    press(&mut screen, KeyCode::End);
    type_text(&mut screen, "Net  ");
    press(&mut screen, KeyCode::Left);
    press(&mut screen, KeyCode::Delete);

    let Some(Action::NetworkSave(_, request)) = press(&mut screen, KeyCode::Enter) else {
        panic!("a rename saves without asking");
    };
    assert_eq!(
        request.name.as_deref(),
        Some("The Primary Net"),
        "the name is saved trimmed"
    );
}

#[test]
fn a_paste_lands_in_the_focused_field_without_its_newline() {
    let mut screen = screen_with(vec![network(1, "Primary LAN", 100)]);
    press(&mut screen, KeyCode::Char('e'));
    fill(&mut screen, Field::RangeStart, "");

    screen.handle_paste("192.168.100.42\r\n");

    assert!(screen.form.is_some(), "the newline did not submit the form");
    assert_eq!(
        form(&screen).input(Field::RangeStart).map(LineInput::text),
        Some("192.168.100.42")
    );
    assert_eq!(form(&screen).focused(), Field::RangeStart);
}

#[test]
fn a_draft_that_cannot_be_saved_stays_open_with_the_reason_on_its_field() {
    let mut screen = screen_with(vec![network(1, "Primary LAN", 100)]);
    press(&mut screen, KeyCode::Char('e'));
    fill(&mut screen, Field::RangeStop, "192.168.101.20");
    form_mut(&mut screen).focus_on(Field::Name);

    assert!(
        press(&mut screen, KeyCode::Enter).is_none(),
        "nothing is sent"
    );

    let form = form(&screen);
    assert_eq!(form.focused(), Field::RangeStop, "focus moves to the field");
    let rows = draw(&screen, 100, 30);
    let field = rows
        .iter()
        .position(|row| row.contains("Stop") && row.contains("192.168.101.20"))
        .expect("the draft is intact");
    assert!(
        rows[field + 1].contains("Must be inside 192.168.100.1 to 192.168.100.254"),
        "the reason sits under its field:\n{}",
        rows.join("\n")
    );

    // Typing in the field clears the complaint.
    press(&mut screen, KeyCode::Backspace);
    assert!(self::form(&screen).error.is_none());
}

fn form_mut(screen: &mut NetworksScreen) -> &mut NetworkForm {
    screen.form.as_mut().expect("open form")
}

#[test]
fn ranges_typed_with_dhcp_off_keep_the_draft_until_it_is_on() {
    for initially_enabled in [false, true] {
        let mut network = test_network();
        if !initially_enabled {
            network.dhcp = None;
        }
        let mut screen = screen_with(vec![Arc::new(network)]);
        press(&mut screen, KeyCode::Char('e'));
        if initially_enabled {
            toggle(&mut screen, Field::Dhcp);
        }
        fill(&mut screen, Field::RangeStart, "192.168.100.20");
        fill(&mut screen, Field::RangeStop, "192.168.100.200");

        assert!(press(&mut screen, KeyCode::Enter).is_none());
        let error = form(&screen).error.clone().expect("a reason");
        assert_eq!(error.field, Some(Field::Dhcp));
        assert!(
            error.message.contains("Turn the DHCP server on"),
            "{error:?}"
        );
        assert_eq!(
            form(&screen).input(Field::RangeStart).map(LineInput::text),
            Some("192.168.100.20")
        );

        toggle(&mut screen, Field::Dhcp);
        let action = press(&mut screen, KeyCode::Enter);
        let Some(Action::ShowConfirm(confirm)) = action else {
            panic!("a DHCP change is confirmed: {action:?}");
        };
        let Action::NetworkSave(_, request) = *confirm.action else {
            panic!("yes sends the save");
        };
        assert_eq!(request.dhcp_enabled, (!initially_enabled).then_some(true));
        assert_eq!(
            request.dhcp.expect("range edit").range_start.as_deref(),
            Some("192.168.100.20")
        );
    }
}

#[test]
fn enabling_dhcp_sends_the_new_range_and_nothing_else() {
    let mut network = test_network();
    network.dhcp = None;
    let mut form = NetworkForm::edit(&network);
    form.focus_on(Field::Dhcp);
    form.toggle();
    let missing = form.submit().expect_err("a server needs a range");
    assert_eq!(missing.field, Some(Field::RangeStart));

    let mut screen = screen_with(vec![Arc::new(network)]);
    press(&mut screen, KeyCode::Char('e'));
    toggle(&mut screen, Field::Dhcp);
    fill(&mut screen, Field::RangeStart, "192.168.100.10");
    fill(&mut screen, Field::RangeStop, "192.168.100.200");
    fill(&mut screen, Field::Dns, "9.9.9.9, 1.1.1.1");
    let Ok(Submit::Update { request, .. }) = self::form(&screen).submit() else {
        panic!("a whole range is valid");
    };
    assert_eq!(
        serde_json::to_value(request).expect("request JSON"),
        json!({
            "dhcp_enabled": true,
            "dhcp": {
                "range_start": "192.168.100.10", "range_stop": "192.168.100.200",
                "dns_servers": ["9.9.9.9", "1.1.1.1"]
            }
        })
    );
}

#[test]
fn the_gateway_and_prefix_can_be_changed_and_are_checked() {
    let mut screen = screen_with(vec![network(1, "Primary LAN", 100)]);
    press(&mut screen, KeyCode::Char('e'));
    assert_eq!(
        form(&screen).input(Field::Gateway).map(LineInput::text),
        Some("192.168.100.1/24")
    );

    fill(&mut screen, Field::Gateway, "192.168.100.1");
    assert!(press(&mut screen, KeyCode::Enter).is_none());
    assert_eq!(
        form(&screen).error.as_ref().and_then(|error| error.field),
        Some(Field::Gateway)
    );

    fill(&mut screen, Field::Gateway, "192.168.100.1/23");
    let Ok(Submit::Update { request, .. }) = form(&screen).submit() else {
        panic!("a wider prefix still holds the range");
    };
    assert_eq!(request.subnet.as_deref(), Some("192.168.100.1/23"));
}

#[test]
fn a_short_terminal_scrolls_the_form_to_the_focused_field() {
    let mut screen = screen_with(vec![network(1, "Primary LAN", 100)]);
    press(&mut screen, KeyCode::Char('e'));
    form_mut(&mut screen).focus_on(Field::Ipv6);

    let rows = draw(&screen, 60, 12);
    let text = rows.join("\n");
    assert!(text.contains("IPv6"), "{text}");
    assert!(text.contains("save"), "the keys stay in view:\n{text}");
    assert!(!text.contains("VLAN ID"), "the top scrolled away:\n{text}");
}

#[test]
fn networks_list_by_name_and_selection_follows_the_network() {
    let names = |screen: &NetworksScreen| -> Vec<String> {
        screen.networks.iter().map(|n| n.name.clone()).collect()
    };

    let mut screen = screen_with(vec![
        network(3, "Studio", 10),
        network(1, "guest", 30),
        network(2, "IoT", 40),
    ]);
    assert_eq!(names(&screen), vec!["guest", "IoT", "Studio"]);

    screen.select(1);
    screen.apply_action(&Action::NetworksUpdated(Arc::new(vec![
        network(2, "IoT", 40),
        network(4, "Cameras", 50),
        network(3, "Studio", 10),
        network(1, "guest", 30),
    ])));
    assert_eq!(names(&screen), vec!["Cameras", "guest", "IoT", "Studio"]);
    assert_eq!(
        screen.selected_network().map(|n| n.name.as_str()),
        Some("IoT")
    );
}

#[test]
fn edits_save_to_the_network_they_were_opened_on() {
    let mut screen = screen_with(vec![network(1, "Guest", 30), network(2, "IoT", 40)]);
    screen.select(1);
    press(&mut screen, KeyCode::Char('e'));

    // IoT disappears while its form is open; the row now holds Guest.
    screen.apply_action(&Action::NetworksUpdated(Arc::new(vec![network(
        1, "Guest", 30,
    )])));
    let gone = press(&mut screen, KeyCode::Enter);
    assert!(
        matches!(&gone, Some(Action::Notify(notice))
            if notice.message.contains("IoT is no longer on the controller")),
        "{gone:?}"
    );
    assert!(screen.form.is_none());

    screen.apply_action(&Action::NetworksUpdated(Arc::new(vec![
        network(1, "Guest", 30),
        network(3, "Cameras", 50),
    ])));
    screen.select(0);
    press(&mut screen, KeyCode::Char('e'));
    press(&mut screen, KeyCode::Char('!'));
    screen.apply_action(&Action::NetworksUpdated(Arc::new(vec![
        network(4, "Admin", 5),
        network(1, "Guest", 30),
        network(3, "Cameras", 50),
    ])));
    assert!(matches!(
        press(&mut screen, KeyCode::Enter),
        Some(Action::NetworkSave(id, _)) if id == EntityId::Uuid(Uuid::from_u128(3))
    ));
}

#[test]
fn the_form_locks_while_saving_and_a_failure_keeps_the_draft() {
    let mut screen = screen_with(vec![network(1, "Primary LAN", 100)]);
    press(&mut screen, KeyCode::Char('e'));
    type_text(&mut screen, " 2");
    let save = press(&mut screen, KeyCode::Enter).expect("a save");
    assert!(screen.form.is_some(), "the form stays open while it saves");

    screen.apply_action(&save);
    let text = draw(&screen, 100, 30).join("\n");
    assert!(text.contains("Saving\u{2026}"), "{text}");
    assert!(
        !text.contains("cancel"),
        "no key works while it saves:\n{text}"
    );
    for code in [KeyCode::Esc, KeyCode::Enter, KeyCode::Char('x')] {
        assert!(press(&mut screen, code).is_none());
    }
    screen.handle_paste("pasted");
    assert_eq!(
        form(&screen).input(Field::Name).map(LineInput::text),
        Some("Primary LAN 2"),
        "a locked form takes no input"
    );

    screen.apply_action(&Action::NetworkEditResult(Err(
        "the controller said no".into()
    )));
    let text = draw(&screen, 100, 30).join("\n");
    assert!(text.contains("the controller said no"), "{text}");
    assert!(
        text.contains("Primary LAN 2"),
        "the draft is intact:\n{text}"
    );
    assert!(!form(&screen).saving);

    let save = press(&mut screen, KeyCode::Enter).expect("a retry");
    screen.apply_action(&save);
    screen.apply_action(&Action::NetworkEditResult(Ok(())));
    assert!(screen.form.is_none(), "a save that lands closes the form");
}

#[test]
fn a_late_result_never_closes_a_form_that_is_not_saving() {
    let mut screen = screen_with(vec![network(1, "Primary LAN", 100)]);
    press(&mut screen, KeyCode::Char('e'));
    screen.apply_action(&Action::NetworkEditResult(Ok(())));
    assert!(screen.form.is_some());
}

#[test]
fn a_new_network_needs_a_name_a_vlan_and_a_gateway_for_dhcp() {
    let mut screen = screen_with(vec![network(1, "Primary LAN", 100)]);
    screen.apply_action(&Action::AccessUpdated(Access {
        integration: true,
        session: true,
        live_events: false,
    }));
    press(&mut screen, KeyCode::Char('c'));
    let text = draw(&screen, 100, 30).join("\n");
    assert!(text.contains("New network"), "{text}");
    assert!(
        !text.contains("mDNS"),
        "a new network takes no mDNS setting"
    );

    assert!(press(&mut screen, KeyCode::Enter).is_none());
    assert_eq!(
        form(&screen).error.as_ref().and_then(|error| error.field),
        Some(Field::Name)
    );
    fill(&mut screen, Field::Name, "Stage");
    assert!(press(&mut screen, KeyCode::Enter).is_none());
    assert_eq!(
        form(&screen).error.as_ref().and_then(|error| error.field),
        Some(Field::Vlan)
    );
    fill(&mut screen, Field::Vlan, "60");
    toggle(&mut screen, Field::Dhcp);
    assert!(press(&mut screen, KeyCode::Enter).is_none());
    assert_eq!(
        form(&screen).error.as_ref().and_then(|error| error.field),
        Some(Field::Gateway),
        "a DHCP server needs somewhere to serve from"
    );

    fill(&mut screen, Field::Gateway, "10.40.60.1/24");
    fill(&mut screen, Field::RangeStart, "10.40.60.20");
    fill(&mut screen, Field::RangeStop, "10.40.60.200");
    let Some(Action::NetworkCreate(request)) = press(&mut screen, KeyCode::Enter) else {
        panic!("a whole draft creates the network");
    };
    assert_eq!(
        serde_json::to_value(request).expect("request JSON"),
        json!({
            "name": "Stage", "vlan_id": 60, "subnet": "10.40.60.1/24",
            "dhcp_enabled": true, "enabled": true,
            "dhcp_range_start": "10.40.60.20", "dhcp_range_stop": "10.40.60.200",
            "isolation_enabled": false, "internet_access_enabled": true
        })
    );
}

#[test]
fn an_unmanaged_network_offers_only_what_it_can_take() {
    let mut vlan_only = test_network();
    vlan_only.management = Some(NetworkManagement::Unmanaged);
    vlan_only.dhcp = None;
    let form = NetworkForm::edit(&vlan_only);
    assert_eq!(form.fields(), [Field::Name, Field::Vlan, Field::Enabled]);
}

#[test]
fn the_whole_detail_fits_a_24_row_terminal_and_shows_a_disabled_network() {
    let mut disabled = test_network();
    disabled.enabled = false;
    disabled.name = "Lab".into();
    let mut screen = screen_with(vec![Arc::new(disabled), network(2, "Studio", 10)]);
    press(&mut screen, KeyCode::Enter);

    // 24 rows less the app's tab and status bars.
    let rows = draw(&screen, 80, 22);
    let text = rows.join("\n");
    for needed in [
        "Status",
        "Disabled",
        "Purpose",
        "Corporate",
        "Default",
        "Lease",
        "Internet",
        "Isolation",
        "mDNS",
        "Cellular",
        "IPv6",
        "Static",
        "192.168.100.10 to 192.168.100.250",
    ] {
        assert!(text.contains(needed), "missing {needed:?}:\n{text}");
    }
    assert!(
        !text.contains('\u{2014}'),
        "no em dash placeholders:\n{text}"
    );
    // The column `text` starts in, on the detail row that holds `marker`.
    let column = |marker: &str, text: &str| {
        let row = rows
            .iter()
            .find(|row| row.contains(marker))
            .unwrap_or_else(|| panic!("{marker} is drawn"));
        let at = row.find(text).unwrap_or_else(|| panic!("{text} on {row}"));
        row[..at].chars().count()
    };
    // Three fields to a row: labels and values each start in one column
    // whichever row they are on.
    assert_eq!(column("Purpose", "Status"), column("Lease", "Lease"));
    assert_eq!(column("Purpose", "Purpose"), column("Prefix", "IPv6"));
    assert_eq!(
        column("Purpose", "Corporate"),
        column("Prefix", "Static"),
        "values line up under each other"
    );
    // The management type sits after the addresses: a gateway-managed
    // network never reads `Gateway` twice in a row.
    let doubled = rows.iter().any(|row| {
        let words: Vec<&str> = row.split_whitespace().collect();
        words.windows(2).any(|pair| pair == ["Gateway", "Gateway"])
    });
    assert!(!doubled, "{text}");
}

#[test]
fn the_screen_draws_no_hint_row_and_the_detail_takes_only_its_rows() {
    let mut screen = screen_with(vec![
        network(1, "Primary LAN", 100),
        network(2, "Studio", 10),
    ]);
    // The last row inside the border is the table's, not a row of keys.
    let rows = draw(&screen, 80, 5);
    assert!(rows[3].contains("Studio"), "{}", rows.join("\n"));

    press(&mut screen, KeyCode::Enter);
    let rows = draw(&screen, 100, 40);
    let text = rows.join("\n");
    for dead in ["navigate", "Esc close", "e edit", "D delete"] {
        assert!(!text.contains(dead), "{dead} is drawn:\n{text}");
    }
    // The panel is its border and one row per line of fields, and its
    // last row is the last row inside the screen's border.
    let top = rows
        .iter()
        .position(|row| row.contains("\u{256d} Primary LAN "))
        .expect("the panel is drawn");
    assert!(rows[top + 1].contains("Status"), "{}", rows[top + 1]);
    assert!(rows[38].starts_with("\u{2502}\u{2570}"), "{}", rows[38]);
    assert!(rows[37].contains("Prefix"), "{}", rows[37]);
    assert!(top >= 28, "the panel takes {} rows:\n{text}", 39 - top);
}

#[test]
fn hints_offer_the_actions_that_work_on_the_selected_network() {
    let mut default = test_network();
    default.is_default = true;
    default.name = "Default".into();
    let mut screen = screen_with(vec![Arc::new(default), network(2, "Studio", 10)]);

    // Deleting the default network is refused, so it is not offered.
    assert_eq!(hints(&screen), "Enter detail  e edit  c new");
    screen.select(1);
    assert_eq!(hints(&screen), "Enter detail  e edit  c new  D delete");

    // An open detail swaps the way in for the way out, last.
    press(&mut screen, KeyCode::Enter);
    assert_eq!(hints(&screen), "e edit  c new  D delete  Esc close");

    // The form has the keyboard and shows its own keys.
    press(&mut screen, KeyCode::Char('e'));
    assert_eq!(hints(&screen), "");
    let text = draw(&screen, 100, 30).join("\n");
    assert!(text.contains("Enter save  Esc cancel"), "{text}");
    assert!(!text.contains("Tab next"), "{text}");
}

#[test]
fn an_empty_table_says_why_and_offers_only_what_works() {
    let mut screen = NetworksScreen::new();
    let text = |screen: &NetworksScreen| draw(screen, 80, 12).join("\n");
    assert!(text(&screen).contains("Waiting for the controller"));
    assert_eq!(hints(&screen), "");

    let mut access = Access {
        integration: false,
        session: true,
        live_events: true,
    };
    screen.apply_action(&Action::AccessUpdated(access));
    assert!(
        text(&screen).contains("Add an API key"),
        "{}",
        text(&screen)
    );
    assert_eq!(hints(&screen), "", "nothing to create a network with");
    assert!(matches!(
        press(&mut screen, KeyCode::Char('c')),
        Some(Action::Notify(_))
    ));
    assert!(screen.form.is_none());

    access.integration = true;
    screen.apply_action(&Action::AccessUpdated(access));
    assert!(text(&screen).contains("No networks on this site"));
    assert_eq!(hints(&screen), "c new");

    press(&mut screen, KeyCode::Enter);
    assert!(!screen.detail_open, "there is nothing to show a detail of");
}

#[test]
fn a_new_controller_drops_the_networks_the_detail_and_the_form() {
    let mut screen = screen_with(vec![network(1, "Primary LAN", 100)]);
    press(&mut screen, KeyCode::Char('e'));
    screen.set_focused(true);

    screen.apply_action(&Action::ControllerChanged);

    assert!(screen.networks.is_empty());
    assert!(screen.form.is_none() && !screen.detail_open);
    assert!(screen.access.is_none());
    assert!(screen.focused, "it is still the screen on show");
}

#[test]
fn the_default_network_is_not_offered_for_deletion() {
    let mut default = test_network();
    default.is_default = true;
    let mut screen = screen_with(vec![Arc::new(default), network(2, "Studio", 10)]);

    let refused = press(&mut screen, KeyCode::Char('D'));
    assert!(
        matches!(&refused, Some(Action::Notify(notice)) if notice.message.contains("default network")),
        "{refused:?}"
    );
    screen.select(1);
    assert!(matches!(
        press(&mut screen, KeyCode::Char('D')),
        Some(Action::RequestDeleteNetwork(id)) if id == EntityId::Uuid(Uuid::from_u128(2))
    ));
}

#[test]
fn help_lists_the_keys_of_the_mode_on_screen() {
    let mut screen = screen_with(vec![network(1, "Primary LAN", 100)]);
    let keys = |screen: &NetworksScreen| -> Vec<&str> {
        screen.help().into_iter().map(|(key, _)| key).collect()
    };
    for key in ["Enter", "e", "c", "D"] {
        assert!(keys(&screen).contains(&key), "{key}");
    }
    press(&mut screen, KeyCode::Char('e'));
    assert!(keys(&screen).contains(&"Space"));
    assert!(!keys(&screen).contains(&"D"), "a form has no delete key");
}

#[test]
fn lease_time_formats_common_units() {
    assert_eq!(format_lease_time(3600), "1h");
    assert_eq!(format_lease_time(120), "2m");
    assert_eq!(format_lease_time(90), "90s");
}
