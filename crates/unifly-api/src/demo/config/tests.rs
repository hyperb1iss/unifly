use chrono::{DateTime, TimeZone, Utc};

use crate::command::{
    Command, CommandResult, CreateNetworkRequest, UpdateAclRuleRequest, UpdateNatPolicyRequest,
    UpdateNetworkRequest,
};
use crate::core_error::CoreError;
use crate::demo::fleet::Fleet;
use crate::demo::sim::Simulation;
use crate::demo::{DEFAULT_DEMO_SEED, DemoFleetOptions};
use crate::model::{EntityId, EntityOrigin, Network};
use crate::{Controller, DhcpUpdate};

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 18, 15, 30, 0)
        .single()
        .expect("valid time")
}

fn sim() -> Simulation {
    Simulation::new(Fleet::generate(DEFAULT_DEMO_SEED, now()))
}

fn stage_network(vlan: u16) -> CreateNetworkRequest {
    CreateNetworkRequest {
        name: "Stage".into(),
        vlan_id: Some(vlan),
        subnet: Some("10.40.60.1/24".into()),
        management: None,
        device_id: None,
        purpose: None,
        dhcp_enabled: true,
        enabled: true,
        dhcp_range_start: Some("10.40.60.20".into()),
        dhcp_range_stop: Some("10.40.60.200".into()),
        dhcp_lease_time: None,
        dns_servers: None,
        firewall_zone_id: None,
        isolation_enabled: false,
        internet_access_enabled: true,
    }
}

fn create(sim: &mut Simulation, request: CreateNetworkRequest) -> Result<Network, CoreError> {
    match sim.apply_command(Command::CreateNetwork(request), now())? {
        (CommandResult::Network(network), delta) => {
            assert!(delta.config_changed);
            Ok(network)
        }
        (other, _) => panic!("a created network comes back, not {other:?}"),
    }
}

#[test]
fn a_created_network_serves_dhcp_and_can_be_deleted_again() {
    let mut sim = sim();
    let network = create(&mut sim, stage_network(60)).expect("create");
    assert_eq!(network.subnet.as_deref(), Some("10.40.60.0/24"));
    assert_eq!(
        network.gateway_ip.map(|ip| ip.to_string()).as_deref(),
        Some("10.40.60.1")
    );
    assert!(network.dhcp.as_ref().is_some_and(|dhcp| dhcp.enabled));

    let taken = create(&mut sim, stage_network(60)).expect_err("VLAN 60 is taken");
    assert!(
        taken.to_string().contains("already used by Stage"),
        "{taken}"
    );

    sim.apply_command(
        Command::DeleteNetwork {
            id: network.id.clone(),
            force: false,
        },
        now(),
    )
    .expect("a created network deletes");
    assert!(
        sim.fleet
            .networks
            .iter()
            .all(|other| other.id != network.id)
    );

    let built_in = sim.fleet.networks[1].id.clone();
    let refused = sim
        .apply_command(
            Command::DeleteNetwork {
                id: built_in,
                force: false,
            },
            now(),
        )
        .expect_err("the fleet's own networks stay");
    assert!(matches!(refused, CoreError::Rejected { .. }), "{refused}");
}

#[test]
fn a_rejected_network_update_changes_nothing() {
    let mut sim = sim();
    let studio = sim.fleet.networks[1].clone();
    let render_vlan = sim.fleet.networks[2].vlan_id;

    let error = sim
        .apply_command(
            Command::UpdateNetwork {
                id: studio.id.clone(),
                update: UpdateNetworkRequest {
                    name: Some("Soundstage".into()),
                    vlan_id: render_vlan,
                    ..UpdateNetworkRequest::default()
                },
            },
            now(),
        )
        .expect_err("a VLAN in use is refused");
    assert!(error.to_string().contains("already used"), "{error}");
    assert_eq!(sim.fleet.networks[1].name, studio.name);

    sim.apply_command(
        Command::UpdateNetwork {
            id: studio.id.clone(),
            update: UpdateNetworkRequest {
                name: Some("Soundstage".into()),
                subnet: Some("10.40.11.1/24".into()),
                dhcp: Some(DhcpUpdate {
                    range_start: Some("10.40.11.50".into()),
                    range_stop: Some("10.40.11.99".into()),
                    dns_servers: Some(vec!["9.9.9.9".into()]),
                }),
                ..UpdateNetworkRequest::default()
            },
        },
        now(),
    )
    .expect("a valid update lands");
    let updated = &sim.fleet.networks[1];
    assert_eq!(updated.name, "Soundstage");
    assert_eq!(updated.subnet.as_deref(), Some("10.40.11.0/24"));
    let dhcp = updated.dhcp.as_ref().expect("DHCP");
    assert_eq!(
        dhcp.range_stop.map(|ip| ip.to_string()).as_deref(),
        Some("10.40.11.99")
    );
    assert_eq!(dhcp.dns_servers.len(), 1);
}

#[test]
fn references_count_the_clients_and_ssids_on_a_network() {
    let sim = sim();
    let studio = &sim.fleet.networks[1];
    let references = sim.network_references(&studio.id);
    let resources = references["referenceResources"]
        .as_array()
        .expect("resources");
    assert!(
        resources
            .iter()
            .any(|resource| resource["resourceType"] == "CLIENT"
                && resource["referenceCount"].as_u64() > Some(0)),
        "{references}"
    );
    let unused = sim.network_references(&EntityId::Legacy("nowhere".into()));
    assert_eq!(unused["referenceResources"], serde_json::json!([]));
}

#[test]
fn acl_and_nat_rules_toggle_and_delete() {
    let mut sim = sim();
    let acl = sim.fleet.acls[0].clone();
    sim.apply_command(
        Command::UpdateAclRule {
            id: acl.id.clone(),
            update: UpdateAclRuleRequest {
                enabled: Some(!acl.enabled),
                ..UpdateAclRuleRequest::default()
            },
        },
        now(),
    )
    .expect("ACL toggle");
    assert_eq!(sim.fleet.acls[0].enabled, !acl.enabled);
    sim.apply_command(Command::DeleteAclRule { id: acl.id.clone() }, now())
        .expect("ACL delete");
    assert!(sim.fleet.acls.iter().all(|rule| rule.id != acl.id));

    let nat = sim
        .fleet
        .nat
        .iter()
        .find(|rule| rule.origin == Some(EntityOrigin::UserDefined))
        .cloned()
        .expect("a user NAT rule");
    let (_, delta) = sim
        .apply_command(
            Command::UpdateNatPolicy {
                id: nat.id.clone(),
                update: UpdateNatPolicyRequest {
                    enabled: Some(!nat.enabled),
                    ..UpdateNatPolicyRequest::default()
                },
            },
            now(),
        )
        .expect("NAT toggle");
    assert!(delta.config_changed);
    let toggled = sim.fleet.nat.iter().find(|rule| rule.id == nat.id);
    assert_eq!(toggled.map(|rule| rule.enabled), Some(!nat.enabled));

    let system = sim
        .fleet
        .nat
        .iter()
        .find(|rule| rule.origin == Some(EntityOrigin::SystemDefined))
        .map(|rule| rule.id.clone())
        .expect("the masquerade rule");
    sim.apply_command(Command::DeleteNatPolicy { id: system }, now())
        .expect_err("a system-defined NAT rule stays");
    sim.apply_command(Command::DeleteNatPolicy { id: nat.id.clone() }, now())
        .expect("NAT delete");
    assert!(sim.fleet.nat.iter().all(|rule| rule.id != nat.id));
}

/// A zone pair holding at least two user policies, with their IDs in
/// evaluation order.
fn busy_zone_pair(sim: &Simulation) -> ((EntityId, EntityId), Vec<EntityId>) {
    sim.fleet
        .policies
        .iter()
        .filter_map(|policy| {
            Some((
                policy.source.zone_id.clone()?,
                policy.destination.zone_id.clone()?,
            ))
        })
        .find_map(|pair| {
            let ordering = sim.policy_ordering(&pair.0, &pair.1);
            (ordering.before_system_defined.len() >= 2).then(|| {
                let ids = ordering
                    .before_system_defined
                    .iter()
                    .map(|id| EntityId::Uuid(*id))
                    .collect();
                (pair, ids)
            })
        })
        .expect("a zone pair with two user policies")
}

#[test]
fn a_reorder_swaps_evaluation_order_and_checks_the_zone_pair() {
    let mut sim = sim();
    let (pair, mut ids) = busy_zone_pair(&sim);
    let after: Vec<EntityId> = sim
        .policy_ordering(&pair.0, &pair.1)
        .after_system_defined
        .iter()
        .map(|id| EntityId::Uuid(*id))
        .collect();

    ids.swap(0, 1);
    sim.apply_command(
        Command::ReorderFirewallPolicies {
            zone_pair: pair.clone(),
            before_system_ids: ids.clone(),
            after_system_ids: after.clone(),
        },
        now(),
    )
    .expect("reorder");
    let reordered: Vec<EntityId> = sim
        .policy_ordering(&pair.0, &pair.1)
        .before_system_defined
        .iter()
        .map(|id| EntityId::Uuid(*id))
        .collect();
    assert_eq!(reordered, ids);

    let short = sim
        .apply_command(
            Command::ReorderFirewallPolicies {
                zone_pair: pair,
                before_system_ids: ids[1..].to_vec(),
                after_system_ids: after,
            },
            now(),
        )
        .expect_err("an ordering that drops a policy is refused");
    assert!(short.to_string().contains("zone pair"), "{short}");
}

#[test]
fn only_user_policies_can_be_deleted() {
    let mut sim = sim();
    let by_origin = |sim: &Simulation, origin| {
        sim.fleet
            .policies
            .iter()
            .find(|policy| policy.origin == Some(origin))
            .map(|policy| policy.id.clone())
            .expect("a policy of that origin")
    };
    let system = by_origin(&sim, EntityOrigin::SystemDefined);
    sim.apply_command(Command::DeleteFirewallPolicy { id: system }, now())
        .expect_err("system-defined policies stay");

    let user = by_origin(&sim, EntityOrigin::UserDefined);
    sim.apply_command(Command::DeleteFirewallPolicy { id: user.clone() }, now())
        .expect("a user policy deletes");
    assert!(sim.fleet.policies.iter().all(|policy| policy.id != user));
}

#[tokio::test]
async fn the_store_drops_what_a_command_deleted() {
    let controller = Controller::demo(DemoFleetOptions::default());
    controller.connect().await.expect("demo connect");
    let rule = controller.acl_rules_snapshot()[0].clone();
    let events_before = controller.events_snapshot().len();

    controller
        .execute(Command::DeleteAclRule {
            id: rule.id.clone(),
        })
        .await
        .expect("delete");

    assert!(
        controller
            .acl_rules_snapshot()
            .iter()
            .all(|other| other.id != rule.id),
        "the deleted rule left the store"
    );
    assert!(
        controller.events_snapshot().len() >= events_before,
        "a resync keeps the events the store already held"
    );
    controller.disconnect().await;
}
