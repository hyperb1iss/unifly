use std::sync::Arc;

use crossterm::event::KeyCode;
use unifly_api::Command;

use serde_json::json;

use super::super::fixtures::{
    acl_rule, busy_screen, draw, hints, nat_rule, policy, press, screen_with, user,
};
use crate::tui::action::{Action, FirewallSubTab, RuleSwitch};
use crate::tui::component::Component;
use crate::tui::screens::firewall::FirewallScreen;
use crate::tui::widgets::click::click_label;

/// The command a confirmation runs on yes, and its question.
fn confirmed(action: Option<Action>) -> (String, Command) {
    let Some(Action::ShowConfirm(confirm)) = action else {
        panic!("a confirmation, not {action:?}");
    };
    let Action::Run { command, .. } = *confirm.action else {
        panic!("yes runs a command");
    };
    assert!(confirm.detail.is_some(), "the question says what yes costs");
    (confirm.prompt, *command)
}

/// The switch a confirmation flips on yes, and its question.
fn switched(action: Option<Action>) -> (String, RuleSwitch, bool) {
    let Some(Action::ShowConfirm(confirm)) = action else {
        panic!("a confirmation, not {action:?}");
    };
    let Action::SetRuleSwitch { switch, on, .. } = *confirm.action else {
        panic!("yes flips a switch, not {:?}", confirm.action);
    };
    assert!(confirm.detail.is_some(), "the question says what yes costs");
    (confirm.prompt, switch, on)
}

fn warning(action: Option<Action>) -> String {
    match action {
        Some(Action::Notify(notice)) => notice.message,
        other => panic!("a refusal, not {other:?}"),
    }
}

#[test]
fn clicking_a_sub_tab_switches_to_it() {
    let mut screen = FirewallScreen::new();
    for (label, tab) in [
        ("Zones", FirewallSubTab::Zones),
        ("ACL Rules", FirewallSubTab::AclRules),
        ("NAT", FirewallSubTab::NatPolicies),
        ("Policies", FirewallSubTab::Policies),
    ] {
        let rows = draw(&screen, 100, 12);
        let action = screen.handle_mouse_input(click_label(&rows, "ACL Rules", label));
        assert!(
            matches!(action, Some(Action::FirewallSubTab(clicked)) if clicked == tab),
            "{label}: {action:?}"
        );
        assert_eq!(screen.sub_tab, tab);
    }

    let rows = draw(&screen, 100, 12);
    let action = screen.handle_mouse_input(click_label(&rows, "ACL Rules", "Policies"));
    assert!(action.is_none(), "the active tab is already showing");
}

#[test]
fn arrows_and_letters_step_the_sub_tabs_both_ways_around() {
    let mut screen = FirewallScreen::new();
    press(&mut screen, KeyCode::Left);
    assert_eq!(screen.sub_tab, FirewallSubTab::NatPolicies);
    press(&mut screen, KeyCode::Char('l'));
    assert_eq!(screen.sub_tab, FirewallSubTab::Policies);
    press(&mut screen, KeyCode::Right);
    assert_eq!(screen.sub_tab, FirewallSubTab::Zones);
    press(&mut screen, KeyCode::Char('h'));
    assert_eq!(screen.sub_tab, FirewallSubTab::Policies);
}

#[test]
fn enabling_and_deleting_ask_first_and_logging_does_not() {
    let mut screen = busy_screen();
    screen.select(1);

    let toggle = switched(press(&mut screen, KeyCode::Char('E')));
    assert_eq!(
        toggle,
        ("Disable policy a?".into(), RuleSwitch::PolicyEnabled, false)
    );

    let logging = press(&mut screen, KeyCode::Char('L'));
    assert!(
        matches!(
            logging,
            Some(Action::SetRuleSwitch {
                switch: RuleSwitch::PolicyLogging,
                on: true,
                ..
            })
        ),
        "logging changes at once: {logging:?}"
    );

    let (prompt, command) = confirmed(press(&mut screen, KeyCode::Char('D')));
    assert_eq!(prompt, "Delete policy a?");
    assert!(matches!(command, Command::DeleteFirewallPolicy { .. }));
}

#[test]
fn a_system_defined_policy_takes_logging_and_refuses_the_rest_without_a_round_trip() {
    let mut screen = busy_screen();
    screen.select(3);

    for key in ['K', 'J'] {
        let refusal = warning(press(&mut screen, KeyCode::Char(key)));
        assert_eq!(refusal, "sys is system-defined and keeps its place");
    }
    assert!(screen.reorder.is_none());
    let refusal = warning(press(&mut screen, KeyCode::Char('D')));
    assert_eq!(refusal, "sys is system-defined and cannot be deleted");
    let refusal = warning(press(&mut screen, KeyCode::Char('E')));
    assert_eq!(
        refusal,
        "sys is system-defined and cannot be switched on or off here"
    );

    // Logging is the one switch the controller takes on its own policies.
    let logging = press(&mut screen, KeyCode::Char('L'));
    assert!(
        matches!(
            logging,
            Some(Action::SetRuleSwitch {
                switch: RuleSwitch::PolicyLogging,
                ..
            })
        ),
        "{logging:?}"
    );
    assert_eq!(hints(&screen), "Enter detail  L log on");
}

/// A policy the controller derived from another, such as an allow rule's
/// return traffic, is not an admin's to move, switch, or delete.
#[test]
fn a_derived_policy_refuses_to_move_switch_or_be_deleted() {
    let lan = ("lan", "wan");
    let mut screen = screen_with(vec![
        user("a", lan, 10_000),
        policy("ret", lan, 10_001, &json!({ "origin": "Derived" })),
        user("b", lan, 10_002),
    ]);
    screen.select(1);
    assert_eq!(
        screen.selected_policy().map(|policy| policy.name.clone()),
        Some("ret".into())
    );

    for (key, refusal) in [
        ('K', "ret is derived from another rule and keeps its place"),
        (
            'E',
            "ret is derived from another rule and cannot be switched on or off here",
        ),
        (
            'D',
            "ret is derived from another rule and cannot be deleted",
        ),
    ] {
        assert_eq!(warning(press(&mut screen, KeyCode::Char(key))), refusal);
    }
    assert!(screen.reorder.is_none());
}

#[test]
fn a_policy_with_nowhere_to_go_says_so_and_is_not_picked_up() {
    let mut screen = busy_screen();
    screen.select(1);
    let edge = warning(press(&mut screen, KeyCode::Char('K')));
    assert_eq!(edge, "a is already first in its zone pair");
    assert!(screen.reorder.is_none());

    screen.select(0);
    let alone = warning(press(&mut screen, KeyCode::Char('J')));
    assert_eq!(alone, "x is already last in its zone pair");
}

#[test]
fn acl_and_nat_rules_toggle_and_delete_with_a_question() {
    let mut screen = FirewallScreen::new();
    screen.apply_action(&Action::AclRulesUpdated(Arc::new(vec![acl_rule(
        "Quarantine",
        false,
    )])));
    screen.apply_action(&Action::NatPoliciesUpdated(Arc::new(vec![
        nat_rule("Egress", "UserDefined"),
        nat_rule("Masquerade", "SystemDefined"),
    ])));

    screen.apply_action(&Action::FirewallSubTab(FirewallSubTab::AclRules));
    let toggle = switched(press(&mut screen, KeyCode::Char('E')));
    assert_eq!(
        toggle,
        (
            "Enable ACL rule Quarantine?".into(),
            RuleSwitch::AclEnabled,
            true
        )
    );
    let (prompt, command) = confirmed(press(&mut screen, KeyCode::Char('D')));
    assert_eq!(prompt, "Delete ACL rule Quarantine?");
    assert!(matches!(command, Command::DeleteAclRule { .. }));
    assert!(
        press(&mut screen, KeyCode::Char('L')).is_none(),
        "ACL rules have no logging key"
    );
    assert!(
        press(&mut screen, KeyCode::Char('K')).is_none(),
        "nor a move key"
    );

    screen.apply_action(&Action::FirewallSubTab(FirewallSubTab::NatPolicies));
    let toggle = switched(press(&mut screen, KeyCode::Char('E')));
    assert_eq!(
        toggle,
        (
            "Disable NAT rule Egress?".into(),
            RuleSwitch::NatEnabled,
            false
        )
    );
    press(&mut screen, KeyCode::Char('j'));
    let refusal = warning(press(&mut screen, KeyCode::Char('D')));
    assert_eq!(
        refusal,
        "Masquerade is system-defined and cannot be deleted"
    );
    let refusal = warning(press(&mut screen, KeyCode::Char('E')));
    assert_eq!(
        refusal,
        "Masquerade is system-defined and cannot be switched on or off here"
    );
    assert_eq!(
        hints(&screen),
        "Enter detail",
        "a rule the controller owns offers no write"
    );

    screen.apply_action(&Action::FirewallSubTab(FirewallSubTab::Zones));
    for key in ['E', 'D', 'K', 'L'] {
        assert!(
            press(&mut screen, KeyCode::Char(key)).is_none(),
            "{key} on a zone"
        );
    }
}

#[test]
fn enter_opens_the_detail_and_esc_closes_it_before_going_back() {
    let mut screen = busy_screen();
    assert!(press(&mut screen, KeyCode::Enter).is_none());
    assert!(screen.detail_open);
    let consumed = press(&mut screen, KeyCode::Esc);
    assert!(matches!(consumed, Some(Action::Invalidate)), "{consumed:?}");
    assert!(
        press(&mut screen, KeyCode::Esc).is_none(),
        "the app goes back"
    );

    let mut empty = FirewallScreen::new();
    press(&mut empty, KeyCode::Enter);
    assert!(!empty.detail_open, "no row, no detail");
}

#[test]
fn a_second_tap_on_the_selected_row_opens_its_detail() {
    let mut screen = busy_screen();
    let rows = draw(&screen, 100, 24);
    screen.handle_mouse_input(click_label(&rows, " b ", "b"));
    assert_eq!(
        screen.selected_policy().map(|policy| policy.name.clone()),
        Some("b".into())
    );
    assert!(!screen.detail_open);
    let rows = draw(&screen, 100, 24);
    screen.handle_mouse_input(click_label(&rows, " b ", "b"));
    assert!(screen.detail_open);
}

#[test]
fn help_lists_the_keys_of_the_tab_and_mode_on_screen() {
    let mut screen = busy_screen();
    let keys = |screen: &FirewallScreen| -> Vec<&str> {
        screen.help().into_iter().map(|(key, _)| key).collect()
    };
    for key in ["Enter", "E", "L", "K/J", "D"] {
        assert!(keys(&screen).contains(&key), "{key}");
    }
    screen.select(3);
    assert!(
        keys(&screen).contains(&"L"),
        "a built-in policy takes logging"
    );
    for key in ["E", "K/J", "D"] {
        assert!(!keys(&screen).contains(&key), "{key} on a built-in policy");
    }
    screen.apply_action(&Action::FirewallSubTab(FirewallSubTab::Zones));
    assert!(!keys(&screen).contains(&"E"), "zones are read-only");

    screen.apply_action(&Action::FirewallSubTab(FirewallSubTab::Policies));
    screen.select(1);
    press(&mut screen, KeyCode::Char('J'));
    assert_eq!(keys(&screen).len(), 3, "a picked-up policy has three keys");
}
