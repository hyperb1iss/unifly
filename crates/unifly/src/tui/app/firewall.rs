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
use crate::tui::action::{Action, Notification, NotificationLevel, PairOrder, PolicyMove};

fn pair_order(ordering: &FirewallPolicyOrdering) -> PairOrder {
    let ids = |ids: &[_]| ids.iter().map(|id| EntityId::Uuid(*id)).collect();
    PairOrder {
        before: ids(&ordering.before_system_defined),
        after: ids(&ordering.after_system_defined),
    }
}

fn flat(order: &PairOrder) -> impl Iterator<Item = &EntityId> {
    order.before.iter().chain(&order.after)
}

/// Whether two orders hold the same policies, in any order.
fn same_members(a: &PairOrder, b: &PairOrder) -> bool {
    let sorted = |order: &PairOrder| {
        let mut ids: Vec<String> = flat(order).map(ToString::to_string).collect();
        ids.sort_unstable();
        ids
    };
    sorted(a) == sorted(b)
}

/// What a drafted move comes to on the controller's own ordering.
#[derive(Debug, PartialEq, Eq)]
enum Planned {
    /// The ordering to write.
    Write(PairOrder),
    /// The controller already has the drafted order.
    Unchanged,
    /// The controller's order is not the one the draft was made on.
    Stale,
    /// The controller keeps the policy in a fixed place: it is in neither
    /// half of the ordering, as with system-defined policies.
    Fixed,
}

/// Turn the move the table showed into the ordering to write, given the
/// ordering the controller has now.
///
/// The user moved a policy through the order on screen, so that order is
/// what has to be true of the controller before the drafted one is
/// written. Replaying key presses on whatever the controller holds would
/// put the policy somewhere the user never saw.
fn planned(fetched: &PairOrder, id: &EntityId, plan: &PolicyMove) -> Planned {
    if !flat(fetched).any(|other| other == id) {
        return Planned::Fixed;
    }
    if !same_members(&plan.started, &plan.drafted) {
        return Planned::Stale;
    }
    let target = if plan.split_shown {
        if *fetched != plan.started {
            return Planned::Stale;
        }
        plan.drafted.clone()
    } else {
        if !flat(fetched).eq(flat(&plan.started)) {
            return Planned::Stale;
        }
        let drafted: Vec<EntityId> = flat(&plan.drafted).cloned().collect();
        match resplit(fetched, id, &drafted) {
            Some(order) => order,
            None => return Planned::Stale,
        }
    };
    if target == *fetched {
        Planned::Unchanged
    } else {
        Planned::Write(target)
    }
}

/// Split `drafted`, a zone pair's whole order after `id` moved, into the
/// controller's two halves, for a pair whose system-defined policies the
/// table did not show.
///
/// Every other policy stays in the half it is in. The moved one joins
/// the half of the policies it landed between, and keeps its own where
/// it landed on the boundary: with nothing evaluated between the halves
/// that is the same order either way, and the smaller change.
fn resplit(fetched: &PairOrder, id: &EntityId, drafted: &[EntityId]) -> Option<PairOrder> {
    let in_before = |other: &EntityId| fetched.before.contains(other);
    let at = drafted.iter().position(|other| other == id)?;
    let above = drafted[..at].last().map(in_before);
    let below = drafted.get(at + 1).map(in_before);
    let to_before = match (above != Some(false), below != Some(true)) {
        (true, false) => true,
        (false, true) => false,
        (true, true) => in_before(id),
        (false, false) => return None,
    };
    let mut order = PairOrder::default();
    for other in drafted {
        let before = if other == id {
            to_before
        } else {
            in_before(other)
        };
        if before {
            order.before.push(other.clone());
        } else {
            order.after.push(other.clone());
        }
    }
    Some(order)
}

/// How far a move takes a policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Moved {
    up: bool,
    /// Places moved; the system-defined policies count as one.
    places: usize,
    /// Whether it crossed the system-defined policies.
    crossed: bool,
}

/// The move of `id` that turns `from` into `to`.
fn describe(from: &PairOrder, to: &PairOrder, id: &EntityId) -> Moved {
    let place = |order: &PairOrder| flat(order).position(|other| other == id).unwrap_or(0);
    let (was, now) = (place(from), place(to));
    let crossed = from.before.contains(id) != to.before.contains(id);
    Moved {
        up: now < was || (now == was && to.before.contains(id)),
        places: now.abs_diff(was) + usize::from(crossed),
        crossed,
    }
}

/// Where a move failed. Reading the order changes nothing, but a write
/// that failed or timed out may still have landed.
enum MoveFailure {
    Read(CoreError),
    Write(CoreError),
}

/// How a move that did not fail ended.
enum Outcome {
    Moved,
    Unchanged,
    Stale,
    Fixed,
}

/// Read the zone pair's ordering from the controller and, where it is
/// still the one the draft was made on, write the drafted order once.
async fn move_policy(
    controller: &Controller,
    zone_pair: (EntityId, EntityId),
    id: &EntityId,
    plan: &PolicyMove,
) -> Result<Outcome, MoveFailure> {
    let fetched = controller
        .get_firewall_policy_ordering(&zone_pair.0, &zone_pair.1)
        .await
        .map_err(MoveFailure::Read)?;
    let fetched = pair_order(&fetched);
    let order = match planned(&fetched, id, plan) {
        Planned::Write(order) => order,
        Planned::Unchanged => return Ok(Outcome::Unchanged),
        Planned::Stale => return Ok(Outcome::Stale),
        Planned::Fixed => return Ok(Outcome::Fixed),
    };
    controller
        .execute(Command::ReorderFirewallPolicies {
            zone_pair,
            before_system_ids: order.before,
            after_system_ids: order.after,
        })
        .await
        .map_err(MoveFailure::Write)?;
    Ok(Outcome::Moved)
}

/// What a move that changed the order says.
fn moved_message(name: &str, moved: Moved) -> String {
    let (label, side) = if moved.up {
        ("up", "above")
    } else {
        ("down", "below")
    };
    match (moved.places, moved.crossed) {
        (1, true) => format!("Moved {name} {side} the system-defined policies"),
        (1, false) => format!("Moved {name} {label}"),
        (places, true) => {
            format!("Moved {name} {label} {places} places, past the system-defined policies")
        }
        (places, false) => format!("Moved {name} {label} {places} places"),
    }
}

/// Run one move and say how it went, and whether the order changed.
/// `moved` is the move as the table showed it, which is how it is told.
async fn run_move(
    controller: Controller,
    zone_pair: (EntityId, EntityId),
    id: EntityId,
    name: String,
    plan: PolicyMove,
) -> (Notification, bool) {
    let moved = describe(&plan.started, &plan.drafted, &id);
    match move_policy(&controller, zone_pair, &id, &plan).await {
        Ok(Outcome::Moved) => {
            let message = moved_message(&name, moved);
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
        Ok(Outcome::Fixed) => (
            Notification::info(format!("{name} keeps a fixed place in its zone pair")),
            false,
        ),
        Ok(Outcome::Unchanged) => (
            Notification::info(format!("The controller already has {name} there")),
            false,
        ),
        // The table was behind the controller. Nothing was written; the
        // refresh puts the real order on screen to move through.
        Ok(Outcome::Stale) => {
            let refreshed = refresh_after_write(&controller).await;
            let next = if refreshed {
                "The list is up to date now"
            } else {
                "Refresh the list"
            };
            (
                Notification::warning(format!(
                    "The policy order on the controller is not the one shown, so {name} \
                     was not moved. {next}: move it again."
                )),
                false,
            )
        }
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
    /// Write a policy's drafted move to the controller. The Firewall
    /// screen has already shown the move; this makes it real, or says why
    /// not.
    ///
    /// The controller's ordering is read first and has to be the one the
    /// draft was made on. The drafted order is then written as one
    /// ordering, under a progress toast that stays until the move has
    /// landed and the data has been fetched again. The screen hears the
    /// end as [`Action::PolicyMoveSettled`].
    pub(super) fn move_firewall_policy(&mut self, policy: &Arc<FirewallPolicy>, plan: &PolicyMove) {
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
        let drafted = describe(&plan.started, &plan.drafted, &policy.id);
        let label = if drafted.up { "up" } else { "down" };
        let pending = if drafted.places <= 1 {
            format!("Moving {name} {label}\u{2026}")
        } else {
            format!("Moving {name} {label} {} places\u{2026}", drafted.places)
        };
        let progress = Some(self.begin_progress(&pending));
        let id = policy.id.clone();
        let plan = plan.clone();
        let tx = self.action_tx.clone();
        tokio::spawn(async move {
            let (notification, moved) = run_move(controller, zone_pair, id, name, plan).await;
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
pub(super) mod tests;
