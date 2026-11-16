use std::time::Duration;

use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;

fn uuid(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn id(n: u128) -> EntityId {
    EntityId::Uuid(uuid(n))
}

fn halves(before: &[u128], after: &[u128]) -> PairOrder {
    PairOrder {
        before: before.iter().copied().map(id).collect(),
        after: after.iter().copied().map(id).collect(),
    }
}

/// A move the table showed with the pair's system-defined policies in
/// view: both orders name their halves.
fn shown(started: (&[u128], &[u128]), drafted: (&[u128], &[u128])) -> PolicyMove {
    PolicyMove {
        started: halves(started.0, started.1),
        drafted: halves(drafted.0, drafted.1),
        split_shown: true,
    }
}

/// A move through a pair whose system-defined policies the table did not
/// show: one flat order each.
fn flat_move(started: &[u128], drafted: &[u128]) -> PolicyMove {
    PolicyMove {
        started: halves(started, &[]),
        drafted: halves(drafted, &[]),
        split_shown: false,
    }
}

#[test]
fn the_drafted_order_is_written_when_the_controller_has_the_order_it_started_from() {
    let fetched = halves(&[1, 2, 3], &[4, 5]);
    let swap = shown((&[1, 2, 3], &[4, 5]), (&[2, 1, 3], &[4, 5]));
    assert_eq!(
        planned(&fetched, &id(2), &swap),
        Planned::Write(halves(&[2, 1, 3], &[4, 5]))
    );
    let cross = shown((&[1, 2, 3], &[4, 5]), (&[1, 2], &[3, 4, 5]));
    assert_eq!(
        planned(&fetched, &id(3), &cross),
        Planned::Write(halves(&[1, 2], &[3, 4, 5]))
    );
}

#[test]
fn an_order_that_changed_under_the_draft_is_not_written_over() {
    let swap = shown((&[1, 2, 3], &[4, 5]), (&[2, 1, 3], &[4, 5]));
    // Someone reordered the pair, moved a policy across the system
    // block, or added one, since the table last heard.
    for fetched in [
        halves(&[1, 3, 2], &[4, 5]),
        halves(&[1, 2], &[3, 4, 5]),
        halves(&[1, 2, 3, 6], &[4, 5]),
    ] {
        assert_eq!(
            planned(&fetched, &id(2), &swap),
            Planned::Stale,
            "{fetched:?}"
        );
    }
    let flat = flat_move(&[1, 2, 3, 4], &[2, 1, 3, 4]);
    assert_eq!(
        planned(&halves(&[2, 1], &[3, 4]), &id(2), &flat),
        Planned::Stale
    );
}

#[test]
fn a_policy_outside_the_ordering_and_a_move_already_made_write_nothing() {
    let fetched = halves(&[1, 2, 3], &[4, 5]);
    let system = shown((&[1, 2, 3], &[4, 5]), (&[2, 1, 3], &[4, 5]));
    assert_eq!(planned(&fetched, &id(9), &system), Planned::Fixed);
    let nowhere = shown((&[1, 2, 3], &[4, 5]), (&[1, 2, 3], &[4, 5]));
    assert_eq!(planned(&fetched, &id(2), &nowhere), Planned::Unchanged);
}

/// Without the system-defined policies on screen the table cannot say
/// which half a policy is in. The controller's own split decides, and a
/// swap stays a swap instead of a jump across a block nobody saw.
#[test]
fn a_move_through_a_pair_shown_flat_keeps_the_controllers_split() {
    let fetched = halves(&[1, 2], &[3, 4]);
    let plan = |drafted: &[u128]| flat_move(&[1, 2, 3, 4], drafted);

    // Within a half.
    assert_eq!(
        planned(&fetched, &id(2), &plan(&[2, 1, 3, 4])),
        Planned::Write(halves(&[2, 1], &[3, 4]))
    );
    // Past a policy of the other half: it joins that half.
    assert_eq!(
        planned(&fetched, &id(2), &plan(&[1, 3, 2, 4])),
        Planned::Write(halves(&[1], &[3, 2, 4]))
    );
    assert_eq!(
        planned(&fetched, &id(3), &plan(&[3, 1, 2, 4])),
        Planned::Write(halves(&[3, 1, 2], &[4]))
    );
    // To the far end.
    assert_eq!(
        planned(&fetched, &id(1), &plan(&[2, 3, 4, 1])),
        Planned::Write(halves(&[2], &[3, 4, 1]))
    );
}

#[test]
fn a_move_is_told_as_the_table_showed_it() {
    let told =
        |plan: &PolicyMove, n| moved_message("p", describe(&plan.started, &plan.drafted, &id(n)));
    let swap = shown((&[1, 2, 3], &[4]), (&[2, 1, 3], &[4]));
    assert_eq!(told(&swap, 2), "Moved p up");
    let cross = shown((&[1, 2, 3], &[4]), (&[1, 2], &[3, 4]));
    assert_eq!(told(&cross, 3), "Moved p below the system-defined policies");
    let back = shown((&[1, 2], &[3, 4]), (&[1, 2, 3], &[4]));
    assert_eq!(told(&back, 3), "Moved p above the system-defined policies");
    let far = shown((&[1, 2, 3], &[4]), (&[2, 3], &[1, 4]));
    assert_eq!(
        told(&far, 1),
        "Moved p down 3 places, past the system-defined policies"
    );
    let flat = flat_move(&[1, 2, 3], &[2, 3, 1]);
    assert_eq!(told(&flat, 1), "Moved p down 2 places");
}

pub(in crate::tui::app) const SITE: &str = "550e8400-e29b-41d4-a716-446655440000";
const SOURCE_ZONE: u128 = 0x10;
const DESTINATION_ZONE: u128 = 0x20;

fn policy(n: u128, zones: Option<(u128, u128)>) -> Arc<FirewallPolicy> {
    let zone = |zone: Option<u128>| zone.map(|zone| uuid(zone).to_string());
    Arc::new(
        serde_json::from_value(json!({
            "id": uuid(n).to_string(),
            "name": format!("policy-{n}"),
            "description": null,
            "enabled": true,
            "index": 10_000,
            "action": "Allow",
            "ip_version": "Both",
            "source": { "zone_id": zone(zones.map(|pair| pair.0)), "filter": null },
            "destination": { "zone_id": zone(zones.map(|pair| pair.1)), "filter": null },
            "source_summary": null,
            "destination_summary": null,
            "protocol_summary": null,
            "schedule": null,
            "ipsec_mode": null,
            "connection_states": [],
            "logging_enabled": false,
            "origin": "UserDefined"
        }))
        .expect("policy fixture should deserialize"),
    )
}

/// An API-key controller on `server`, connected without loading data,
/// that refreshes on its own every `refresh_interval_secs` (never, when
/// zero).
pub(in crate::tui::app) async fn controller_refreshing(
    server: &MockServer,
    refresh_interval_secs: u64,
) -> Controller {
    for (route, status) in [("/api/auth/login", 401), ("/api/login", 404)] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(status))
            .mount(server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/proxy/network/integration/v1/sites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "offset": 0, "limit": 50, "count": 1, "totalCount": 1,
            "data": [{ "id": SITE, "internalReference": "default", "name": "Default" }],
        })))
        .mount(server)
        .await;

    let config = unifly_api::ControllerConfig {
        url: server.uri().parse().expect("mock server URL"),
        auth: unifly_api::AuthCredentials::ApiKey("test-key".to_owned().into()),
        site: "default".into(),
        tls: unifly_api::TlsVerification::DangerAcceptInvalid,
        timeout: Duration::from_secs(5),
        refresh_interval_secs,
        polling_interval_secs: 0,
        websocket_enabled: false,
        ..unifly_api::ControllerConfig::default()
    };
    let controller = Controller::new(config);
    controller
        .connect_lightweight()
        .await
        .expect("mock controller connects");
    controller
}

pub(in crate::tui::app) async fn controller(server: &MockServer) -> Controller {
    controller_refreshing(server, 0).await
}

pub(in crate::tui::app) fn page(items: &[serde_json::Value]) -> serde_json::Value {
    json!({
        "offset": 0, "limit": 200, "count": items.len(), "totalCount": items.len(),
        "data": items,
    })
}

/// Empty pages for the collections a refresh cannot do without, so a
/// refresh after a write succeeds.
pub(in crate::tui::app) async fn mount_refresh(server: &MockServer) {
    for collection in [
        "devices",
        "clients",
        "networks",
        "wifi/broadcasts",
        "traffic-matching-lists",
    ] {
        Mock::given(method("GET"))
            .and(path(format!(
                "/proxy/network/integration/v1/sites/{SITE}/{collection}"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(page(&[])))
            .mount(server)
            .await;
    }
}

/// Serve the zone pair's ordering as policies 1-3 before the system
/// block and 4 after it, and accept exactly `writes` writes, each of
/// `moved`, the two halves after the move.
pub(in crate::tui::app) async fn mount_ordering_expecting(
    server: &MockServer,
    moved: (&[u128], &[u128]),
    writes: u64,
) {
    let zone_query = |mock: wiremock::MockBuilder| {
        mock.and(query_param(
            "sourceFirewallZoneId",
            uuid(SOURCE_ZONE).to_string(),
        ))
        .and(query_param(
            "destinationFirewallZoneId",
            uuid(DESTINATION_ZONE).to_string(),
        ))
    };
    zone_query(Mock::given(method("GET")).and(path(ordering_route())))
        .respond_with(ResponseTemplate::new(200).set_body_json(ordering_body(&[1, 2, 3], &[4])))
        .mount(server)
        .await;
    zone_query(Mock::given(method("PUT")).and(path(ordering_route())))
        .and(body_json(ordering_body(moved.0, moved.1)))
        .respond_with(ResponseTemplate::new(200).set_body_json(ordering_body(moved.0, moved.1)))
        .expect(writes)
        .mount(server)
        .await;
}

async fn mount_ordering(server: &MockServer, moved: (&[u128], &[u128])) {
    mount_ordering_expecting(server, moved, 1).await;
}

/// What the controller serves: policies 1-3, the system block, policy 4.
const SERVED: (&[u128], &[u128]) = (&[1, 2, 3], &[4]);
/// Policy 2 one step down, from 1-3 before the system block.
const TWO_DOWN: (&[u128], &[u128]) = (&[1, 3, 2], &[4]);

pub(in crate::tui::app) fn move_down() -> Action {
    move_to(2, TWO_DOWN)
}

/// Move policy `n` of the test zone pair from the served order to
/// `drafted`, as a table showing the system block would ask.
fn move_to(n: u128, drafted: (&[u128], &[u128])) -> Action {
    Action::MovePolicy {
        policy: policy(n, Some((SOURCE_ZONE, DESTINATION_ZONE))),
        plan: Box::new(shown(SERVED, drafted)),
    }
}

fn ordering_route() -> String {
    format!("/proxy/network/integration/v1/sites/{SITE}/firewall/policies/ordering")
}

fn ordering_body(before: &[u128], after: &[u128]) -> serde_json::Value {
    let ids = |ids: &[u128]| -> Vec<String> { ids.iter().map(|n| uuid(*n).to_string()).collect() };
    json!({ "orderedFirewallPolicyIds": {
        "beforeSystemDefined": ids(before),
        "afterSystemDefined": ids(after),
    }})
}

/// How a move ended: the notification that takes over its progress
/// toast, or the refusal shown in its place. Every action on the way
/// is processed, as the app's loop would.
pub(in crate::tui::app) async fn outcome(app: &mut App) -> Notification {
    let in_flight = app
        .notices
        .messages()
        .iter()
        .any(|message| message.starts_with("Moving "));
    if let Some(refusal) = app.notices.latest().filter(|_| !in_flight) {
        return refusal.clone();
    }
    loop {
        let action = tokio::time::timeout(Duration::from_secs(10), app.action_rx.recv())
            .await
            .expect("the move reports back")
            .expect("the action channel stays open");
        app.process_action(&action).expect("action");
        if let Action::OpFinished { notification, .. } = action {
            return notification;
        }
    }
}

#[tokio::test]
async fn a_reorder_writes_the_drafted_order_to_the_controller() {
    let server = MockServer::start().await;
    let controller = controller(&server).await;
    mount_ordering(&server, TWO_DOWN).await;
    mount_refresh(&server).await;

    let mut app = App::new(Some(controller), None, false, true);
    app.process_action(&move_down()).expect("reorder handling");

    let notification = outcome(&mut app).await;
    assert_eq!(notification.level, NotificationLevel::Success);
    assert_eq!(notification.message, "Moved policy-2 down");
    server.verify().await;
}

/// The controller's order moved on since the table last heard: the draft
/// was made on an order that no longer exists, so nothing is written and
/// the table is brought up to date instead.
#[tokio::test]
async fn a_draft_made_on_a_stale_order_writes_nothing_and_refreshes() {
    let server = MockServer::start().await;
    let controller = controller(&server).await;
    mount_ordering_expecting(&server, TWO_DOWN, 0).await;
    Mock::given(method("PUT"))
        .and(path(ordering_route()))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/proxy/network/integration/v1/sites/{SITE}/devices"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(&[])))
        .expect(1..)
        .mount(&server)
        .await;
    mount_refresh(&server).await;
    let mut app = App::new(Some(controller), None, false, true);

    // The table still showed 2 above 1; the controller has 1, 2, 3.
    app.process_action(&Action::MovePolicy {
        policy: policy(2, Some((SOURCE_ZONE, DESTINATION_ZONE))),
        plan: Box::new(shown((&[2, 1, 3], &[4]), (&[1, 2, 3], &[4]))),
    })
    .expect("reorder handling");

    let notification = outcome(&mut app).await;
    assert_eq!(notification.level, NotificationLevel::Warning);
    assert_eq!(
        notification.message,
        "The policy order on the controller is not the one shown, so policy-2 was not \
         moved. The list is up to date now: move it again."
    );
    server.verify().await;
}

#[tokio::test]
async fn a_pair_shown_without_its_system_block_is_written_on_the_controllers_split() {
    let server = MockServer::start().await;
    let controller = controller(&server).await;
    // The table showed 1 2 3 4 with nothing between; 3 goes below 4,
    // which the controller keeps after its system-defined policies.
    mount_ordering(&server, (&[1, 2], &[4, 3])).await;
    mount_refresh(&server).await;
    let mut app = App::new(Some(controller), None, false, true);

    app.process_action(&Action::MovePolicy {
        policy: policy(3, Some((SOURCE_ZONE, DESTINATION_ZONE))),
        plan: Box::new(flat_move(&[1, 2, 3, 4], &[1, 2, 4, 3])),
    })
    .expect("reorder handling");

    let notification = outcome(&mut app).await;
    assert_eq!(notification.message, "Moved policy-3 down");
    server.verify().await;
}

#[tokio::test]
async fn a_move_the_table_cannot_show_yet_says_so() {
    let server = MockServer::start().await;
    let controller = controller(&server).await;
    mount_ordering(&server, TWO_DOWN).await;

    let mut app = App::new(Some(controller), None, false, true);
    app.process_action(&move_down()).expect("reorder handling");

    let notification = outcome(&mut app).await;
    assert_eq!(notification.level, NotificationLevel::Warning);
    assert_eq!(
        notification.message,
        "Moved policy-2 down; the list shows it after the next refresh"
    );
    server.verify().await;
}

#[tokio::test]
async fn a_move_shows_at_once_and_holds_further_presses_until_it_lands() {
    let server = MockServer::start().await;
    let controller = controller(&server).await;
    mount_ordering(&server, TWO_DOWN).await;
    mount_refresh(&server).await;
    let mut app = App::new(Some(controller), None, false, true);

    app.process_action(&move_down()).expect("first press");
    app.process_action(&move_down()).expect("second press");

    assert_eq!(
        app.notices.messages(),
        [
            "Moving policy-2 down\u{2026}",
            "A policy move is still in progress"
        ],
        "the progress shows at once and the second press is held"
    );
    let held = app.action_rx.try_recv().expect("the held move is settled");
    assert!(
        matches!(held, Action::PolicyMoveSettled { moved: false }),
        "the screen puts the held move's row back: {held:?}"
    );
    let landed = outcome(&mut app).await;
    assert_eq!(landed.message, "Moved policy-2 down");
    assert!(
        !app.notices
            .messages()
            .iter()
            .any(|m| m.starts_with("Moving ")),
        "the result takes over the progress toast"
    );
    server.verify().await;
    assert!(
        !app.policy_move.load(std::sync::atomic::Ordering::Acquire),
        "a landed move takes presses again"
    );
}

#[tokio::test]
async fn a_step_across_the_system_block_says_so() {
    let server = MockServer::start().await;
    let controller = controller(&server).await;
    mount_ordering(&server, (&[1, 2], &[3, 4])).await;
    mount_refresh(&server).await;
    let mut app = App::new(Some(controller), None, false, true);

    app.process_action(&move_to(3, (&[1, 2], &[3, 4])))
        .expect("reorder handling");

    let notification = outcome(&mut app).await;
    assert_eq!(
        notification.message,
        "Moved policy-3 below the system-defined policies"
    );
    server.verify().await;
}

#[tokio::test]
async fn a_failed_write_still_refreshes_the_table() {
    let server = MockServer::start().await;
    let controller = controller(&server).await;
    Mock::given(method("GET"))
        .and(path(ordering_route()))
        .respond_with(ResponseTemplate::new(200).set_body_json(ordering_body(&[1, 2, 3], &[4])))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(ordering_route()))
        .respond_with(ResponseTemplate::new(504))
        .mount(&server)
        .await;
    // The write may have landed before it failed, so the table asks
    // the controller again.
    Mock::given(method("GET"))
        .and(path(format!(
            "/proxy/network/integration/v1/sites/{SITE}/devices"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(&[])))
        .expect(1..)
        .mount(&server)
        .await;
    let mut app = App::new(Some(controller), None, false, true);

    app.process_action(&move_down()).expect("reorder handling");

    let notification = outcome(&mut app).await;
    assert_eq!(notification.level, NotificationLevel::Error);
    server.verify().await;
}

#[tokio::test]
async fn a_move_while_disconnected_is_refused_up_front() {
    let server = MockServer::start().await;
    let controller = controller(&server).await;
    controller.disconnect().await;
    let mut app = App::new(Some(controller), None, false, true);

    app.process_action(&move_down()).expect("reorder handling");

    let notification = outcome(&mut app).await;
    assert_eq!(notification.level, NotificationLevel::Error);
    assert_eq!(
        notification.message,
        "Not connected, so policy-2 stays where it is"
    );
    assert!(!app.policy_move.load(atomic::Ordering::Acquire));
    let settled = app.action_rx.try_recv().expect("the screen is told");
    assert!(
        matches!(settled, Action::PolicyMoveSettled { moved: false }),
        "{settled:?}"
    );
}

#[tokio::test]
async fn a_connection_lost_mid_move_says_so() {
    let server = MockServer::start().await;
    let controller = controller(&server).await;
    Mock::given(method("GET"))
        .and(path(ordering_route()))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(ordering_body(&[1, 2, 3], &[4]))
                .set_delay(Duration::from_millis(300)),
        )
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(ordering_route()))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let mut app = App::new(Some(controller.clone()), None, false, true);

    app.process_action(&move_down()).expect("reorder handling");
    // The order is read on a connection that drops before the write.
    tokio::time::sleep(Duration::from_millis(50)).await;
    controller.disconnect().await;

    let notification = outcome(&mut app).await;
    assert_eq!(notification.level, NotificationLevel::Error);
    assert_eq!(
        notification.message,
        "Could not move policy-2: the controller is not connected"
    );
    assert!(!app.policy_move.load(atomic::Ordering::Acquire));
    server.verify().await;
}

#[tokio::test]
async fn a_rejected_reorder_says_so() {
    let server = MockServer::start().await;
    let controller = controller(&server).await;
    Mock::given(method("GET"))
        .and(path(ordering_route()))
        .respond_with(ResponseTemplate::new(200).set_body_json(ordering_body(&[1, 2, 3], &[4])))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(ordering_route()))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "statusCode": 400, "statusName": "BAD_REQUEST",
            "message": "ordering does not match the zone pair",
        })))
        .mount(&server)
        .await;

    let mut app = App::new(Some(controller), None, false, true);
    app.process_action(&move_to(2, (&[2, 1, 3], &[4])))
        .expect("reorder handling");

    let notification = outcome(&mut app).await;
    assert_eq!(notification.level, NotificationLevel::Error);
    assert!(
        notification.message.starts_with("Could not move policy-2"),
        "{}",
        notification.message
    );
}

#[tokio::test]
async fn several_steps_are_written_as_one_ordering() {
    let server = MockServer::start().await;
    let controller = controller(&server).await;
    // Policy 1 goes down three: past 2 and 3, then across the block.
    mount_ordering(&server, (&[2, 3], &[1, 4])).await;
    mount_refresh(&server).await;
    let mut app = App::new(Some(controller), None, false, true);

    app.process_action(&move_to(1, (&[2, 3], &[1, 4])))
        .expect("reorder handling");
    assert_eq!(
        app.notices.messages(),
        ["Moving policy-1 down 3 places\u{2026}"]
    );

    let notification = outcome(&mut app).await;
    assert_eq!(
        notification.message,
        "Moved policy-1 down 3 places, past the system-defined policies"
    );
    server.verify().await;
}

#[tokio::test]
async fn the_demo_fleet_moves_a_policy() {
    let demo = Controller::demo(unifly_api::DemoFleetOptions::default());
    demo.connect().await.expect("demo fleet connects");
    let zones = demo.firewall_zones_snapshot();
    let ordered = |demo: &Controller| {
        crate::cli::commands::util::order::firewall_policies(
            &demo.firewall_policies_snapshot(),
            &zones,
        )
    };
    let user = |policy: &FirewallPolicy| {
        policy.origin == Some(unifly_api::model::EntityOrigin::UserDefined)
    };
    let before = ordered(&demo);
    let second = before
        .windows(2)
        .find(|pair| {
            user(&pair[0])
                && user(&pair[1])
                && pair[0].source.zone_id == pair[1].source.zone_id
                && pair[0].destination.zone_id == pair[1].destination.zone_id
        })
        .map(|pair| Arc::clone(&pair[1]))
        .expect("two user policies next to each other in a zone pair");
    let place = |policies: &[Arc<FirewallPolicy>]| {
        policies.iter().position(|policy| policy.id == second.id)
    };
    let (source, destination) = (
        second.source.zone_id.clone().expect("source zone"),
        second
            .destination
            .zone_id
            .clone()
            .expect("destination zone"),
    );
    let started = pair_order(
        &demo
            .get_firewall_policy_ordering(&source, &destination)
            .await
            .expect("the fleet's ordering"),
    );
    let mut drafted = started.clone();
    let half = if drafted.before.contains(&second.id) {
        &mut drafted.before
    } else {
        &mut drafted.after
    };
    let at = half
        .iter()
        .position(|id| id == &second.id)
        .expect("in its half");
    half.swap(at, at - 1);
    let mut app = App::new(Some(demo.clone()), None, false, true);

    app.process_action(&Action::MovePolicy {
        policy: Arc::clone(&second),
        plan: Box::new(PolicyMove {
            started,
            drafted,
            split_shown: true,
        }),
    })
    .expect("reorder handling");

    let notification = outcome(&mut app).await;
    assert_eq!(notification.level, NotificationLevel::Success);
    assert_eq!(
        place(&ordered(&demo)),
        place(&before).map(|at| at - 1),
        "the policy is one place earlier"
    );
}

#[tokio::test]
async fn a_policy_without_a_zone_pair_is_refused_up_front() {
    let server = MockServer::start().await;
    let controller = controller(&server).await;
    let mut app = App::new(Some(controller), None, false, true);

    app.process_action(&Action::MovePolicy {
        policy: policy(2, None),
        plan: Box::new(shown(SERVED, TWO_DOWN)),
    })
    .expect("reorder handling");

    let notification = outcome(&mut app).await;
    assert_eq!(notification.level, NotificationLevel::Error);
    assert!(notification.message.contains("no zone pair"));
}
