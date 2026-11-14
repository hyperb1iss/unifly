use std::sync::Arc;

use crossterm::event::KeyCode;

use super::super::fixtures::{acl_rule, busy_screen, names, press, screen_with, user, zone};
use super::super::reorder::Stage;
use super::*;
use crate::tui::component::Component;

#[test]
fn select_clamps_to_active_tab_length() {
    let mut screen = FirewallScreen::new();
    screen.zones = Arc::new(vec![zone("lan", &[]), zone("iot", &[])]);
    screen.sub_tab = FirewallSubTab::Zones;

    screen.select(99);

    assert_eq!(screen.zone_table.selected(), Some(1));
    assert_eq!(screen.policy_table.selected(), None);
}

#[test]
fn selected_index_tracks_active_tab() {
    let mut screen = FirewallScreen::new();
    screen.policy_table.select(Some(2));
    screen.zone_table.select(Some(1));
    screen.acl_rules = Arc::new(
        (0..4)
            .map(|n| acl_rule(&format!("rule-{n}"), true))
            .collect(),
    );
    screen.acl_table.select(Some(3));

    screen.sub_tab = FirewallSubTab::Policies;
    assert_eq!(screen.selected_index(), 2);
    screen.sub_tab = FirewallSubTab::Zones;
    assert_eq!(screen.selected_index(), 1);
    screen.sub_tab = FirewallSubTab::AclRules;
    assert_eq!(screen.selected_index(), 3);
}

fn zoned_fixture() -> Vec<Arc<FirewallPolicy>> {
    vec![
        user("wan-lan default", ("wan", "lan"), i32::MAX),
        user("lan-wan second", ("lan", "wan"), 10_001),
        user("iot-lan block", ("iot", "lan"), 10_000),
        user("lan-wan first", ("lan", "wan"), 10_000),
        user("wan-lan allow", ("wan", "lan"), 10_000),
    ]
}

#[test]
fn policies_list_in_evaluation_order_per_zone_pair() {
    let screen = screen_with(zoned_fixture());
    assert_eq!(
        names(&screen),
        "iot-lan block lan-wan first lan-wan second wan-lan allow wan-lan default"
    );
}

#[test]
fn policy_selection_follows_the_policy_across_refreshes() {
    let mut policies = zoned_fixture();
    let mut screen = screen_with(policies.clone());
    screen.select(2);
    assert_eq!(
        screen.selected_policy().map(|policy| policy.name.clone()),
        Some("lan-wan second".into())
    );

    policies.push(user("iot-lan allow", ("iot", "lan"), 9_000));
    screen.apply_action(&Action::FirewallPoliciesUpdated(Arc::new(policies)));

    assert_eq!(screen.policy_table.selected(), Some(3));
    assert_eq!(
        screen.selected_policy().map(|policy| policy.name.clone()),
        Some("lan-wan second".into())
    );
}

#[test]
fn first_zones_regroup_policies_without_moving_the_cursor() {
    let mut screen = FirewallScreen::new();
    screen.apply_action(&Action::FirewallPoliciesUpdated(Arc::new(zoned_fixture())));
    screen.apply_action(&Action::FirewallZonesUpdated(Arc::new(vec![
        zone("lan", &[]),
        zone("wan", &[]),
        zone("iot", &[]),
    ])));

    assert_eq!(screen.policy_table.selected(), Some(0));
    assert_eq!(
        screen.policy_view().first().map(|p| p.name.clone()),
        Some("iot-lan block".into())
    );
}

#[test]
fn a_picked_up_policy_moves_on_screen_and_nothing_is_sent_until_enter() {
    let mut screen = busy_screen();
    assert_eq!(names(&screen), "x a b sys c");
    screen.select(2);

    // A stray shifted key, as Caps Lock makes of `k`, writes nothing.
    assert!(press(&mut screen, KeyCode::Char('K')).is_none());
    assert!(screen.is_drafting());
    assert_eq!(names(&screen), "x b a sys c");
    assert_eq!(
        screen.selected_policy().map(|policy| policy.name.clone()),
        Some("b".into()),
        "the cursor rides with the policy"
    );

    // Held keys pile up as places, not as writes, and stop at the edge.
    for _ in 0..4 {
        assert!(press(&mut screen, KeyCode::Char('J')).is_none());
    }
    assert_eq!(names(&screen), "x a sys c b");
    assert!(press(&mut screen, KeyCode::Char('k')).is_none());
    assert_eq!(
        names(&screen),
        "x a sys b c",
        "one step crosses the fixed policy"
    );

    let sent = press(&mut screen, KeyCode::Enter);
    assert!(
        matches!(&sent, Some(Action::MovePolicy { policy, steps: 1 }) if policy.name == "b"),
        "{sent:?}"
    );
    assert!(!screen.is_drafting());
    assert_eq!(
        names(&screen),
        "x a sys b c",
        "the row holds its place while it lands"
    );
}

#[test]
fn esc_puts_a_picked_up_policy_back() {
    let mut screen = busy_screen();
    screen.select(1);
    press(&mut screen, KeyCode::Char('J'));
    assert_eq!(names(&screen), "x b a sys c");

    let consumed = press(&mut screen, KeyCode::Esc);
    assert!(matches!(consumed, Some(Action::Invalidate)), "{consumed:?}");
    assert_eq!(names(&screen), "x a b sys c");
    assert_eq!(
        screen.policy_table.selected(),
        Some(1),
        "the cursor is back on it"
    );
    assert!(screen.reorder.is_none());

    // Moved and then walked back to where it started: nothing to send.
    press(&mut screen, KeyCode::Char('J'));
    press(&mut screen, KeyCode::Char('K'));
    let nothing = press(&mut screen, KeyCode::Enter);
    assert!(matches!(nothing, Some(Action::Invalidate)), "{nothing:?}");
    assert!(screen.reorder.is_none());
}

#[test]
fn a_move_holds_its_place_until_the_fresh_order_arrives_or_is_refused() {
    let mut screen = busy_screen();
    screen.select(1);
    press(&mut screen, KeyCode::Char('J'));
    press(&mut screen, KeyCode::Enter);

    // While it is in flight another move is held, not dropped silently.
    let held = press(&mut screen, KeyCode::Char('K'));
    assert!(
        matches!(&held, Some(Action::Notify(notice)) if notice.message == "Still moving a"),
        "{held:?}"
    );

    screen.apply_action(&Action::PolicyMoveSettled { moved: true });
    assert_eq!(
        names(&screen),
        "x b a sys c",
        "no jump back before the refresh"
    );
    let lan = ("lan", "wan");
    screen.apply_action(&Action::FirewallPoliciesUpdated(Arc::new(vec![
        user("b", lan, 10_000),
        user("a", lan, 10_001),
        super::super::fixtures::system("sys", lan, 30_000),
        user("c", lan, 30_001),
        user("x", ("iot", "wan"), 10_000),
    ])));
    assert!(screen.reorder.is_none());
    assert_eq!(names(&screen), "x b a sys c");
    assert_eq!(
        screen.selected_policy().map(|policy| policy.name.clone()),
        Some("a".into())
    );

    // A refused move goes back, cursor and all.
    press(&mut screen, KeyCode::Char('K'));
    press(&mut screen, KeyCode::Enter);
    screen.apply_action(&Action::PolicyMoveSettled { moved: false });
    assert!(screen.reorder.is_none());
    assert_eq!(names(&screen), "x b a sys c");
    assert_eq!(
        screen.selected_policy().map(|policy| policy.name.clone()),
        Some("a".into())
    );
}

#[test]
fn a_landed_move_lets_go_when_no_refresh_comes() {
    let mut screen = busy_screen();
    screen.select(1);
    press(&mut screen, KeyCode::Char('J'));
    press(&mut screen, KeyCode::Enter);
    screen.apply_action(&Action::PolicyMoveSettled { moved: true });
    assert!(matches!(
        screen.reorder.as_ref().map(|reorder| reorder.stage),
        Some(Stage::Landed { .. })
    ));

    for _ in 0..12 {
        screen.apply_action(&Action::Tick);
    }
    assert!(screen.reorder.is_none());
    assert_eq!(
        names(&screen),
        "x a b sys c",
        "the controller's order shows again"
    );
}

#[test]
fn switching_tabs_or_controllers_drops_a_picked_up_policy() {
    let mut screen = busy_screen();
    screen.select(1);
    press(&mut screen, KeyCode::Char('J'));
    screen.apply_action(&Action::FirewallSubTab(FirewallSubTab::Zones));
    assert!(screen.reorder.is_none());

    screen.apply_action(&Action::FirewallSubTab(FirewallSubTab::Policies));
    press(&mut screen, KeyCode::Char('J'));
    press(&mut screen, KeyCode::Enter);
    screen.set_focused(true);
    screen.apply_action(&Action::ControllerChanged);
    assert!(screen.reorder.is_none() && !screen.detail_open);
    assert!(screen.policies.is_empty() && screen.zones.is_empty());
    assert!(screen.access.is_none());
    assert!(screen.focused, "it is still the screen on show");
}
