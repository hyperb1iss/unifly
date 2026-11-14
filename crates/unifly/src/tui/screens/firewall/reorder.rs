//! A policy move as the table shows it before the controller has it.
//!
//! `K` and `J` pick a policy up and move it through its zone pair on
//! screen only. Nothing is written until `Enter`, so a stray shifted key
//! cannot reorder a firewall, and a run of presses becomes one write.

use std::sync::Arc;

use unifly_api::EntityId;
use unifly_api::model::{EntityOrigin, FirewallPolicy};

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
/// an admin made are in a zone pair's ordering.
pub(super) fn is_fixed(policy: &FirewallPolicy) -> bool {
    policy
        .origin
        .is_some_and(|origin| origin != EntityOrigin::UserDefined)
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

/// `policies` with the policy `id` moved `steps` places through its zone
/// pair, and the places it could actually take.
///
/// A step swaps the policy with its neighbour, as the controller's
/// ordering does. A run of fixed policies counts as one neighbour: a
/// step past it crosses all of it. The move stops at the pair's ends.
pub(super) fn shifted(
    policies: &[Arc<FirewallPolicy>],
    id: &EntityId,
    steps: i32,
) -> (Vec<Arc<FirewallPolicy>>, i32) {
    let Some(at) = policies.iter().position(|policy| &policy.id == id) else {
        return (policies.to_vec(), 0);
    };
    let pair = pair_range(policies, at);

    let mut slots: Vec<Vec<Arc<FirewallPolicy>>> = Vec::new();
    let mut picked = 0;
    for policy in &policies[pair.clone()] {
        let joins_block = is_fixed(policy)
            && slots
                .last()
                .and_then(|slot| slot.first())
                .is_some_and(|previous| is_fixed(previous));
        match slots.last_mut() {
            Some(slot) if joins_block => slot.push(Arc::clone(policy)),
            _ => slots.push(vec![Arc::clone(policy)]),
        }
        if &policy.id == id {
            picked = slots.len() - 1;
        }
    }

    let last = i64::try_from(slots.len().saturating_sub(1)).unwrap_or(i64::MAX);
    let from = i64::try_from(picked).unwrap_or(0);
    let to = (from + i64::from(steps)).clamp(0, last);
    let slot = slots.remove(picked);
    slots.insert(usize::try_from(to).unwrap_or(picked), slot);

    let mut moved = policies[..pair.start].to_vec();
    moved.extend(slots.into_iter().flatten());
    moved.extend_from_slice(&policies[pair.end..]);
    (moved, i32::try_from(to - from).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use unifly_api::EntityId;
    use unifly_api::model::FirewallPolicy;

    use super::shifted;

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

    fn moved(id: &str, steps: i32) -> (String, i32) {
        let (order, taken) = shifted(&table(), &EntityId::Legacy(id.into()), steps);
        let names: Vec<&str> = order.iter().map(|policy| policy.name.as_str()).collect();
        (names.join(" "), taken)
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
}
