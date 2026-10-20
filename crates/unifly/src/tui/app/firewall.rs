//! Moving a firewall policy through its zone pair's ordering on the
//! controller.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tracing::warn;

use unifly_api::integration_types::FirewallPolicyOrdering;
use unifly_api::model::FirewallPolicy;
use unifly_api::{Command, Controller, CoreError, EntityId};

use super::App;
use crate::tui::action::{Action, Direction, Notification, NotificationLevel};

/// Where one step through a zone pair's ordering leaves a policy.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    /// The pair's new ordering, both halves in full, and whether the
    /// step crossed the system-defined block between them.
    Moved {
        before_system: Vec<EntityId>,
        after_system: Vec<EntityId>,
        crossed: bool,
    },
    /// The policy is already first or last in its zone pair.
    AtEdge,
    /// The controller keeps the policy in a fixed place: it is in neither
    /// half of the ordering, as with system-defined policies.
    Fixed,
}

/// Move `id` one step `direction` through `ordering`.
///
/// The controller orders a zone pair's user policies in two halves around
/// its system-defined policies, which sit between them as one block that
/// never moves. A step past the block crosses all of it, so `J` and `K`
/// can still bring a policy to any place it can hold.
fn step(ordering: &FirewallPolicyOrdering, id: &EntityId, direction: Direction) -> Step {
    let half = |ids: &[_]| {
        ids.iter()
            .map(|id| Some(EntityId::Uuid(*id)))
            .collect::<Vec<_>>()
    };
    let mut slots: Vec<Option<EntityId>> = half(&ordering.before_system_defined);
    slots.push(None);
    slots.extend(half(&ordering.after_system_defined));

    let Some(index) = slots.iter().position(|slot| slot.as_ref() == Some(id)) else {
        return Step::Fixed;
    };
    let target = match direction {
        Direction::Up => index.checked_sub(1),
        Direction::Down => Some(index + 1).filter(|next| *next < slots.len()),
    };
    let Some(target) = target else {
        return Step::AtEdge;
    };
    let crossed = slots.get(target).is_some_and(Option::is_none);
    slots.swap(index, target);

    let mut halves = slots
        .split(Option::is_none)
        .map(|half| half.iter().flatten().cloned().collect::<Vec<_>>());
    Step::Moved {
        before_system: halves.next().unwrap_or_default(),
        after_system: halves.next().unwrap_or_default(),
        crossed,
    }
}

/// Where a move failed. Reading the order changes nothing, but a write
/// that failed or timed out may still have landed.
enum MoveFailure {
    Read(CoreError),
    Write(CoreError),
}

/// Read the zone pair's ordering from the controller, move the policy one
/// step, and write the new ordering back.
async fn move_policy(
    controller: &Controller,
    zone_pair: (EntityId, EntityId),
    id: &EntityId,
    direction: Direction,
) -> Result<Step, MoveFailure> {
    let ordering = controller
        .get_firewall_policy_ordering(&zone_pair.0, &zone_pair.1)
        .await
        .map_err(MoveFailure::Read)?;
    let step = step(&ordering, id, direction);
    if let Step::Moved {
        before_system,
        after_system,
        ..
    } = &step
    {
        controller
            .execute(Command::ReorderFirewallPolicies {
                zone_pair,
                before_system_ids: before_system.clone(),
                after_system_ids: after_system.clone(),
            })
            .await
            .map_err(MoveFailure::Write)?;
    }
    Ok(step)
}

/// Fetch the controller's data now rather than at the next periodic
/// refresh, so the table shows the order the controller has.
async fn refresh(controller: &Controller) -> bool {
    match controller.full_refresh().await {
        Ok(()) => true,
        Err(error) => {
            warn!(%error, "refresh after a policy move failed");
            false
        }
    }
}

/// Run one move and say how it went.
async fn run_move(
    controller: Controller,
    zone_pair: (EntityId, EntityId),
    id: EntityId,
    name: String,
    direction: Direction,
) -> Notification {
    let (label, edge, side) = match direction {
        Direction::Up => ("up", "first", "above"),
        Direction::Down => ("down", "last", "below"),
    };
    match move_policy(&controller, zone_pair, &id, direction).await {
        Ok(Step::Moved { crossed, .. }) => {
            let moved = if crossed {
                format!("Moved {name} {side} the system-defined policies")
            } else {
                format!("Moved {name} {label}")
            };
            if refresh(&controller).await {
                Notification::success(moved)
            } else {
                Notification {
                    message: format!("{moved}; the list shows it after the next refresh"),
                    level: NotificationLevel::Warning,
                }
            }
        }
        Ok(Step::AtEdge) => {
            Notification::info(format!("{name} is already {edge} in its zone pair"))
        }
        Ok(Step::Fixed) => {
            Notification::info(format!("{name} keeps a fixed place in its zone pair"))
        }
        Err(MoveFailure::Read(error)) => {
            warn!(%error, "policy move failed");
            Notification::error(format!("Could not move {name}: {error}"))
        }
        Err(MoveFailure::Write(error)) => {
            warn!(%error, "policy move failed");
            refresh(&controller).await;
            Notification::error(format!("Could not move {name}: {error}"))
        }
    }
}

/// Marks a policy move in flight until dropped, so even a move that
/// panics frees `J` and `K` again.
struct MoveInFlight(Arc<AtomicBool>);

impl Drop for MoveInFlight {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl App {
    /// Move `policy` one step through its zone pair's ordering on the
    /// controller. The Policies table only ever shows the controller's
    /// order: a move that lands refreshes the data, and one that fails
    /// says why and refreshes too if its write may have landed.
    ///
    /// Each step is computed from the controller's current order, so the
    /// next one cannot be right until this one lands. Presses while a move
    /// is in flight are dropped with a note, not queued.
    pub(super) fn reorder_policy(&self, policy: &Arc<FirewallPolicy>, direction: Direction) {
        let notify = |notification| {
            let _ = self.action_tx.send(Action::Notify(notification));
        };
        let Some(controller) = self.controller.clone() else {
            notify(Notification::error("Not connected"));
            return;
        };
        let name = policy.name.clone();
        let (Some(source), Some(destination)) = (
            policy.source.zone_id.clone(),
            policy.destination.zone_id.clone(),
        ) else {
            notify(Notification::error(format!(
                "{name} has no zone pair, so it has no order to change"
            )));
            return;
        };
        if self.policy_move.swap(true, Ordering::AcqRel) {
            notify(Notification::info("A policy move is still in progress"));
            return;
        }

        let in_flight = MoveInFlight(Arc::clone(&self.policy_move));
        let label = match direction {
            Direction::Up => "up",
            Direction::Down => "down",
        };
        notify(Notification::info(format!("Moving {name} {label}\u{2026}")));
        let id = policy.id.clone();
        let tx = self.action_tx.clone();
        tokio::spawn(async move {
            let notification =
                run_move(controller, (source, destination), id, name, direction).await;
            // Free the keys before the toast, so a press that answers it
            // is taken.
            drop(in_flight);
            let _ = tx.send(Action::Notify(notification));
        });
    }
}

#[cfg(test)]
mod tests {
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

    /// A zone pair's ordering: user policies 1-3 before the system block,
    /// 4-5 after it.
    fn ordering() -> FirewallPolicyOrdering {
        FirewallPolicyOrdering {
            before_system_defined: vec![uuid(1), uuid(2), uuid(3)],
            after_system_defined: vec![uuid(4), uuid(5)],
        }
    }

    fn moved(before: &[u128], after: &[u128], crossed: bool) -> Step {
        Step::Moved {
            before_system: before.iter().copied().map(id).collect(),
            after_system: after.iter().copied().map(id).collect(),
            crossed,
        }
    }

    #[test]
    fn a_step_swaps_a_policy_with_its_neighbour() {
        assert_eq!(
            step(&ordering(), &id(2), Direction::Up),
            moved(&[2, 1, 3], &[4, 5], false)
        );
        assert_eq!(
            step(&ordering(), &id(4), Direction::Down),
            moved(&[1, 2, 3], &[5, 4], false)
        );
    }

    #[test]
    fn a_step_past_the_system_block_crosses_all_of_it() {
        assert_eq!(
            step(&ordering(), &id(3), Direction::Down),
            moved(&[1, 2], &[3, 4, 5], true)
        );
        assert_eq!(
            step(&ordering(), &id(4), Direction::Up),
            moved(&[1, 2, 3, 4], &[5], true)
        );
    }

    #[test]
    fn the_ends_and_fixed_policies_stay_put() {
        assert_eq!(step(&ordering(), &id(1), Direction::Up), Step::AtEdge);
        assert_eq!(step(&ordering(), &id(5), Direction::Down), Step::AtEdge);
        assert_eq!(step(&ordering(), &id(9), Direction::Up), Step::Fixed);
    }

    const SITE: &str = "550e8400-e29b-41d4-a716-446655440000";
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

    /// An API-key controller on `server`, connected without loading data.
    async fn controller(server: &MockServer) -> Controller {
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
            refresh_interval_secs: 0,
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

    /// Empty pages for the collections a refresh cannot do without, so a
    /// refresh after a move succeeds.
    async fn mount_refresh(server: &MockServer) {
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
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "offset": 0, "limit": 200, "count": 0, "totalCount": 0, "data": [],
                })))
                .mount(server)
                .await;
        }
    }

    /// Serve the zone pair's ordering as policies 1-3 before the system
    /// block and 4 after it, and accept exactly one write of `moved`, the
    /// two halves after the move.
    async fn mount_ordering(server: &MockServer, moved: (&[u128], &[u128])) {
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
            .expect(1)
            .mount(server)
            .await;
    }

    /// Policy 2 one step down, from 1-3 before the system block.
    const TWO_DOWN: (&[u128], &[u128]) = (&[1, 3, 2], &[4]);

    fn move_down() -> Action {
        Action::ReorderPolicy(
            policy(2, Some((SOURCE_ZONE, DESTINATION_ZONE))),
            Direction::Down,
        )
    }

    fn ordering_route() -> String {
        format!("/proxy/network/integration/v1/sites/{SITE}/firewall/policies/ordering")
    }

    fn ordering_body(before: &[u128], after: &[u128]) -> serde_json::Value {
        let ids =
            |ids: &[u128]| -> Vec<String> { ids.iter().map(|n| uuid(*n).to_string()).collect() };
        json!({ "orderedFirewallPolicyIds": {
            "beforeSystemDefined": ids(before),
            "afterSystemDefined": ids(after),
        }})
    }

    /// The toast a finished move leaves, past the one it showed on
    /// starting.
    async fn outcome(app: &mut App) -> Notification {
        loop {
            let notification = next_notification(app).await;
            if !notification.message.starts_with("Moving ") {
                return notification;
            }
        }
    }

    async fn next_notification(app: &mut App) -> Notification {
        loop {
            let action = tokio::time::timeout(Duration::from_secs(10), app.action_rx.recv())
                .await
                .expect("the reorder reports back")
                .expect("the action channel stays open");
            if let Action::Notify(notification) = action {
                return notification;
            }
        }
    }

    #[tokio::test]
    async fn a_reorder_writes_the_moved_order_to_the_controller() {
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

        let started = next_notification(&mut app).await;
        assert_eq!(started.message, "Moving policy-2 down\u{2026}");
        let held = next_notification(&mut app).await;
        assert_eq!(held.message, "A policy move is still in progress");
        let landed = next_notification(&mut app).await;
        assert_eq!(landed.message, "Moved policy-2 down");
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

        app.process_action(&Action::ReorderPolicy(
            policy(3, Some((SOURCE_ZONE, DESTINATION_ZONE))),
            Direction::Down,
        ))
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
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "offset": 0, "limit": 200, "count": 0, "totalCount": 0, "data": [],
            })))
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
        app.process_action(&Action::ReorderPolicy(
            policy(2, Some((SOURCE_ZONE, DESTINATION_ZONE))),
            Direction::Up,
        ))
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
    async fn the_demo_fleet_refuses_a_move() {
        let demo = Controller::demo(unifly_api::DemoFleetOptions::default());
        demo.connect().await.expect("demo fleet connects");
        let mut app = App::new(Some(demo), None, false, true);

        app.process_action(&Action::ReorderPolicy(
            policy(2, Some((SOURCE_ZONE, DESTINATION_ZONE))),
            Direction::Up,
        ))
        .expect("reorder handling");

        let notification = outcome(&mut app).await;
        assert_eq!(notification.level, NotificationLevel::Error);
        assert!(
            notification.message.contains("Integration API"),
            "{}",
            notification.message
        );
    }

    #[tokio::test]
    async fn a_policy_without_a_zone_pair_is_refused_up_front() {
        let server = MockServer::start().await;
        let controller = controller(&server).await;
        let mut app = App::new(Some(controller), None, false, true);

        app.process_action(&Action::ReorderPolicy(policy(2, None), Direction::Up))
            .expect("reorder handling");

        let notification = outcome(&mut app).await;
        assert_eq!(notification.level, NotificationLevel::Error);
        assert!(notification.message.contains("no zone pair"));
    }
}
