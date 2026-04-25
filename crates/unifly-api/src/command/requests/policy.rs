use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{EntityId, FirewallAction};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFirewallPolicyRequest {
    pub name: String,
    pub action: FirewallAction,
    #[serde(alias = "source_zone")]
    pub source_zone_id: EntityId,
    #[serde(alias = "dest_zone")]
    pub destination_zone_id: EntityId,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, alias = "logging")]
    pub logging_enabled: bool,
    #[serde(default = "default_true")]
    pub allow_return_traffic: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_states: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_filter: Option<TrafficFilterSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_filter: Option<TrafficFilterSpec>,

    // Shorthand fields for --from-file convenience (map to source/destination_filter)
    #[serde(default, skip_serializing)]
    pub src_network: Option<Vec<String>>,
    #[serde(default, skip_serializing)]
    pub src_ip: Option<Vec<String>>,
    #[serde(default, skip_serializing)]
    pub src_port: Option<Vec<String>>,
    #[serde(default, skip_serializing)]
    pub dst_network: Option<Vec<String>>,
    #[serde(default, skip_serializing)]
    pub dst_ip: Option<Vec<String>>,
    #[serde(default, skip_serializing)]
    pub dst_port: Option<Vec<String>>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateFirewallPolicyRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<FirewallAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_return_traffic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_states: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_filter: Option<TrafficFilterSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_filter: Option<TrafficFilterSpec>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "logging")]
    pub logging_enabled: Option<bool>,

    // Shorthand fields for --from-file convenience (map to source/destination_filter)
    #[serde(default, skip_serializing)]
    pub src_network: Option<Vec<String>>,
    #[serde(default, skip_serializing)]
    pub src_ip: Option<Vec<String>>,
    #[serde(default, skip_serializing)]
    pub src_port: Option<Vec<String>>,
    #[serde(default, skip_serializing)]
    pub dst_network: Option<Vec<String>>,
    #[serde(default, skip_serializing)]
    pub dst_ip: Option<Vec<String>>,
    #[serde(default, skip_serializing)]
    pub dst_port: Option<Vec<String>>,
}

/// Port-side specification: either inline values or a reference to a
/// firewall port-group by its `external_id`. Mirrors the controller's
/// portFilter wire shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PortSpec {
    /// Inline port values (single ports or ranges like `"8000-9000"`).
    Values {
        items: Vec<String>,
        #[serde(default)]
        match_opposite: bool,
    },
    /// Reference to a port-group via its `external_id` UUID.
    MatchingList {
        list_id: String,
        #[serde(default)]
        match_opposite: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    from = "TrafficFilterSpecWire"
)]
pub enum TrafficFilterSpec {
    Network {
        network_ids: Vec<String>,
        #[serde(default)]
        match_opposite: bool,
        /// Optional port restriction (the API nests portFilter inside the
        /// network/IP filter rather than treating it as a separate type).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ports: Option<PortSpec>,
    },
    IpAddress {
        addresses: Vec<String>,
        #[serde(default)]
        match_opposite: bool,
        /// Optional port restriction.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ports: Option<PortSpec>,
    },
    Port {
        ports: PortSpec,
    },
}

/// Internal wire-format wrapper used during deserialization to accept the
/// pre-PortSpec shape from existing JSON files. The legacy `Port` variant
/// stored ports as a flat `Vec<String>` with `match_opposite` at the
/// variant level instead of nested inside `PortSpec`. Both shapes round
/// through here into [`TrafficFilterSpec`].
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum TrafficFilterSpecWire {
    Network {
        network_ids: Vec<String>,
        #[serde(default)]
        match_opposite: bool,
        #[serde(default, deserialize_with = "deserialize_port_spec_opt")]
        ports: Option<PortSpec>,
    },
    IpAddress {
        addresses: Vec<String>,
        #[serde(default)]
        match_opposite: bool,
        #[serde(default, deserialize_with = "deserialize_port_spec_opt")]
        ports: Option<PortSpec>,
    },
    Port {
        #[serde(deserialize_with = "deserialize_port_spec")]
        ports: PortSpec,
        /// Legacy field: pre-PortSpec the Port variant carried
        /// `match_opposite` at the variant level. Folded into the inner
        /// `PortSpec` during conversion.
        #[serde(default)]
        match_opposite: bool,
    },
}

impl From<TrafficFilterSpecWire> for TrafficFilterSpec {
    fn from(wire: TrafficFilterSpecWire) -> Self {
        match wire {
            TrafficFilterSpecWire::Network {
                network_ids,
                match_opposite,
                ports,
            } => Self::Network {
                network_ids,
                match_opposite,
                ports,
            },
            TrafficFilterSpecWire::IpAddress {
                addresses,
                match_opposite,
                ports,
            } => Self::IpAddress {
                addresses,
                match_opposite,
                ports,
            },
            TrafficFilterSpecWire::Port {
                mut ports,
                match_opposite: legacy_mo,
            } => {
                if legacy_mo {
                    match &mut ports {
                        PortSpec::Values { match_opposite, .. }
                        | PortSpec::MatchingList { match_opposite, .. } => {
                            *match_opposite = *match_opposite || legacy_mo;
                        }
                    }
                }
                Self::Port { ports }
            }
        }
    }
}

/// Deserialize a [`PortSpec`] from either the new tagged shape
/// (`{"type": "values", "items": [...]}`) or the legacy flat
/// `Vec<String>` array used pre-PortSpec.
fn deserialize_port_spec<'de, D>(deserializer: D) -> Result<PortSpec, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Compat {
        Tagged(PortSpec),
        LegacyArray(Vec<String>),
    }
    Ok(match Compat::deserialize(deserializer)? {
        Compat::Tagged(spec) => spec,
        Compat::LegacyArray(items) => PortSpec::Values {
            items,
            match_opposite: false,
        },
    })
}

fn deserialize_port_spec_opt<'de, D>(deserializer: D) -> Result<Option<PortSpec>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Compat {
        Tagged(PortSpec),
        LegacyArray(Vec<String>),
    }
    let opt: Option<Compat> = Option::deserialize(deserializer)?;
    Ok(opt.map(|compat| match compat {
        Compat::Tagged(spec) => spec,
        Compat::LegacyArray(items) => PortSpec::Values {
            items,
            match_opposite: false,
        },
    }))
}

impl CreateFirewallPolicyRequest {
    /// Convert shorthand `src_ip`/`dst_ip`/`src_port`/`dst_port`/`src_network`/
    /// `dst_network` fields into the canonical `source_filter`/`destination_filter`.
    ///
    /// Returns `Err` if both a shorthand field and the corresponding filter are set,
    /// or if more than one shorthand family is specified for the same side.
    pub fn resolve_filters(&mut self) -> Result<(), String> {
        self.source_filter = resolve_side(
            "src",
            self.source_filter.take(),
            self.src_network.take(),
            self.src_ip.take(),
            self.src_port.take(),
        )?;
        self.destination_filter = resolve_side(
            "dst",
            self.destination_filter.take(),
            self.dst_network.take(),
            self.dst_ip.take(),
            self.dst_port.take(),
        )?;
        Ok(())
    }
}

impl UpdateFirewallPolicyRequest {
    /// Same as [`CreateFirewallPolicyRequest::resolve_filters`].
    pub fn resolve_filters(&mut self) -> Result<(), String> {
        self.source_filter = resolve_side(
            "src",
            self.source_filter.take(),
            self.src_network.take(),
            self.src_ip.take(),
            self.src_port.take(),
        )?;
        self.destination_filter = resolve_side(
            "dst",
            self.destination_filter.take(),
            self.dst_network.take(),
            self.dst_ip.take(),
            self.dst_port.take(),
        )?;
        Ok(())
    }
}

fn resolve_side(
    prefix: &str,
    existing: Option<TrafficFilterSpec>,
    networks: Option<Vec<String>>,
    ips: Option<Vec<String>>,
    ports: Option<Vec<String>>,
) -> Result<Option<TrafficFilterSpec>, String> {
    // network + ip is invalid; port can combine with either network or ip
    if networks.is_some() && ips.is_some() {
        return Err(format!("cannot combine {prefix}_network and {prefix}_ip"));
    }

    let has_shorthand = networks.is_some() || ips.is_some() || ports.is_some();

    if has_shorthand && existing.is_some() {
        return Err(format!(
            "cannot combine shorthand fields with {prefix_filter}",
            prefix_filter = if prefix == "src" {
                "source_filter"
            } else {
                "destination_filter"
            }
        ));
    }

    if let Some(existing) = existing {
        return Ok(Some(existing));
    }

    let port_spec = ports.map(|items| PortSpec::Values {
        items,
        match_opposite: false,
    });

    Ok(if let Some(network_ids) = networks {
        Some(TrafficFilterSpec::Network {
            network_ids,
            match_opposite: false,
            ports: port_spec,
        })
    } else if let Some(addresses) = ips {
        Some(TrafficFilterSpec::IpAddress {
            addresses,
            match_opposite: false,
            ports: port_spec,
        })
    } else {
        port_spec.map(|ports| TrafficFilterSpec::Port { ports })
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFirewallZoneRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(alias = "networks")]
    pub network_ids: Vec<EntityId>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateFirewallZoneRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "networks")]
    pub network_ids: Option<Vec<EntityId>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAclRuleRequest {
    pub name: String,
    #[serde(default = "default_acl_rule_type")]
    pub rule_type: String,
    pub action: FirewallAction,
    #[serde(alias = "source_zone")]
    pub source_zone_id: EntityId,
    #[serde(alias = "dest_zone")]
    pub destination_zone_id: EntityId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "src_port")]
    pub source_port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "dst_port")]
    pub destination_port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_filter: Option<TrafficFilterSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_filter: Option<TrafficFilterSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enforcing_device_filter: Option<Value>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_acl_rule_type() -> String {
    "IP".into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateAclRuleRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub rule_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<FirewallAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "source_zone")]
    pub source_zone_id: Option<EntityId>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "dest_zone")]
    pub destination_zone_id: Option<EntityId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "src_port")]
    pub source_port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "dst_port")]
    pub destination_port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_filter: Option<TrafficFilterSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_filter: Option<TrafficFilterSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enforcing_device_filter: Option<Value>,
}

// ── NAT Policy ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateNatPolicyRequest {
    pub name: String,
    /// masquerade | source | destination
    #[serde(rename = "type", alias = "nat_type")]
    pub nat_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface_id: Option<EntityId>,
    /// tcp | udp | tcp_udp | all
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub src_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub src_port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dst_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dst_port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translated_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translated_port: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateNatPolicyRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// masquerade | source | destination
    #[serde(
        rename = "type",
        alias = "nat_type",
        skip_serializing_if = "Option::is_none"
    )]
    pub nat_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface_id: Option<EntityId>,
    /// tcp | udp | tcp_udp | all
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub src_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub src_port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dst_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dst_port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translated_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translated_port: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        CreateAclRuleRequest, CreateFirewallPolicyRequest, PortSpec, TrafficFilterSpec,
        UpdateAclRuleRequest, UpdateFirewallPolicyRequest,
    };
    use crate::model::FirewallAction;

    /// Bug 1 regression: dst_ip and dst_port in --from-file JSON must
    /// deserialize into the shorthand fields (not be silently dropped).
    #[test]
    fn create_firewall_policy_shorthand_fields_deserialize() {
        let req: CreateFirewallPolicyRequest = serde_json::from_value(serde_json::json!({
            "name": "Allow Awair",
            "action": "Allow",
            "source_zone_id": "d2864b8e-56fb-4945-b69f-6d424fa5b248",
            "destination_zone_id": "5888bc93-aaae-4242-ae2f-2050d76211fd",
            "allow_return_traffic": false,
            "connection_states": ["NEW"],
            "dst_ip": ["10.0.40.10"],
            "dst_port": ["80"]
        }))
        .expect("shorthand fields should deserialize");

        assert_eq!(req.dst_ip.as_deref(), Some(&["10.0.40.10".to_owned()][..]));
        assert_eq!(req.dst_port.as_deref(), Some(&["80".to_owned()][..]));
        // Filter fields should still be None — resolution happens later
        assert!(req.destination_filter.is_none());
    }

    /// Shorthand fields must not leak into serialized output (they are
    /// internal to --from-file and should never reach the API wire format).
    #[test]
    fn create_firewall_policy_shorthand_fields_skip_serializing() {
        let req: CreateFirewallPolicyRequest = serde_json::from_value(serde_json::json!({
            "name": "Test",
            "action": "Block",
            "source_zone_id": "aaa",
            "destination_zone_id": "bbb",
            "dst_ip": ["10.0.0.1"]
        }))
        .expect("should deserialize");

        let value = serde_json::to_value(&req).expect("should serialize");
        assert!(value.get("dst_ip").is_none(), "dst_ip must not serialize");
        assert!(
            value.get("dst_port").is_none(),
            "dst_port must not serialize"
        );
        assert!(value.get("src_ip").is_none(), "src_ip must not serialize");
    }

    /// The existing source_filter / destination_filter path must still work
    /// for users who write the full TrafficFilterSpec in their JSON files.
    #[test]
    fn create_firewall_policy_full_filter_spec_still_works() {
        let req: CreateFirewallPolicyRequest = serde_json::from_value(serde_json::json!({
            "name": "Full filter",
            "action": "Allow",
            "source_zone_id": "aaa",
            "destination_zone_id": "bbb",
            "destination_filter": {
                "type": "ip_address",
                "addresses": ["10.0.40.10"],
                "match_opposite": false
            }
        }))
        .expect("full filter spec should deserialize");

        assert!(req.destination_filter.is_some());
        assert!(req.dst_ip.is_none());
    }

    /// dst_ip + dst_port should combine into IpAddress filter with nested ports
    #[test]
    fn resolve_filters_combines_dst_ip_and_dst_port() {
        let mut req: CreateFirewallPolicyRequest = serde_json::from_value(serde_json::json!({
            "name": "Allow Awair",
            "action": "Allow",
            "source_zone_id": "d2864b8e-56fb-4945-b69f-6d424fa5b248",
            "destination_zone_id": "5888bc93-aaae-4242-ae2f-2050d76211fd",
            "dst_ip": ["10.0.40.10"],
            "dst_port": ["80"]
        }))
        .expect("should deserialize");

        req.resolve_filters().expect("ip + port should be allowed");
        match &req.destination_filter {
            Some(TrafficFilterSpec::IpAddress {
                addresses, ports, ..
            }) => {
                assert_eq!(addresses, &["10.0.40.10"]);
                let Some(PortSpec::Values { items, .. }) = ports else {
                    panic!("expected PortSpec::Values, got {ports:?}")
                };
                assert_eq!(items, &["80"]);
            }
            other => panic!("expected IpAddress filter with ports, got {other:?}"),
        }
    }

    /// dst_network + dst_ip is still invalid (two primary filter types)
    #[test]
    fn resolve_filters_rejects_network_plus_ip() {
        let mut req: CreateFirewallPolicyRequest = serde_json::from_value(serde_json::json!({
            "name": "Conflict",
            "action": "Block",
            "source_zone_id": "aaa",
            "destination_zone_id": "bbb",
            "dst_network": ["net-uuid"],
            "dst_ip": ["10.0.0.1"]
        }))
        .expect("should deserialize");

        assert!(req.resolve_filters().is_err());
    }

    #[test]
    fn resolve_filters_converts_dst_ip_only() {
        let mut req: CreateFirewallPolicyRequest = serde_json::from_value(serde_json::json!({
            "name": "Allow Awair",
            "action": "Allow",
            "source_zone_id": "aaa",
            "destination_zone_id": "bbb",
            "dst_ip": ["10.0.40.10"]
        }))
        .expect("should deserialize");

        req.resolve_filters().expect("should resolve");
        match &req.destination_filter {
            Some(TrafficFilterSpec::IpAddress { addresses, .. }) => {
                assert_eq!(addresses, &["10.0.40.10"]);
            }
            other => panic!("expected IpAddress filter, got {other:?}"),
        }
    }

    #[test]
    fn resolve_filters_rejects_shorthand_plus_full_filter() {
        let mut req: CreateFirewallPolicyRequest = serde_json::from_value(serde_json::json!({
            "name": "Conflict",
            "action": "Block",
            "source_zone_id": "aaa",
            "destination_zone_id": "bbb",
            "dst_ip": ["10.0.0.1"],
            "destination_filter": {
                "type": "ip_address",
                "addresses": ["10.0.0.2"]
            }
        }))
        .expect("should deserialize");

        let err = req.resolve_filters().expect_err("should conflict");
        assert!(err.contains("cannot combine"), "got: {err}");
    }

    #[test]
    fn resolve_filters_update_request_works() {
        let mut req: UpdateFirewallPolicyRequest = serde_json::from_value(serde_json::json!({
            "dst_port": ["443", "8443"]
        }))
        .expect("should deserialize");

        req.resolve_filters().expect("should resolve");
        let Some(TrafficFilterSpec::Port {
            ports: PortSpec::Values { items, .. },
        }) = &req.destination_filter
        else {
            panic!(
                "expected Port filter with values, got {:?}",
                req.destination_filter
            )
        };
        assert_eq!(items, &["443", "8443"]);
    }

    /// Pre-PortSpec JSON files used a flat `Vec<String>` for `Port.ports`
    /// with `match_opposite` at the variant level. The new schema nests
    /// both inside `PortSpec`, but the deserializer must still accept the
    /// legacy shape so existing payloads keep working.
    #[test]
    fn destination_filter_accepts_legacy_port_variant_shape() {
        let req: CreateFirewallPolicyRequest = serde_json::from_value(serde_json::json!({
            "name": "Block port 80",
            "action": "Block",
            "source_zone_id": "d2864b8e-56fb-4945-b69f-6d424fa5b248",
            "destination_zone_id": "5888bc93-aaae-4242-ae2f-2050d76211fd",
            "destination_filter": {
                "type": "port",
                "ports": ["80"],
                "match_opposite": true
            }
        }))
        .expect("legacy port shape should still deserialize");

        let Some(TrafficFilterSpec::Port {
            ports:
                PortSpec::Values {
                    items,
                    match_opposite,
                },
        }) = &req.destination_filter
        else {
            panic!(
                "expected Port with PortSpec::Values, got {:?}",
                req.destination_filter
            )
        };
        assert_eq!(items, &["80"]);
        // Legacy outer match_opposite is folded into the inner PortSpec.
        assert!(*match_opposite);
    }

    /// Tagged PortSpec::MatchingList round-trips from JSON as a sibling of
    /// addresses (the shape PR 2's group resolver emits and what users will
    /// hand-write for direct group-uuid references).
    #[test]
    fn destination_filter_accepts_ip_address_with_port_matching_list() {
        let mut req: CreateFirewallPolicyRequest = serde_json::from_value(serde_json::json!({
            "name": "Apple Companion Link",
            "action": "Allow",
            "source_zone_id": "d2864b8e-56fb-4945-b69f-6d424fa5b248",
            "destination_zone_id": "5888bc93-aaae-4242-ae2f-2050d76211fd",
            "destination_filter": {
                "type": "ip_address",
                "addresses": ["10.0.10.2", "10.0.10.4"],
                "ports": {
                    "type": "matching_list",
                    "list_id": "24740a56-9cb9-4890-a5ac-589d30914a55"
                }
            }
        }))
        .expect("ip_address + port matching_list should deserialize");

        req.resolve_filters().expect("no shorthand, no-op");

        let Some(TrafficFilterSpec::IpAddress {
            addresses,
            ports: Some(PortSpec::MatchingList { list_id, .. }),
            ..
        }) = &req.destination_filter
        else {
            panic!(
                "expected IpAddress with PortSpec::MatchingList, got {:?}",
                req.destination_filter
            )
        };
        assert_eq!(addresses, &["10.0.10.2", "10.0.10.4"]);
        assert_eq!(list_id, "24740a56-9cb9-4890-a5ac-589d30914a55");
    }

    #[test]
    fn create_acl_rule_request_defaults_rule_type() {
        let request: CreateAclRuleRequest = serde_json::from_value(serde_json::json!({
            "name": "Allow IoT",
            "action": "Allow",
            "source_zone_id": "iot",
            "destination_zone_id": "lan",
            "enabled": true
        }))
        .expect("acl rule request should deserialize");

        assert_eq!(request.rule_type, "IP");
    }

    #[test]
    fn update_acl_rule_request_serializes_type_field() {
        let request = UpdateAclRuleRequest {
            rule_type: Some("DEVICE".into()),
            action: Some(FirewallAction::Allow),
            ..Default::default()
        };

        let value = serde_json::to_value(&request).expect("acl rule request should serialize");
        assert_eq!(
            value.get("type").and_then(serde_json::Value::as_str),
            Some("DEVICE")
        );
        assert_eq!(value.get("rule_type"), None);
    }
}
