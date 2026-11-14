//! Moving a firewall policy through its zone pair's ordering on the
//! controller.

use std::sync::Arc;
use std::sync::atomic::{self, AtomicBool};

use tracing::warn;

use unifly_api::integration_types::FirewallPolicyOrdering;
use unifly_api::model::FirewallPolicy;
use unifly_api::{Command, Controller, CoreError, EntityId};

use super::App;
use super::networks_firewall::{is_connected, refresh_after_write};
use crate::tui::action::{Action, Direction, Notification, NotificationLevel};

/// A zone pair's user policies in evaluation order: those before its
/// system-defined policies, and those after.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Ordering {
    before_system: Vec<EntityId>,
    after_system: Vec<EntityId>,
}

impl From<&FirewallPolicyOrdering> for Ordering {
    fn from(ordering: &FirewallPolicyOrdering) -> Self {
        let ids = |ids: &[_]| ids.iter().map(|id| EntityId::Uuid(*id)).collect();
        Self {
            before_system: ids(&ordering.before_system_defined),
            after_system: ids(&ordering.after_system_defined),
        }
    }
}

/// Where one step through a zone pair's ordering leaves a policy.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    /// The pair's ordering after the step, and whether the step crossed
    /// the system-defined block between its halves.
    Moved { ordering: Ordering, crossed: bool },
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
fn step(ordering: &Ordering, id: &EntityId, direction: Direction) -> Step {
    let half = |ids: &[EntityId]| ids.iter().cloned().map(Some).collect::<Vec<_>>();
    let mut slots: Vec<Option<EntityId>> = half(&ordering.before_system);
    slots.push(None);
    slots.extend(half(&ordering.after_system));

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
        ordering: Ordering {
            before_system: halves.next().unwrap_or_default(),
            after_system: halves.next().unwrap_or_default(),
        },
        crossed,
    }
}

/// How far a move got.
#[derive(Debug, PartialEq, Eq)]
struct Moved {
    /// Steps the policy took before it ran out of room.
    taken: u32,
    /// Whether any step crossed the system-defined block.
    crossed: bool,
    /// Why it took none, when it took none.
    stopped: Option<Step>,
}

/// Where a move failed. Reading the order changes nothing, but a write
/// that failed or timed out may still have landed.
enum MoveFailure {
    Read(CoreError),
    Write(CoreError),
}

/// Read the zone pair's ordering from the controller, move the policy up
/// to `steps` places through it, and write the result back once.
async fn move_policy(
    controller: &Controller,
    zone_pair: (EntityId, EntityId),
    id: &EntityId,
    direction: Direction,
    steps: u32,
) -> Result<Moved, MoveFailure> {
    let fetched = controller
        .get_firewall_policy_ordering(&zone_pair.0, &zone_pair.1)
        .await
        .map_err(MoveFailure::Read)?;
    let mut ordering = Ordering::from(&fetched);
    let mut moved = Moved {
        taken: 0,
        crossed: false,
        stopped: None,
    };
    for _ in 0..steps {
        match step(&ordering, id, direction) {
            Step::Moved {
                ordering: next,
                crossed,
            } => {
                ordering = next;
                moved.taken += 1;
                moved.crossed |= crossed;
            }
            stopped => {
                if moved.taken == 0 {
                    moved.stopped = Some(stopped);
                }
                break;
            }
        }
    }
    if moved.taken > 0 {
        controller
            .execute(Command::ReorderFirewallPolicies {
                zone_pair,
                before_system_ids: ordering.before_system,
                after_system_ids: ordering.after_system,
            })
            .await
            .map_err(MoveFailure::Write)?;
    }
    Ok(moved)
}

/// What a move that changed the order says.
fn moved_message(name: &str, direction: Direction, moved: &Moved) -> String {
    let (label, side) = match direction {
        Direction::Up => ("up", "above"),
        Direction::Down => ("down", "below"),
    };
    match (moved.taken, moved.crossed) {
        (1, true) => format!("Moved {name} {side} the system-defined policies"),
        (1, false) => format!("Moved {name} {label}"),
        (taken, true) => {
            format!("Moved {name} {label} {taken} places, past the system-defined policies")
        }
        (taken, false) => format!("Moved {name} {label} {taken} places"),
    }
}

/// Run one move and say how it went, and whether the order changed.
async fn run_move(
    controller: Controller,
    zone_pair: (EntityId, EntityId),
    id: EntityId,
    name: String,
    steps: i32,
) -> (Notification, bool) {
    let (direction, edge) = if steps < 0 {
        (Direction::Up, "first")
    } else {
        (Direction::Down, "last")
    };
    match move_policy(&controller, zone_pair, &id, direction, steps.unsigned_abs()).await {
        Ok(moved) if moved.taken > 0 => {
            let message = moved_message(&name, direction, &moved);
            let notification = if refresh_after_write(&controller).await {
                Notification::success(message)
            } else {
                Notification {
                    message: format!("{message}; the list shows it after the next refresh"),
                    level: NotificationLevel::Warning,
                }
            };
            (notification, true)
        }
        Ok(Moved {
            stopped: Some(Step::Fixed),
            ..
        }) => (
            Notification::info(format!("{name} keeps a fixed place in its zone pair")),
            false,
        ),
        Ok(_) => (
            Notification::info(format!("{name} is already {edge} in its zone pair")),
            false,
        ),
        // A connection lost mid-move is named as such: a refresh could
        // not run, and the next connect's refresh shows the real order.
        Err(MoveFailure::Read(error) | MoveFailure::Write(error)) if !is_connected(&controller) => {
            warn!(%error, "policy move failed with the connection down");
            (
                Notification::error(format!(
                    "Could not move {name}: the controller is not connected"
                )),
                false,
            )
        }
        Err(MoveFailure::Read(error)) => {
            warn!(%error, "policy move failed");
            (
                Notification::error(format!("Could not move {name}: {error}")),
                false,
            )
        }
        Err(MoveFailure::Write(error)) => {
            warn!(%error, "policy move failed");
            refresh_after_write(&controller).await;
            (
                Notification::error(format!("Could not move {name}: {error}")),
                false,
            )
        }
    }
}

/// Marks a policy move in flight until dropped, so even a move that
/// panics frees `J` and `K` again.
struct MoveInFlight(Arc<AtomicBool>);

impl Drop for MoveInFlight {
    fn drop(&mut self) {
        self.0.store(false, atomic::Ordering::Release);
    }
}

impl App {
    /// Move `policy` `steps` places through its zone pair's ordering on
    /// the controller, up when negative. The Firewall screen has already
    /// shown the move as a draft; this makes it real, or says why not.
    ///
    /// The steps are taken on the controller's current order and written
    /// back as one ordering, under a progress toast that stays until the
    /// move has landed and the data has been fetched again. The screen
    /// hears the end as [`Action::PolicyMoveSettled`].
    pub(super) fn move_firewall_policy(&mut self, policy: &Arc<FirewallPolicy>, steps: i32) {
        let name = self.shown_name(&policy.name);
        let refusal = match self.controller.clone() {
            None => Err("Not connected".to_owned()),
            Some(controller) => match (
                policy.source.zone_id.clone(),
                policy.destination.zone_id.clone(),
            ) {
                (Some(source), Some(destination)) if is_connected(&controller) => {
                    Ok((controller, (source, destination)))
                }
                (Some(_), Some(_)) => Err(format!("Not connected, so {name} stays where it is")),
                _ => Err(format!(
                    "{name} has no zone pair, so it has no order to change"
                )),
            },
        };
        let settle = |moved| Action::PolicyMoveSettled { moved };
        let (controller, zone_pair) = match refusal {
            Ok(ready) => ready,
            Err(reason) => {
                self.show_notification(Notification::error(reason));
                let _ = self.action_tx.send(settle(false));
                return;
            }
        };
        if self.policy_move.swap(true, atomic::Ordering::AcqRel) {
            self.show_notification(Notification::info("A policy move is still in progress"));
            // This move will not run, so its row goes back; the one in
            // flight reports for itself.
            let _ = self.action_tx.send(settle(false));
            return;
        }

        let in_flight = MoveInFlight(Arc::clone(&self.policy_move));
        let (label, places) = (if steps < 0 { "up" } else { "down" }, steps.unsigned_abs());
        let pending = if places == 1 {
            format!("Moving {name} {label}\u{2026}")
        } else {
            format!("Moving {name} {label} {places} places\u{2026}")
        };
        let progress = Some(self.begin_progress(&pending));
        let id = policy.id.clone();
        let tx = self.action_tx.clone();
        tokio::spawn(async move {
            let (notification, moved) = run_move(controller, zone_pair, id, name, steps).await;
            // Free the keys before the toast, so a press that answers it
            // is taken.
            drop(in_flight);
            let _ = tx.send(settle(moved));
            let _ = tx.send(Action::OpFinished {
                progress,
                notification,
            });
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
    fn ordering() -> Ordering {
        halves(&[1, 2, 3], &[4, 5])
    }

    fn halves(before: &[u128], after: &[u128]) -> Ordering {
        Ordering {
            before_system: before.iter().copied().map(id).collect(),
            after_system: after.iter().copied().map(id).collect(),
        }
    }

    fn moved(before: &[u128], after: &[u128], crossed: bool) -> Step {
        Step::Moved {
            ordering: halves(before, after),
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
        move_by(2, 1)
    }

    /// Move policy `n` of the test zone pair `steps` places.
    fn move_by(n: u128, steps: i32) -> Action {
        Action::MovePolicy {
            policy: policy(n, Some((SOURCE_ZONE, DESTINATION_ZONE))),
            steps,
        }
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

    /// How a move ended: the notification that takes over its progress
    /// toast, or the refusal shown in its place. Every action on the way
    /// is processed, as the app's loop would.
    async fn outcome(app: &mut App) -> Notification {
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

        app.process_action(&move_by(3, 1))
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
        app.process_action(&move_by(2, -1))
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

        app.process_action(&move_by(1, 3))
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
    async fn a_move_past_the_end_stops_there_and_one_with_no_room_writes_nothing() {
        let server = MockServer::start().await;
        let controller = controller(&server).await;
        // Asked for nine places up, policy 3 has two to take.
        mount_ordering(&server, (&[3, 1, 2], &[4])).await;
        mount_refresh(&server).await;
        let mut app = App::new(Some(controller), None, false, true);

        app.process_action(&move_by(3, -9))
            .expect("reorder handling");
        let notification = outcome(&mut app).await;
        assert_eq!(notification.message, "Moved policy-3 up 2 places");

        app.notices.dismiss_sticky();
        app.process_action(&move_by(1, -1))
            .expect("reorder handling");
        let notification = loop {
            let notification = outcome(&mut app).await;
            if !notification.message.starts_with("Moved policy-3") {
                break notification;
            }
        };
        assert_eq!(
            notification.message,
            "policy-1 is already first in its zone pair"
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
        let mut app = App::new(Some(demo.clone()), None, false, true);

        app.process_action(&Action::MovePolicy {
            policy: Arc::clone(&second),
            steps: -1,
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
            steps: -1,
        })
        .expect("reorder handling");

        let notification = outcome(&mut app).await;
        assert_eq!(notification.level, NotificationLevel::Error);
        assert!(notification.message.contains("no zone pair"));
    }
}
