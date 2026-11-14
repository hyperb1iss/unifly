//! Entities and input helpers for the Firewall screen's tests.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent};
use serde_json::{Value, json};
use unifly_api::model::{AclRule, FirewallPolicy, FirewallZone, NatPolicy};

use super::FirewallScreen;
use crate::tui::action::Action;
use crate::tui::component::Component;
use crate::tui::table::render_lines;

pub(super) fn zone(id: &str, networks: &[&str]) -> Arc<FirewallZone> {
    Arc::new(
        serde_json::from_value(json!({
            "id": id, "name": id, "network_ids": networks, "origin": "SystemDefined"
        }))
        .expect("zone fixture"),
    )
}

/// A policy between two zones, with `extra` laid over the defaults.
pub(super) fn policy(
    name: &str,
    pair: (&str, &str),
    index: i32,
    extra: &Value,
) -> Arc<FirewallPolicy> {
    let mut fields = json!({
        "id": name, "name": name, "description": null, "enabled": true,
        "index": index, "action": "Allow", "ip_version": "Both",
        "source": { "zone_id": pair.0, "filter": null },
        "destination": { "zone_id": pair.1, "filter": null },
        "source_summary": "src", "destination_summary": "dst",
        "protocol_summary": "Any", "schedule": null, "ipsec_mode": null,
        "connection_states": [], "logging_enabled": false, "origin": "UserDefined"
    });
    if let (Some(fields), Some(extra)) = (fields.as_object_mut(), extra.as_object()) {
        fields.extend(extra.clone());
    }
    Arc::new(serde_json::from_value(fields).expect("policy fixture"))
}

pub(super) fn user(name: &str, pair: (&str, &str), index: i32) -> Arc<FirewallPolicy> {
    policy(name, pair, index, &json!({}))
}

pub(super) fn system(name: &str, pair: (&str, &str), index: i32) -> Arc<FirewallPolicy> {
    policy(name, pair, index, &json!({ "origin": "SystemDefined" }))
}

pub(super) fn acl_rule(name: &str, enabled: bool) -> Arc<AclRule> {
    Arc::new(
        serde_json::from_value(json!({
            "id": name, "name": name, "enabled": enabled, "index": 7,
            "rule_type": "Ipv4", "action": "Block",
            "source_summary": "10.40.40.0/24", "destination_summary": "10.40.0.0/24",
            "origin": "UserDefined"
        }))
        .expect("acl fixture"),
    )
}

pub(super) fn nat_rule(name: &str, origin: &str) -> Arc<NatPolicy> {
    Arc::new(
        serde_json::from_value(json!({
            "id": name, "name": name, "description": "Outbound for the render farm",
            "enabled": true, "nat_type": "Source", "interface_id": "wan-1",
            "protocol": null, "src_address": "10.40.20.0/24", "src_port": null,
            "dst_address": null, "dst_port": null,
            "translated_address": "203.0.113.43", "translated_port": null,
            "rule_index": 4_000, "origin": origin
        }))
        .expect("nat fixture"),
    )
}

/// A screen holding the `lan` and `wan` zones and these policies, as the
/// data bridge would deliver them.
pub(super) fn screen_with(policies: Vec<Arc<FirewallPolicy>>) -> FirewallScreen {
    let mut screen = FirewallScreen::new();
    screen.apply_action(&Action::FirewallZonesUpdated(Arc::new(vec![
        zone("iot", &[]),
        zone("lan", &[]),
        zone("wan", &[]),
    ])));
    screen.apply_action(&Action::FirewallPoliciesUpdated(Arc::new(policies)));
    screen
}

/// `lan → wan` holding `a b [sys] c`, and `iot → wan` holding `x`.
pub(super) fn busy_screen() -> FirewallScreen {
    let lan = ("lan", "wan");
    screen_with(vec![
        user("a", lan, 10_000),
        user("b", lan, 10_001),
        system("sys", lan, 30_000),
        user("c", lan, 30_001),
        user("x", ("iot", "wan"), 10_000),
    ])
}

pub(super) fn press(screen: &mut FirewallScreen, code: KeyCode) -> Option<Action> {
    screen.handle_key_input(KeyEvent::from(code))
}

pub(super) fn names(screen: &FirewallScreen) -> String {
    let view = screen.policy_view();
    let names: Vec<&str> = view.iter().map(|policy| policy.name.as_str()).collect();
    names.join(" ")
}

pub(super) fn draw(screen: &FirewallScreen, width: u16, height: u16) -> Vec<String> {
    render_lines(width, height, |frame, area| screen.render(frame, area))
}
