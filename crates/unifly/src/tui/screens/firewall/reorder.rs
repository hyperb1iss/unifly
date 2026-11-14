//! A policy move as the table shows it before the controller has it.
//!
//! `K` and `J` pick a policy up and move it through its zone pair on
//! screen only. Nothing is written until `Enter`, so a stray shifted key
//! cannot reorder a firewall, and a run of presses becomes one write.

use std::sync::Arc;

use unifly_api::EntityId;
use unifly_api::model::{EntityOrigin, FirewallPolicy};

use crate::tui::action::{PairOrder, PolicyMove};

/// Ticks a landed move keeps its place on screen while the refreshed
/// order is on its way (about two seconds).
const LANDED_TICKS: u8 = 8;

/// Where a move is between the first key and the controller's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    /// Picked up: on screen only.
    Draft,
    /// Sent to the controller.
    Sent,
    /// Written; the refreshed order has not arrived yet.
    Landed { ticks: u8 },
}

/// One policy being moved.
#[derive(Debug, Clone)]
pub(super) struct Reorder {
    pub(super) policy: Arc<FirewallPolicy>,
    /// Places moved through the zone pair, up when negative.
    pub(super) steps: i32,
    pub(super) stage: Stage,
}

impl Reorder {
    /// Count a tick; returns whether a landed move has waited long enough
    /// for its refresh and should let go.
    pub(super) fn tick(&mut self) -> bool {
        match &mut self.stage {
            Stage::Landed { ticks } => {
                *ticks += 1;
                *ticks > LANDED_TICKS
            }
            Stage::Draft | Stage::Sent => false,
        }
    }
}

/// Whether the controller keeps a policy where it is. Only the policies
/// an admin made are in a zone pair's ordering: the built-in ones and the
/// ones derived from another policy are not.
pub(super) fn is_fixed(policy: &FirewallPolicy) -> bool {
    policy
        .origin
        .is_some_and(|origin| origin != EntityOrigin::UserDefined)
}

/// What a row that is not an admin's is, for the line that says why a key
/// does nothing on it.
pub(super) fn owner(origin: Option<EntityOrigin>) -> Option<&'static str> {
    match origin? {
        EntityOrigin::UserDefined => None,
        EntityOrigin::SystemDefined => Some("system-defined"),
        EntityOrigin::Derived => Some("derived from another rule"),
        EntityOrigin::Orchestrated => Some("managed by the controller"),
    }
}

fn is_system(policy: &FirewallPolicy) -> bool {
    policy.origin == Some(EntityOrigin::SystemDefined)
}

fn same_pair(a: &FirewallPolicy, b: &FirewallPolicy) -> bool {
    a.source.zone_id == b.source.zone_id && a.destination.zone_id == b.destination.zone_id
}

/// The rows of `policies` (in evaluation order) that hold the zone pair
/// of the policy at `at`.
pub(super) fn pair_range(policies: &[Arc<FirewallPolicy>], at: usize) -> std::ops::Range<usize> {
    let Some(policy) = policies.get(at) else {
        return at..at;
    };
    let before = policies[..at]
        .iter()
        .rev()
        .take_while(|other| same_pair(other, policy))
        .count();
    let after = policies[at..]
        .iter()
        .take_while(|other| same_pair(other, policy))
        .count();
    at - before..at + after
}

/// The rows a step from `at` lands past, going down when `down`: the
/// next user policy, or the whole run of fixed rows that holds the
/// system-defined policies. `None` at the end of the pair.
///
/// The controller's ordering is the user policies in two halves around
/// the system-defined ones, so those are the only two things a move can
/// pass. Derived rows are in neither half: a step carries on over them,
/// and where they sit afterwards is the controller's choice.
fn next_stop(
    rows: &[Arc<FirewallPolicy>],
    at: usize,
    down: bool,
) -> Option<std::ops::Range<usize>> {
    let ahead: Box<dyn Iterator<Item = usize>> = if down {
        Box::new(at + 1..rows.len())
    } else {
        Box::new((0..at).rev())
    };
    let mut run: Option<std::ops::Range<usize>> = None;
    let mut run_has_system = false;
    for index in ahead {
        let fixed = is_fixed(&rows[index]);
        if !fixed && run_has_system {
            break;
        }
        if !fixed {
            return Some(index..index + 1);
        }
        run_has_system |= is_system(&rows[index]);
        run = Some(match run {
            Some(run) => run.start.min(index)..run.end.max(index + 1),
            None => index..index + 1,
        });
    }
    run.filter(|_| run_has_system)
}

/// `policies` with the policy `id` moved `steps` places through its zone
/// pair, and the places it could actually take.
///
/// A step takes the policy past its next user policy, or past all of the
/// pair's system-defined policies at once. The move stops at the pair's
/// ends.
pub(super) fn shifted(
    policies: &[Arc<FirewallPolicy>],
    id: &EntityId,
    steps: i32,
) -> (Vec<Arc<FirewallPolicy>>, i32) {
    let Some(at) = policies.iter().position(|policy| &policy.id == id) else {
        return (policies.to_vec(), 0);
    };
    let pair = pair_range(policies, at);
    let mut rows = policies[pair.clone()].to_vec();
    let mut at = at - pair.start;
    let down = steps > 0;
    let mut taken = 0;
    for _ in 0..steps.unsigned_abs() {
        let Some(stop) = next_stop(&rows, at, down) else {
            break;
        };
        let moving = rows.remove(at);
        // Past the stop: after its last row going down, before its first
        // going up. Taking the policy out shifted the rows below it.
        at = if down { stop.end - 1 } else { stop.start };
        rows.insert(at, moving);
        taken += if down { 1 } else { -1 };
    }

    let mut moved = policies[..pair.start].to_vec();
    moved.extend(rows);
    moved.extend_from_slice(&policies[pair.end..]);
    (moved, taken)
}

/// The user policies of the zone pair that holds row `at`, in the two
/// halves the screen shows them in, and whether it shows the
/// system-defined policies that split them. Without any on screen the
/// split is not known here, and every policy is in the first half.
fn halves(policies: &[Arc<FirewallPolicy>], at: usize) -> (PairOrder, bool) {
    let rows = &policies[pair_range(policies, at)];
    let split = rows.iter().position(|policy| is_system(policy));
    let ids = |rows: &[Arc<FirewallPolicy>]| {
        rows.iter()
            .filter(|policy| !is_fixed(policy))
            .map(|policy| policy.id.clone())
            .collect()
    };
    let (before, after) = rows.split_at(split.unwrap_or(rows.len()));
    let order = PairOrder {
        before: ids(before),
        after: ids(after),
    };
    (order, split.is_some())
}

/// The move of `id` by `steps` as the controller needs it: its zone
/// pair's order as the table shows it now, and as the draft leaves it.
pub(super) fn plan(
    policies: &[Arc<FirewallPolicy>],
    id: &EntityId,
    steps: i32,
) -> Option<PolicyMove> {
    let at = policies.iter().position(|policy| &policy.id == id)?;
    let (started, split_shown) = halves(policies, at);
    let (moved, _) = shifted(policies, id, steps);
    let at = moved.iter().position(|policy| &policy.id == id)?;
    let (drafted, _) = halves(&moved, at);
    Some(PolicyMove {
        started,
        drafted,
        split_shown,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use unifly_api::EntityId;
    use unifly_api::model::FirewallPolicy;

    use super::{plan, shifted};
    use crate::tui::action::PairOrder;

    fn policy(name: &str, pair: (&str, &str), origin: &str) -> Arc<FirewallPolicy> {
        Arc::new(
            serde_json::from_value(json!({
                "id": name, "name": name, "description": null, "enabled": true,
                "index": 1, "action": "Allow", "ip_version": "Both",
                "source": { "zone_id": pair.0, "filter": null },
                "destination": { "zone_id": pair.1, "filter": null },
                "source_summary": null, "destination_summary": null,
                "protocol_summary": null, "schedule": null, "ipsec_mode": null,
                "connection_states": [], "logging_enabled": false, "origin": origin
            }))
            .expect("policy fixture"),
        )
    }

    /// Two zone pairs: `a b [sys1 sys2] c` then `x y`.
    fn table() -> Vec<Arc<FirewallPolicy>> {
        let lan = ("lan", "wan");
        let iot = ("iot", "wan");
        vec![
            policy("a", lan, "UserDefined"),
            policy("b", lan, "UserDefined"),
            policy("sys1", lan, "SystemDefined"),
            policy("sys2", lan, "SystemDefined"),
            policy("c", lan, "UserDefined"),
            policy("x", iot, "UserDefined"),
            policy("y", iot, "UserDefined"),
        ]
    }

    /// One zone pair from `names`: `s…` rows are system-defined, `d…`
    /// rows derived, the rest an admin's.
    fn pair(names: &str) -> Vec<Arc<FirewallPolicy>> {
        names
            .split_whitespace()
            .map(|name| {
                let origin = match name.chars().next() {
                    Some('s') => "SystemDefined",
                    Some('d') => "Derived",
                    _ => "UserDefined",
                };
                policy(name, ("lan", "wan"), origin)
            })
            .collect()
    }

    fn id(name: &str) -> EntityId {
        EntityId::Legacy(name.into())
    }

    fn names(order: &[Arc<FirewallPolicy>]) -> String {
        let names: Vec<&str> = order.iter().map(|policy| policy.name.as_str()).collect();
        names.join(" ")
    }

    fn moved(name: &str, steps: i32) -> (String, i32) {
        let (order, taken) = shifted(&table(), &id(name), steps);
        (names(&order), taken)
    }

    fn order(before: &[&str], after: &[&str]) -> PairOrder {
        PairOrder {
            before: before.iter().copied().map(id).collect(),
            after: after.iter().copied().map(id).collect(),
        }
    }

    #[test]
    fn a_step_swaps_a_policy_with_its_neighbour() {
        assert_eq!(moved("b", -1), ("b a sys1 sys2 c x y".into(), -1));
        assert_eq!(moved("x", 1), ("a b sys1 sys2 c y x".into(), 1));
    }

    #[test]
    fn a_step_past_the_fixed_policies_crosses_all_of_them() {
        assert_eq!(moved("b", 1), ("a sys1 sys2 b c x y".into(), 1));
        assert_eq!(moved("c", -1), ("a b c sys1 sys2 x y".into(), -1));
        assert_eq!(moved("a", 3), ("b sys1 sys2 c a x y".into(), 3));
    }

    #[test]
    fn a_move_stops_at_the_ends_of_its_zone_pair() {
        assert_eq!(moved("a", -4), ("a b sys1 sys2 c x y".into(), 0));
        assert_eq!(moved("c", 9), ("a b sys1 sys2 c x y".into(), 0));
        assert_eq!(moved("x", 9), ("a b sys1 sys2 c y x".into(), 1));
        assert_eq!(moved("nowhere", 1), ("a b sys1 sys2 c x y".into(), 0));
    }

    /// A derived row is in neither half of the controller's ordering, so
    /// passing one alone would change nothing there: every step has to
    /// reach the next thing the ordering knows.
    #[test]
    fn a_step_carries_on_over_derived_rows_to_the_next_real_stop() {
        let rows = pair("d1 u1 d2 u2 s1 u3");
        let step = |name: &str, steps| {
            let (order, taken) = shifted(&rows, &id(name), steps);
            (names(&order), taken)
        };
        assert_eq!(step("u1", 1), ("d1 d2 u2 u1 s1 u3".into(), 1));
        assert_eq!(step("u2", -1), ("d1 u2 u1 d2 s1 u3".into(), -1));
        assert_eq!(
            step("u1", -1),
            ("d1 u1 d2 u2 s1 u3".into(), 0),
            "a derived row above is no place to move to"
        );

        // Derived rows beside the system-defined ones go by with them.
        let rows = pair("u1 d1 s1 d2 u2 d3");
        let (order, taken) = shifted(&rows, &id("u1"), 1);
        assert_eq!((names(&order), taken), ("d1 s1 d2 u1 u2 d3".into(), 1));
        let (order, taken) = shifted(&rows, &id("u2"), 1);
        assert_eq!((names(&order), taken), (names(&rows), 0));
    }

    #[test]
    fn a_plan_names_the_order_shown_and_the_order_drafted() {
        let swap = plan(&table(), &id("b"), -1).expect("b is listed");
        assert_eq!(swap.started, order(&["a", "b"], &["c"]));
        assert_eq!(swap.drafted, order(&["b", "a"], &["c"]));
        assert!(swap.split_shown);

        let cross = plan(&table(), &id("b"), 1).expect("b is listed");
        assert_eq!(cross.started, order(&["a", "b"], &["c"]));
        assert_eq!(cross.drafted, order(&["a"], &["b", "c"]));

        // The other zone pair shows no system-defined policy: where its
        // halves split is the controller's to say.
        let flat = plan(&table(), &id("x"), 1).expect("x is listed");
        assert_eq!(flat.started, order(&["x", "y"], &[]));
        assert_eq!(flat.drafted, order(&["y", "x"], &[]));
        assert!(!flat.split_shown);
    }

    #[test]
    fn a_plan_leaves_derived_rows_out_of_the_order() {
        let rows = pair("d1 u1 d2 u2 s1 u3");
        let down = plan(&rows, &id("u1"), 1).expect("u1 is listed");
        assert_eq!(down.started, order(&["u1", "u2"], &["u3"]));
        assert_eq!(down.drafted, order(&["u2", "u1"], &["u3"]));
    }
}
