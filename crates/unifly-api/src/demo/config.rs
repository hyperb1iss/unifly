// ── Demo configuration writes ──
//
// The edits a network or firewall screen makes: network create, update,
// and delete, policy delete and reorder, and ACL and NAT toggles and
// deletes. Each one changes the fleet in memory and marks the delta so the
// store resyncs from it, which is how a removal reaches the store.

use std::net::{IpAddr, Ipv4Addr};

use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use super::catalog::NETWORKS;
use super::commands::ADMIN_INFO;
use super::fleet::uuid_from;
use super::sim::{Delta, Simulation};
use crate::command::{
    CreateNetworkRequest, DhcpUpdate, UpdateAclRuleRequest, UpdateNatPolicyRequest,
    UpdateNetworkRequest,
};
use crate::core_error::CoreError;
use crate::integration_types::FirewallPolicyOrdering;
use crate::model::common::DataSource;
use crate::model::{
    DhcpConfig, EntityId, EntityOrigin, FirewallPolicy, Ipv6Mode, Network, NetworkManagement,
    NetworkPurpose,
};
use crate::store::RefreshSnapshot;

/// Lease a DHCP server gets when a request does not name one.
const DEFAULT_LEASE_SECS: u64 = 86_400;

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::ValidationFailed {
        message: message.into(),
    }
}

fn rejected(message: impl Into<String>) -> CoreError {
    CoreError::Rejected {
        message: message.into(),
    }
}

fn not_found(entity_type: &str, id: &EntityId) -> CoreError {
    CoreError::NotFound {
        entity_type: entity_type.into(),
        identifier: id.to_string(),
    }
}

/// A gateway address with its prefix (`10.40.60.1/24`) as the gateway and
/// the subnet it sits in (`10.40.60.0/24`).
fn parse_gateway(cidr: &str) -> Result<(Ipv4Addr, String), CoreError> {
    let parsed = cidr.split_once('/').and_then(|(host, prefix)| {
        let host = host.parse::<Ipv4Addr>().ok()?;
        let prefix = prefix.parse::<u32>().ok().filter(|prefix| *prefix <= 32)?;
        Some((host, prefix))
    });
    let Some((host, prefix)) = parsed else {
        return Err(invalid(format!("invalid ipv4 host/prefix value '{cidr}'")));
    };
    let mask = u32::MAX.checked_shl(32 - prefix).unwrap_or(0);
    let base = Ipv4Addr::from(u32::from(host) & mask);
    Ok((host, format!("{base}/{prefix}")))
}

fn parse_address(label: &str, value: &str) -> Result<Ipv4Addr, CoreError> {
    value
        .parse()
        .map_err(|_| invalid(format!("DHCP range {label} must be an IPv4 address")))
}

fn parse_dns(servers: &[String]) -> Result<Vec<IpAddr>, CoreError> {
    servers
        .iter()
        .map(|server| {
            server
                .parse()
                .map_err(|_| invalid(format!("'{server}' is not a DNS server address")))
        })
        .collect()
}

/// Apply a DHCP server change to `network`, the way the controller
/// validates it: a server needs a managed network and a whole range.
fn apply_dhcp(
    network: &mut Network,
    enable: Option<bool>,
    update: Option<&DhcpUpdate>,
) -> Result<(), CoreError> {
    if enable.is_none() && update.is_none() {
        return Ok(());
    }
    let serving = network.dhcp.as_ref().is_some_and(|dhcp| dhcp.enabled);
    let enabled = enable.unwrap_or(serving);
    if !enabled {
        if update.is_some() {
            return Err(invalid(
                "editing DHCP server settings requires an enabled server; set dhcp_enabled to true",
            ));
        }
        if let Some(dhcp) = network.dhcp.as_mut() {
            dhcp.enabled = false;
        }
        return Ok(());
    }
    if network.management == Some(NetworkManagement::Unmanaged) || network.gateway_ip.is_none() {
        return Err(invalid(
            "IPv4 and DHCP server settings require a gateway- or switch-managed network",
        ));
    }

    let mut dhcp = network.dhcp.clone().unwrap_or(DhcpConfig {
        enabled: false,
        range_start: None,
        range_stop: None,
        lease_time_secs: Some(DEFAULT_LEASE_SECS),
        dns_servers: Vec::new(),
        gateway: network.gateway_ip,
    });
    if let Some(update) = update {
        if let Some(start) = update.range_start.as_deref() {
            dhcp.range_start = Some(parse_address("start", start)?);
        }
        if let Some(stop) = update.range_stop.as_deref() {
            dhcp.range_stop = Some(parse_address("stop", stop)?);
        }
        if let Some(servers) = update.dns_servers.as_deref() {
            dhcp.dns_servers = parse_dns(servers)?;
        }
    }
    let (Some(start), Some(stop)) = (dhcp.range_start, dhcp.range_stop) else {
        return Err(invalid(
            "enabling DHCP requires both range start and stop addresses",
        ));
    };
    if start > stop {
        return Err(invalid("DHCP range start must not be after its stop"));
    }
    dhcp.enabled = true;
    network.dhcp = Some(dhcp);
    Ok(())
}

impl Simulation {
    /// The configuration as a refresh would deliver it. Events are left
    /// alone: the store's list holds live ones the fleet never recorded.
    pub(crate) fn config_snapshot(&self) -> RefreshSnapshot {
        RefreshSnapshot {
            events: None,
            ..self.refresh_snapshot()
        }
    }

    fn admin_event(&mut self, now: DateTime<Utc>, key: &str, message: String, delta: &mut Delta) {
        let event = self.event(now, ADMIN_INFO, key, message);
        delta.events.push(event);
        delta.config_changed = true;
    }

    fn vlan_taken(&self, vlan: u16, except: Option<&EntityId>) -> Result<(), CoreError> {
        match self
            .fleet
            .networks
            .iter()
            .find(|network| network.vlan_id == Some(vlan) && Some(&network.id) != except)
        {
            Some(other) => Err(rejected(format!(
                "VLAN {vlan} is already used by {}",
                other.name
            ))),
            None => Ok(()),
        }
    }

    pub(super) fn create_network(
        &mut self,
        request: CreateNetworkRequest,
        now: DateTime<Utc>,
        delta: &mut Delta,
    ) -> Result<Network, CoreError> {
        let name = request.name.trim().to_owned();
        if name.is_empty() {
            return Err(invalid("a network needs a name"));
        }
        let vlan = request.vlan_id.unwrap_or(1);
        self.vlan_taken(vlan, None)?;
        let gateway = request.subnet.as_deref().map(parse_gateway).transpose()?;
        if gateway.is_none() && request.dhcp_enabled {
            return Err(invalid(
                "IPv4 and DHCP server settings require a gateway- or switch-managed network",
            ));
        }

        let mut network = Network {
            id: EntityId::Uuid(uuid_from(&mut self.rng)),
            name,
            enabled: request.enabled,
            management: Some(if gateway.is_some() {
                NetworkManagement::Gateway
            } else {
                NetworkManagement::Unmanaged
            }),
            purpose: Some(request.purpose.unwrap_or(NetworkPurpose::Corporate)),
            is_default: false,
            vlan_id: Some(vlan),
            subnet: gateway.as_ref().map(|(_, subnet)| subnet.clone()),
            gateway_ip: gateway.as_ref().map(|(host, _)| *host),
            dhcp: None,
            ipv6_enabled: false,
            ipv6_mode: None,
            ipv6_prefix: None,
            dhcpv6_enabled: false,
            slaac_enabled: false,
            ntp_server: None,
            pxe_enabled: false,
            tftp_server: None,
            firewall_zone_id: None,
            isolation_enabled: request.isolation_enabled,
            internet_access_enabled: request.internet_access_enabled,
            mdns_forwarding_enabled: false,
            cellular_backup_enabled: false,
            origin: Some(EntityOrigin::UserDefined),
            source: DataSource::IntegrationApi,
        };
        if request.dhcp_enabled {
            let update = DhcpUpdate {
                range_start: request.dhcp_range_start,
                range_stop: request.dhcp_range_stop,
                dns_servers: request.dns_servers,
            };
            apply_dhcp(&mut network, Some(true), Some(&update))?;
        }

        self.fleet.networks.push(network.clone());
        let message = format!("Network \"{}\" was created", network.name);
        self.admin_event(now, "EVT_AD_NetworkCreated", message, delta);
        Ok(network)
    }

    pub(super) fn update_network(
        &mut self,
        id: &EntityId,
        update: UpdateNetworkRequest,
        now: DateTime<Utc>,
        delta: &mut Delta,
    ) -> Result<(), CoreError> {
        if let Some(vlan) = update.vlan_id {
            self.vlan_taken(vlan, Some(id))?;
        }
        let stored = self
            .fleet
            .networks
            .iter_mut()
            .find(|network| &network.id == id)
            .ok_or_else(|| CoreError::NetworkNotFound {
                identifier: id.to_string(),
            })?;
        // Work on a copy, so a rejected update leaves nothing half applied.
        let mut network = stored.clone();
        if let Some(name) = update.name {
            network.name = name;
        }
        if let Some(vlan) = update.vlan_id {
            network.vlan_id = Some(vlan);
        }
        if let Some(enabled) = update.enabled {
            network.enabled = enabled;
        }
        if let Some(isolation) = update.isolation_enabled {
            network.isolation_enabled = isolation;
        }
        if let Some(internet) = update.internet_access_enabled {
            network.internet_access_enabled = internet;
        }
        if let Some(mdns) = update.mdns_forwarding_enabled {
            network.mdns_forwarding_enabled = mdns;
        }
        if let Some(ipv6) = update.ipv6_enabled {
            network.ipv6_enabled = ipv6;
            network.ipv6_mode = ipv6.then_some(Ipv6Mode::PrefixDelegation);
            if !ipv6 {
                network.ipv6_prefix = None;
            }
        }
        if let Some(cidr) = update.subnet.as_deref() {
            if network.management == Some(NetworkManagement::Unmanaged) {
                return Err(invalid(
                    "IPv4 and DHCP server settings require a gateway- or switch-managed network",
                ));
            }
            let (host, subnet) = parse_gateway(cidr)?;
            network.gateway_ip = Some(host);
            network.subnet = Some(subnet);
        }
        apply_dhcp(&mut network, update.dhcp_enabled, update.dhcp.as_ref())?;

        let message = format!("Network \"{}\" was updated", network.name);
        *stored = network;
        self.admin_event(now, "EVT_AD_NetworkUpdated", message, delta);
        Ok(())
    }

    pub(super) fn delete_network(
        &mut self,
        id: &EntityId,
        now: DateTime<Utc>,
        delta: &mut Delta,
    ) -> Result<(), CoreError> {
        let index = self
            .fleet
            .networks
            .iter()
            .position(|network| &network.id == id)
            .ok_or_else(|| CoreError::NetworkNotFound {
                identifier: id.to_string(),
            })?;
        // The fleet's own networks carry its devices and clients, and the
        // simulation finds them by position.
        if index < NETWORKS.len() {
            return Err(rejected(format!(
                "{} carries the demo fleet's devices and clients; only a network created in \
                 this session can be deleted",
                self.fleet.networks[index].name
            )));
        }
        let removed = self.fleet.networks.remove(index);
        let message = format!("Network \"{}\" was deleted", removed.name);
        self.admin_event(now, "EVT_AD_NetworkDeleted", message, delta);
        Ok(())
    }

    /// What uses a network, in the shape of the Integration API's
    /// references response.
    pub(crate) fn network_references(&self, id: &EntityId) -> Value {
        let clients = self
            .fleet
            .clients
            .iter()
            .filter(|client| client.client.network_id.as_ref() == Some(id))
            .count();
        let wifi: Vec<Value> = self
            .fleet
            .wifi
            .iter()
            .filter(|wifi| wifi.network_id.as_ref() == Some(id))
            .map(|wifi| json!({ "referenceId": wifi.id.to_string() }))
            .collect();
        let mut resources = Vec::new();
        if clients > 0 {
            resources.push(json!({ "resourceType": "CLIENT", "referenceCount": clients }));
        }
        if !wifi.is_empty() {
            resources.push(json!({
                "resourceType": "WIFI", "referenceCount": wifi.len(), "references": wifi,
            }));
        }
        json!({ "referenceResources": resources })
    }

    /// A zone pair's policies in evaluation order.
    fn zone_pair_policies(
        &self,
        source: &EntityId,
        destination: &EntityId,
    ) -> Vec<&FirewallPolicy> {
        let mut pair: Vec<&FirewallPolicy> = self
            .fleet
            .policies
            .iter()
            .filter(|policy| {
                policy.source.zone_id.as_ref() == Some(source)
                    && policy.destination.zone_id.as_ref() == Some(destination)
            })
            .collect();
        pair.sort_by_key(|policy| policy.index);
        pair
    }

    /// A zone pair's user policies, split around its system-defined ones.
    pub(crate) fn policy_ordering(
        &self,
        source: &EntityId,
        destination: &EntityId,
    ) -> FirewallPolicyOrdering {
        let pair = self.zone_pair_policies(source, destination);
        let system_at = pair.iter().position(|policy| is_fixed(policy));
        let mut ordering = FirewallPolicyOrdering {
            before_system_defined: Vec::new(),
            after_system_defined: Vec::new(),
        };
        for (position, policy) in pair.iter().enumerate() {
            let EntityId::Uuid(id) = policy.id else {
                continue;
            };
            if is_fixed(policy) {
                continue;
            }
            if system_at.is_some_and(|system| position > system) {
                ordering.after_system_defined.push(id);
            } else {
                ordering.before_system_defined.push(id);
            }
        }
        ordering
    }

    pub(super) fn reorder_policies(
        &mut self,
        (source, destination): &(EntityId, EntityId),
        before: &[EntityId],
        after: &[EntityId],
        now: DateTime<Utc>,
        delta: &mut Delta,
    ) -> Result<(), CoreError> {
        let pair = self.zone_pair_policies(source, destination);
        let mut slots: Vec<Option<i32>> = pair.iter().map(|policy| policy.index).collect();
        slots.sort_unstable();
        let fixed = pair.iter().filter(|policy| is_fixed(policy));
        let order: Vec<EntityId> = before
            .iter()
            .cloned()
            .chain(fixed.map(|policy| policy.id.clone()))
            .chain(after.iter().cloned())
            .collect();
        let mut wanted: Vec<&EntityId> = order.iter().collect();
        let mut held: Vec<&EntityId> = pair.iter().map(|policy| &policy.id).collect();
        wanted.sort_by_key(ToString::to_string);
        held.sort_by_key(ToString::to_string);
        if wanted != held {
            return Err(rejected("the ordering does not match the zone pair"));
        }

        for (id, index) in order.iter().zip(slots) {
            if let Some(policy) = self
                .fleet
                .policies
                .iter_mut()
                .find(|policy| &policy.id == id)
            {
                policy.index = index;
            }
        }
        let message = "Firewall policy order changed".to_owned();
        self.admin_event(now, "EVT_AD_FirewallPolicyChanged", message, delta);
        Ok(())
    }

    pub(super) fn delete_policy(
        &mut self,
        id: &EntityId,
        now: DateTime<Utc>,
        delta: &mut Delta,
    ) -> Result<(), CoreError> {
        let index = self
            .fleet
            .policies
            .iter()
            .position(|policy| &policy.id == id)
            .ok_or_else(|| not_found("firewall policy", id))?;
        if is_fixed(&self.fleet.policies[index]) {
            return Err(rejected("system-defined policies cannot be deleted"));
        }
        let removed = self.fleet.policies.remove(index);
        let message = format!("Firewall policy \"{}\" deleted", removed.name);
        self.admin_event(now, "EVT_AD_FirewallPolicyChanged", message, delta);
        Ok(())
    }

    pub(super) fn update_acl(
        &mut self,
        id: &EntityId,
        update: UpdateAclRuleRequest,
        now: DateTime<Utc>,
        delta: &mut Delta,
    ) -> Result<(), CoreError> {
        let rule = self
            .fleet
            .acls
            .iter_mut()
            .find(|rule| &rule.id == id)
            .ok_or_else(|| not_found("ACL rule", id))?;
        if let Some(enabled) = update.enabled {
            rule.enabled = enabled;
        }
        if let Some(name) = update.name {
            rule.name = name;
        }
        let state = if rule.enabled { "enabled" } else { "disabled" };
        let message = format!("ACL rule \"{}\" {state}", rule.name);
        self.admin_event(now, "EVT_AD_AclRuleChanged", message, delta);
        Ok(())
    }

    pub(super) fn delete_acl(
        &mut self,
        id: &EntityId,
        now: DateTime<Utc>,
        delta: &mut Delta,
    ) -> Result<(), CoreError> {
        let index = self
            .fleet
            .acls
            .iter()
            .position(|rule| &rule.id == id)
            .ok_or_else(|| not_found("ACL rule", id))?;
        let removed = self.fleet.acls.remove(index);
        let message = format!("ACL rule \"{}\" deleted", removed.name);
        self.admin_event(now, "EVT_AD_AclRuleChanged", message, delta);
        Ok(())
    }

    pub(super) fn update_nat(
        &mut self,
        id: &EntityId,
        update: UpdateNatPolicyRequest,
        now: DateTime<Utc>,
        delta: &mut Delta,
    ) -> Result<(), CoreError> {
        let rule = self
            .fleet
            .nat
            .iter_mut()
            .find(|rule| &rule.id == id)
            .ok_or_else(|| not_found("NAT rule", id))?;
        if let Some(enabled) = update.enabled {
            rule.enabled = enabled;
        }
        if let Some(name) = update.name {
            rule.name = name;
        }
        let state = if rule.enabled { "enabled" } else { "disabled" };
        let message = format!("NAT rule \"{}\" {state}", rule.name);
        self.admin_event(now, "EVT_AD_NatRuleChanged", message, delta);
        Ok(())
    }

    pub(super) fn delete_nat(
        &mut self,
        id: &EntityId,
        now: DateTime<Utc>,
        delta: &mut Delta,
    ) -> Result<(), CoreError> {
        let index = self
            .fleet
            .nat
            .iter()
            .position(|rule| &rule.id == id)
            .ok_or_else(|| not_found("NAT rule", id))?;
        if self.fleet.nat[index].origin == Some(EntityOrigin::SystemDefined) {
            return Err(rejected("system-defined NAT rules cannot be deleted"));
        }
        let removed = self.fleet.nat.remove(index);
        let message = format!("NAT rule \"{}\" deleted", removed.name);
        self.admin_event(now, "EVT_AD_NatRuleChanged", message, delta);
        Ok(())
    }
}

/// Whether the controller keeps a policy where it is: everything it did
/// not get from an admin.
fn is_fixed(policy: &FirewallPolicy) -> bool {
    policy
        .origin
        .is_some_and(|origin| origin != EntityOrigin::UserDefined)
}

#[cfg(test)]
mod tests;
