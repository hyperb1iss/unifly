use std::time::Duration;

use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use unifly_api::model::EntityOrigin;
use unifly_api::{Controller, DemoFleetOptions, EntityId};

use crate::tui::action::{Action, Notification, NotificationLevel, RuleSwitch};
use crate::tui::app::App;
use crate::tui::app::firewall::tests::{SITE, controller, mount_refresh, page};

/// Process queued actions until the switch reports how it went.
async fn finished(app: &mut App) -> Notification {
    loop {
        let action = tokio::time::timeout(Duration::from_secs(10), app.action_rx.recv())
            .await
            .expect("the switch reports back")
            .expect("the action channel stays open");
        app.process_action(&action).expect("action");
        if let Action::OpFinished { notification, .. } = action {
            return notification;
        }
    }
}

fn flip(app: &mut App, id: &EntityId, switch: RuleSwitch, on: bool) {
    app.process_action(&Action::SetRuleSwitch {
        id: id.clone(),
        switch,
        on,
    })
    .expect("switch handling");
}

async fn demo() -> Controller {
    let demo = Controller::demo(DemoFleetOptions::default());
    demo.connect().await.expect("demo fleet connects");
    demo
}

#[tokio::test]
async fn a_switch_that_took_says_so_once_the_store_shows_it() {
    let demo = demo().await;
    let policy = demo
        .firewall_policies_snapshot()
        .iter()
        .find(|policy| policy.origin == Some(EntityOrigin::UserDefined) && policy.enabled)
        .cloned()
        .expect("an enabled user policy");
    let mut app = App::new(Some(demo.clone()), None, false, true);

    flip(&mut app, &policy.id, RuleSwitch::PolicyEnabled, false);
    assert_eq!(
        app.notices.messages(),
        [format!("Disabling {}\u{2026}", policy.name)]
    );

    let notification = finished(&mut app).await;
    assert_eq!(notification.level, NotificationLevel::Success);
    assert_eq!(notification.message, format!("Disabled {}", policy.name));
    let now = demo
        .firewall_policies_snapshot()
        .iter()
        .find(|other| other.id == policy.id)
        .map(|policy| policy.enabled);
    assert_eq!(now, Some(false));
}

#[tokio::test]
async fn logging_reaches_a_built_in_policy_and_acl_and_nat_rules_toggle() {
    let demo = demo().await;
    let system = demo
        .firewall_policies_snapshot()
        .iter()
        .find(|policy| policy.origin == Some(EntityOrigin::SystemDefined))
        .cloned()
        .expect("a built-in policy");
    let mut app = App::new(Some(demo.clone()), None, false, true);

    let on = !system.logging_enabled;
    flip(&mut app, &system.id, RuleSwitch::PolicyLogging, on);
    let notification = finished(&mut app).await;
    let state = if on { "on" } else { "off" };
    assert_eq!(
        notification.message,
        format!("Logging is {state} for {}", system.name)
    );

    let acl = demo.acl_rules_snapshot()[0].clone();
    flip(&mut app, &acl.id, RuleSwitch::AclEnabled, !acl.enabled);
    assert_eq!(
        finished(&mut app).await.level,
        NotificationLevel::Success,
        "ACL rule"
    );
    // A snapshot's order is not stable, so the rule is found by its ID.
    let now = demo
        .acl_rules_snapshot()
        .iter()
        .find(|rule| rule.id == acl.id)
        .map(|rule| rule.enabled);
    assert_eq!(now, Some(!acl.enabled));

    let nat = demo
        .nat_policies_snapshot()
        .iter()
        .find(|rule| rule.origin != Some(EntityOrigin::SystemDefined))
        .cloned()
        .expect("a NAT rule an admin made");
    flip(&mut app, &nat.id, RuleSwitch::NatEnabled, !nat.enabled);
    assert_eq!(
        finished(&mut app).await.level,
        NotificationLevel::Success,
        "NAT rule"
    );
}

#[tokio::test]
async fn a_rule_that_is_gone_is_reported_and_nothing_is_sent() {
    let demo = demo().await;
    let mut app = App::new(Some(demo), None, false, true);

    flip(
        &mut app,
        &EntityId::Uuid(Uuid::from_u128(0xdead)),
        RuleSwitch::PolicyEnabled,
        false,
    );

    assert_eq!(
        app.notices.messages(),
        ["That rule is no longer on the controller"]
    );
}

const POLICY: u128 = 0xf00d;

fn policy_route() -> String {
    format!(
        "/proxy/network/integration/v1/sites/{SITE}/firewall/policies/{}",
        Uuid::from_u128(POLICY)
    )
}

/// A user policy as the Integration API returns it.
fn wire_policy(enabled: bool, logging: bool) -> serde_json::Value {
    let zone = Uuid::from_u128(0x10);
    json!({
        "id": Uuid::from_u128(POLICY), "name": "Block IoT", "enabled": enabled,
        "index": 10_000, "action": {"type": "BLOCK"},
        "source": {"zoneId": zone}, "destination": {"zoneId": zone},
        "ipProtocolScope": {
            "ipVersion": "IPV4",
            "protocolFilter": {"type": "NAMED_PROTOCOL", "protocol": {"name": "TCP"}},
        },
        "loggingEnabled": logging, "reportFlowEnabled": false,
        "metadata": {"origin": "USER_DEFINED"},
    })
}

/// A controller holding one policy in the state `held` whatever is
/// written to it, with that policy loaded into the store.
async fn controller_serving(server: &MockServer, held: &serde_json::Value) -> Controller {
    let controller = controller(server).await;
    mount_refresh(server).await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/proxy/network/integration/v1/sites/{SITE}/firewall/policies"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(std::slice::from_ref(held))))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(policy_route()))
        .respond_with(ResponseTemplate::new(200).set_body_json(held.clone()))
        .mount(server)
        .await;
    controller.full_refresh().await.expect("first refresh");
    controller
}

/// The controller answers 200 and the policy stays as it was. The toast
/// must say that, not "Disabled".
#[tokio::test]
async fn a_switch_the_controller_accepted_but_did_not_apply_is_an_error() {
    let server = MockServer::start().await;
    let held = wire_policy(true, false);
    let controller = controller_serving(&server, &held).await;
    let mut written = held.clone();
    for read_only in ["id", "index", "metadata"] {
        written.as_object_mut().expect("object").remove(read_only);
    }
    written["enabled"] = json!(false);
    Mock::given(method("PUT"))
        .and(path(policy_route()))
        .and(body_json(&written))
        .respond_with(ResponseTemplate::new(200).set_body_json(held.clone()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(policy_route()))
        .respond_with(ResponseTemplate::new(200).set_body_json(held.clone()))
        .expect(0)
        .mount(&server)
        .await;
    let mut app = App::new(Some(controller), None, false, true);

    flip(
        &mut app,
        &EntityId::Uuid(Uuid::from_u128(POLICY)),
        RuleSwitch::PolicyEnabled,
        false,
    );

    let notification = finished(&mut app).await;
    assert_eq!(notification.level, NotificationLevel::Error);
    assert_eq!(
        notification.message,
        "The controller accepted the change, but Block IoT is still enabled"
    );
    server.verify().await;
}

#[tokio::test]
async fn logging_is_patched_with_the_one_field_and_checked_the_same_way() {
    let server = MockServer::start().await;
    let held = wire_policy(true, false);
    let controller = controller_serving(&server, &held).await;
    Mock::given(method("PATCH"))
        .and(path(policy_route()))
        .and(body_json(json!({"loggingEnabled": true})))
        .respond_with(ResponseTemplate::new(200).set_body_json(held.clone()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(policy_route()))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let mut app = App::new(Some(controller), None, false, true);

    flip(
        &mut app,
        &EntityId::Uuid(Uuid::from_u128(POLICY)),
        RuleSwitch::PolicyLogging,
        true,
    );

    let notification = finished(&mut app).await;
    assert_eq!(notification.level, NotificationLevel::Error);
    assert_eq!(
        notification.message,
        "The controller accepted the change, but logging is still off for Block IoT"
    );
    server.verify().await;
}
