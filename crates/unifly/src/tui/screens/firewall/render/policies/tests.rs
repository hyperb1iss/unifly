use std::collections::HashMap;
use std::sync::Arc;

use crossterm::event::KeyCode;
use serde_json::json;
use unifly_api::EntityId;

use super::{FIXED_MARK, zone_networks_label};
use crate::tui::action::{Access, Action, FirewallSubTab};
use crate::tui::screens::firewall::FirewallScreen;
use crate::tui::screens::firewall::fixtures::{
    acl_rule, busy_screen, draw, nat_rule, policy, press, screen_with, user, zone,
};

fn text(screen: &FirewallScreen, width: u16, height: u16) -> String {
    draw(screen, width, height).join("\n")
}

#[test]
fn every_width_names_the_zone_pair_above_its_policies() {
    let screen = busy_screen();
    for width in [80, 96, 120] {
        let rows = draw(&screen, width, 14);
        let pair = rows
            .iter()
            .position(|row| row.contains("lan \u{2192} wan"))
            .unwrap_or_else(|| panic!("zone pair at {width} columns:\n{}", rows.join("\n")));
        assert!(rows[pair + 1].contains(" a "), "{}", rows.join("\n"));
    }
}

#[test]
fn positions_count_from_one_in_each_zone_pair() {
    let rows = draw(&busy_screen(), 100, 14);
    let text = rows.join("\n");
    assert!(!text.contains("10000") && !text.contains("30001"), "{text}");
    let position = |name: &str| {
        let row = rows
            .iter()
            .find(|row| row.contains(&format!(" {name} ")))
            .unwrap_or_else(|| panic!("{name} is listed"));
        row.trim_start_matches(['│', '▸', ' '])
            .split_whitespace()
            .next()
            .map(str::to_owned)
    };
    assert_eq!(position("x").as_deref(), Some("1"));
    assert_eq!(position("a").as_deref(), Some("1"));
    assert_eq!(position("c").as_deref(), Some("4"));
}

#[test]
fn a_fixed_policy_carries_the_mark_and_offers_no_move_or_delete() {
    let mut screen = busy_screen();
    let rows = draw(&screen, 100, 14);
    let marked: Vec<&String> = rows.iter().filter(|row| row.contains(FIXED_MARK)).collect();
    assert_eq!(marked.len(), 1, "{}", rows.join("\n"));
    assert!(marked[0].contains(" sys "));
    assert!(rows.join("\n").contains("K up"), "a user policy can move");

    screen.select(3);
    let hints = text(&screen, 100, 14);
    assert!(
        !hints.contains("K up") && !hints.contains("delete"),
        "{hints}"
    );
    assert!(hints.contains("E on/off"), "{hints}");
}

#[test]
fn a_picked_up_policy_is_marked_and_the_hints_show_how_to_finish() {
    let mut screen = busy_screen();
    screen.select(1);
    press(&mut screen, KeyCode::Char('J'));

    let rows = draw(&screen, 100, 14);
    let moving = rows
        .iter()
        .find(|row| row.contains(" a "))
        .expect("the policy is drawn");
    assert!(
        moving.contains("\u{2195}2"),
        "its new place is marked: {moving}"
    );
    let hints = rows.join("\n");
    assert!(
        hints.contains("Enter apply") && hints.contains("Esc put back"),
        "{hints}"
    );
    assert!(
        !hints.contains("delete"),
        "nothing else answers mid-move:\n{hints}"
    );
}

#[test]
fn the_zone_pair_stays_in_view_above_its_first_policy() {
    let lan = ("lan", "wan");
    let mut policies: Vec<_> = (0..12)
        .map(|n| user(&format!("iot-{n:02}"), ("iot", "wan"), 10_000 + n))
        .collect();
    policies.extend((0..6).map(|n| user(&format!("lan-{n:02}"), lan, 10_000 + n)));
    let mut screen = screen_with(policies);

    // Scroll to the end, then back up to the first policy of the pair.
    press(&mut screen, KeyCode::Char('G'));
    draw(&screen, 100, 12);
    for _ in 0..5 {
        press(&mut screen, KeyCode::Char('k'));
    }
    let rows = draw(&screen, 100, 12);
    let first = rows
        .iter()
        .position(|row| row.contains("lan-00"))
        .unwrap_or_else(|| panic!("the selected policy is on screen:\n{}", rows.join("\n")));
    assert!(
        rows[first - 1].contains("lan \u{2192} wan"),
        "its zone pair scrolled in with it:\n{}",
        rows.join("\n")
    );
}

#[test]
fn the_detail_spells_out_what_the_row_cuts() {
    let detailed = policy(
        "Vendor session",
        ("wan", "lan"),
        10_000,
        &json!({
            "description": "Remote support for the colour suite",
            "action": "Block", "ip_version": "Ipv4", "logging_enabled": true,
            "protocol_summary": "TCP", "schedule": "Weekdays 09:00-17:00",
            "connection_states": ["NEW", "ESTABLISHED"],
            "source_summary": "ip(192.0.2.10, 192.0.2.11 +2 more)",
            "destination_summary": "net(2 networks)",
            "source": { "zone_id": "wan", "filter": {
                "kind": "ip_address", "match_opposite": false,
                "addresses": [
                    { "kind": "address", "value": "192.0.2.10" },
                    { "kind": "address", "value": "192.0.2.11" },
                    { "kind": "subnet", "value": "198.51.100.0/24" },
                    { "kind": "range", "start": "203.0.113.5", "stop": "203.0.113.9" }
                ]
            }},
            "destination": { "zone_id": "lan", "filter": {
                "kind": "network", "match_opposite": false,
                "network_ids": ["net-render", "net-unknown"],
                "ports": { "kind": "values", "items": ["3389", "5900-5910"], "match_opposite": false }
            }}
        }),
    );
    let mut screen = screen_with(vec![detailed]);
    let render: unifly_api::Network = serde_json::from_value(json!({
        "id": "net-render", "name": "Render", "vlan_id": 20, "enabled": true,
        "management": "Gateway", "purpose": "Corporate", "is_default": false,
        "subnet": null, "gateway_ip": null, "isolation_enabled": false,
        "internet_access_enabled": true, "mdns_forwarding_enabled": false,
        "ipv6_enabled": false, "ipv6_mode": null, "ipv6_prefix": null,
        "dhcpv6_enabled": false, "slaac_enabled": false, "ntp_server": null,
        "pxe_enabled": false, "tftp_server": null, "firewall_zone_id": null,
        "cellular_backup_enabled": false, "origin": null, "dhcp": null
    }))
    .expect("network fixture");
    screen.apply_action(&Action::NetworksUpdated(Arc::new(vec![Arc::new(render)])));
    press(&mut screen, KeyCode::Enter);

    // 24 rows less the app's tab and status bars.
    let text = text(&screen, 80, 22);
    for needed in [
        "Remote support for the colour suite",
        "wan \u{2192} lan",
        "1 of 1",
        "IPv4",
        "NEW, ESTABLISHED",
        "Weekdays 09:00-17:00",
        "User-defined",
        "addresses 192.0.2.10, 192.0.2.11, 198.51.100.0/24,",
        "203.0.113.9",
        "networks Render, net-unknown; ports 3389, 5900-5910",
    ] {
        assert!(text.contains(needed), "missing {needed:?}:\n{text}");
    }
    assert!(text.contains("Logging") && text.contains("On"), "{text}");
}

#[test]
fn acl_nat_and_zone_details_show_what_their_tables_leave_out() {
    let mut screen = screen_with(vec![user("a", ("lan", "wan"), 10_000)]);
    screen.apply_action(&Action::AclRulesUpdated(Arc::new(vec![acl_rule(
        "IoT off mgmt",
        true,
    )])));
    screen.apply_action(&Action::NatPoliciesUpdated(Arc::new(vec![nat_rule(
        "Render egress",
        "UserDefined",
    )])));
    press(&mut screen, KeyCode::Enter);

    screen.apply_action(&Action::FirewallSubTab(FirewallSubTab::NatPolicies));
    let nat = text(&screen, 80, 22);
    // The table shows a source-NAT rule's destination as a placeholder.
    for needed in [
        "Source",
        "10.40.20.0/24",
        "Outbound for the render farm",
        "wan-1",
        "1 of 1",
    ] {
        assert!(nat.contains(needed), "missing {needed:?}:\n{nat}");
    }
    assert!(
        !nat.contains("4000"),
        "an ordinal, not the rule index:\n{nat}"
    );

    screen.apply_action(&Action::FirewallSubTab(FirewallSubTab::AclRules));
    let acl = text(&screen, 80, 22);
    for needed in ["Type", "IPv4", "User-defined", "10.40.40.0/24"] {
        assert!(acl.contains(needed), "missing {needed:?}:\n{acl}");
    }

    screen.apply_action(&Action::FirewallSubTab(FirewallSubTab::Zones));
    press(&mut screen, KeyCode::Char('G'));
    let zones = text(&screen, 80, 22);
    assert!(
        zones.contains("Policies to") && zones.contains("System-defined"),
        "{zones}"
    );
}

#[test]
fn sub_tabs_carry_counts_and_empty_tables_say_why() {
    let mut screen = FirewallScreen::new();
    let waiting = text(&screen, 100, 12);
    assert!(
        waiting.contains("Policies (0)") && waiting.contains("NAT (0)"),
        "{waiting}"
    );
    assert!(waiting.contains("Waiting for the controller"), "{waiting}");
    assert!(
        !waiting.contains("navigate"),
        "no row keys without rows:\n{waiting}"
    );

    screen.apply_action(&Action::AccessUpdated(Access {
        integration: false,
        session: false,
        live_events: false,
    }));
    assert!(text(&screen, 100, 12).contains("Add an API key"));
    screen.apply_action(&Action::FirewallSubTab(FirewallSubTab::NatPolicies));
    let nat = text(&screen, 100, 12);
    assert!(nat.contains("need a direct controller session"), "{nat}");

    screen.apply_action(&Action::AccessUpdated(Access {
        integration: true,
        session: true,
        live_events: true,
    }));
    assert!(text(&screen, 100, 12).contains("No NAT rules on this site."));
    screen.apply_action(&Action::FirewallSubTab(FirewallSubTab::AclRules));
    assert!(text(&screen, 100, 12).contains("No ACL rules on this site."));

    let busy = text(&busy_screen(), 100, 12);
    assert!(
        busy.contains("Policies (5)") && busy.contains("Zones (3)"),
        "{busy}"
    );
}

#[test]
fn zone_networks_list_names_or_fall_back_to_a_count() {
    let studio = EntityId::Legacy("studio".into());
    let render_net = EntityId::Legacy("render".into());
    let names: HashMap<&EntityId, &str> =
        HashMap::from([(&studio, "Studio"), (&render_net, "Render")]);

    let known = zone("int", &["studio", "render"]);
    assert_eq!(zone_networks_label(&known, &names), "Studio, Render");

    let partly_known = zone("iot", &["studio", "cameras"]);
    assert_eq!(zone_networks_label(&partly_known, &names), "2 networks");

    let empty = zone("ext", &[]);
    assert_eq!(zone_networks_label(&empty, &names), "\u{2500}");
}
