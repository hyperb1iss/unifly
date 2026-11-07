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
    /// Report matched traffic in Insights Flows.
    #[serde(skip_serializing_if = "Option::is_none", alias = "reportFlowEnabled")]
    pub report_flow_enabled: Option<bool>,
    /// Create an additional mirrored-zone policy. Omitted means false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_return_traffic: Option<bool>,
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
    /// Report matched traffic in Insights Flows.
    #[serde(skip_serializing_if = "Option::is_none", alias = "reportFlowEnabled")]
    pub report_flow_enabled: Option<bool>,
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
    /// Address-group filter referencing a firewall group (address-group)
    /// by its `external_id`. May carry an optional port restriction in
    /// the same filter (mirrors what `IpAddress` supports for inline
    /// addresses).
    IpMatchingList {
        list_id: String,
        #[serde(default)]
        match_opposite: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ports: Option<PortSpec>,
    },
}

/// Internal wire-format wrapper used during deserialization to accept
/// pre-PortSpec JSON files. The legacy `Port` variant stored ports as a
/// flat `Vec<String>` with `match_opposite` at the variant level. The
/// legacy `port_matching_list` top-level variant carried a port-group
/// reference; it lowers to `Port { ports: PortSpec::MatchingList { ... } }`.
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
    /// Legacy top-level port-group reference. Lowered to `Port` with a
    /// nested `PortSpec::MatchingList` during conversion.
    PortMatchingList {
        list_id: String,
        #[serde(default)]
        match_opposite: bool,
    },
    /// Address-group filter. Optional `ports` companion supports rules
    /// like "members of address-group X on port-group Y" in one filter.
    IpMatchingList {
        list_id: String,
        #[serde(default)]
        match_opposite: bool,
        #[serde(default, deserialize_with = "deserialize_port_spec_opt")]
        ports: Option<PortSpec>,
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
            TrafficFilterSpecWire::PortMatchingList {
                list_id,
                match_opposite,
            } => Self::Port {
                ports: PortSpec::MatchingList {
                    list_id,
                    match_opposite,
                },
            },
            TrafficFilterSpecWire::IpMatchingList {
                list_id,
                match_opposite,
                ports,
            } => Self::IpMatchingList {
                list_id,
                match_opposite,
                ports,
            },
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
    /// Compatibility no-op retained for callers from the pre-0.10 request shape.
    ///
    /// Policy requests are canonical now; CLI-only shorthand fields are
    /// normalized before constructing this public API type.
    pub fn resolve_filters(&mut self) -> Result<(), String> {
        Ok(())
    }
}

impl UpdateFirewallPolicyRequest {
    /// Same as [`CreateFirewallPolicyRequest::resolve_filters`].
    pub fn resolve_filters(&mut self) -> Result<(), String> {
        Ok(())
    }
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

// ── Firewall Group ───────────────────────────────────────────

use crate::model::FirewallGroupType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFirewallGroupRequest {
    pub name: String,
    /// Group type. Required from `--from-file`; the CLI flag path always
    /// populates this. Accepts kebab-case (`"port-group"`,
    /// `"address-group"`, `"ipv6-address-group"`) matching the CLI
    /// `--type` flag, and PascalCase variant names for backward
    /// compatibility. Aliased as `type` so JSON files mirroring the
    /// CLI flag (`{"type": "address-group", ...}`) round-trip cleanly.
    #[serde(alias = "type")]
    pub group_type: FirewallGroupType,
    #[serde(alias = "members")]
    pub group_members: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateFirewallGroupRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "members")]
    pub group_members: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::{
        CreateAclRuleRequest, CreateFirewallGroupRequest, CreateFirewallPolicyRequest, PortSpec,
        TrafficFilterSpec, UpdateAclRuleRequest, UpdateFirewallGroupRequest,
    };
    use crate::model::{FirewallAction, FirewallGroupType};

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

    /// Firewall-group `--from-file` JSON should accept `members` (mirroring
    /// the CLI flag name) as well as the wire-level `group_members`.
    /// Otherwise serde silently drops the CLI-style field and a file
    /// written from `--help` output PUTs an unchanged group while
    /// reporting success.
    #[test]
    fn create_firewall_group_request_accepts_members_alias() {
        let req: CreateFirewallGroupRequest = serde_json::from_value(serde_json::json!({
            "name": "HA",
            "type": "port-group",
            "members": ["80", "8000-8002"]
        }))
        .expect("members alias should deserialize");

        assert_eq!(req.name, "HA");
        assert_eq!(req.group_members, vec!["80", "8000-8002"]);
    }

    /// `--from-file` JSON should accept the kebab-case `type` field
    /// (mirroring the CLI `--type` flag) and deserialize each known
    /// group type into its Rust variant. Without this, a file like
    /// `{"type": "address-group", ...}` was silently parsed as a port
    /// group via the previous default, corrupting the wire payload.
    #[test]
    fn create_firewall_group_request_kebab_case_type_alias() {
        let port: CreateFirewallGroupRequest = serde_json::from_value(serde_json::json!({
            "name": "HA",
            "type": "port-group",
            "members": ["80"]
        }))
        .expect("kebab-case port-group should deserialize");
        assert_eq!(port.group_type, FirewallGroupType::PortGroup);

        let addr: CreateFirewallGroupRequest = serde_json::from_value(serde_json::json!({
            "name": "Cloud IOT",
            "type": "address-group",
            "members": ["10.0.0.1"]
        }))
        .expect("kebab-case address-group should deserialize");
        assert_eq!(addr.group_type, FirewallGroupType::AddressGroup);

        let ipv6: CreateFirewallGroupRequest = serde_json::from_value(serde_json::json!({
            "name": "ULA",
            "type": "ipv6-address-group",
            "members": ["fd00::/8"]
        }))
        .expect("kebab-case ipv6-address-group should deserialize");
        assert_eq!(ipv6.group_type, FirewallGroupType::Ipv6AddressGroup);

        // PascalCase still works for backward compatibility with files
        // produced before the alias was added.
        let legacy: CreateFirewallGroupRequest = serde_json::from_value(serde_json::json!({
            "name": "HA",
            "group_type": "AddressGroup",
            "members": ["10.0.0.1"]
        }))
        .expect("PascalCase variant should deserialize");
        assert_eq!(legacy.group_type, FirewallGroupType::AddressGroup);
    }

    /// Missing type should now error rather than silently default to
    /// `port-group` -- a payload like `{"name":"x","members":["10.0.0.1"]}`
    /// was getting silently classified as a port group with addresses
    /// as members, producing an invalid wire payload.
    #[test]
    fn create_firewall_group_request_requires_type() {
        let result: Result<CreateFirewallGroupRequest, _> =
            serde_json::from_value(serde_json::json!({
                "name": "Cloud IOT",
                "members": ["10.0.0.1"]
            }));
        assert!(
            result.is_err(),
            "missing `type` / `group_type` should not silently default to PortGroup"
        );
    }

    #[test]
    fn update_firewall_group_request_accepts_members_alias() {
        let req: UpdateFirewallGroupRequest = serde_json::from_value(serde_json::json!({
            "members": ["80", "443"]
        }))
        .expect("members alias should deserialize");

        assert_eq!(
            req.group_members.as_deref(),
            Some(&["80".into(), "443".into()][..])
        );
    }

    // ── TrafficFilterSpec matching list variants ───────────────────

    /// Port-group references are modeled as `Port { ports: PortSpec::MatchingList }`.
    /// The legacy `port_matching_list` top-level variant is accepted on
    /// deserialize and lowered to the new shape.
    #[test]
    fn port_group_reference_round_trips_via_port_variant() {
        let spec = TrafficFilterSpec::Port {
            ports: PortSpec::MatchingList {
                list_id: "24740a56-9cb9-4890-a5ac-589d30914a55".into(),
                match_opposite: false,
            },
        };
        let json = serde_json::to_value(&spec).expect("should serialize");
        assert_eq!(json.get("type").and_then(|v| v.as_str()), Some("port"));

        // Legacy port_matching_list shape still deserializes (lowered to Port).
        let legacy = serde_json::json!({
            "type": "port_matching_list",
            "list_id": "24740a56-9cb9-4890-a5ac-589d30914a55",
            "match_opposite": false,
        });
        let from_legacy: TrafficFilterSpec =
            serde_json::from_value(legacy).expect("legacy shape should deserialize");
        assert!(matches!(
            from_legacy,
            TrafficFilterSpec::Port {
                ports: PortSpec::MatchingList { .. },
            }
        ));
    }

    #[test]
    fn ip_matching_list_round_trips() {
        let spec = TrafficFilterSpec::IpMatchingList {
            list_id: "b777b27c-410c-4b40-8489-a61bf1a536d4".into(),
            match_opposite: true,
            ports: None,
        };
        let json = serde_json::to_value(&spec).expect("should serialize");
        assert_eq!(
            json.get("type").and_then(|v| v.as_str()),
            Some("ip_matching_list")
        );

        let round_tripped: TrafficFilterSpec =
            serde_json::from_value(json).expect("should deserialize");
        match round_tripped {
            TrafficFilterSpec::IpMatchingList { match_opposite, .. } => assert!(match_opposite),
            other => panic!("expected IpMatchingList, got {other:?}"),
        }
    }
}
