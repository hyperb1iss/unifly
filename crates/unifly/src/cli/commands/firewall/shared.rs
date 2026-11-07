use serde::Deserialize;
use unifly_api::model::{FirewallAction as ModelFirewallAction, FirewallGroupType};
use unifly_api::{
    Controller, CreateFirewallPolicyRequest, EntityId, PortSpec, TrafficFilterSpec,
    UpdateFirewallPolicyRequest,
};

use crate::cli::args::FirewallAction;
use crate::cli::error::CliError;

pub(super) fn map_fw_action(action: &FirewallAction) -> ModelFirewallAction {
    match action {
        FirewallAction::Allow => ModelFirewallAction::Allow,
        FirewallAction::Block => ModelFirewallAction::Block,
        FirewallAction::Reject => ModelFirewallAction::Reject,
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct CreatePolicyInput {
    pub name: String,
    pub action: ModelFirewallAction,
    #[serde(alias = "source_zone")]
    pub source_zone_id: EntityId,
    #[serde(alias = "dest_zone")]
    pub destination_zone_id: EntityId,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, alias = "logging")]
    pub logging_enabled: bool,
    #[serde(alias = "reportFlowEnabled")]
    pub report_flow_enabled: Option<bool>,
    #[serde(default)]
    pub allow_return_traffic: Option<bool>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub ip_version: Option<String>,
    #[serde(default)]
    pub connection_states: Option<Vec<String>>,
    #[serde(flatten)]
    pub filters: PolicyFilterInput,
}

impl CreatePolicyInput {
    pub(super) fn into_request(self) -> CreateFirewallPolicyRequest {
        debug_assert!(!self.filters.has_group_refs());
        let (source_filter, destination_filter) = self.filters.into_filters();
        CreateFirewallPolicyRequest {
            name: self.name,
            action: self.action,
            source_zone_id: self.source_zone_id,
            destination_zone_id: self.destination_zone_id,
            enabled: self.enabled,
            logging_enabled: self.logging_enabled,
            report_flow_enabled: self.report_flow_enabled,
            allow_return_traffic: self.allow_return_traffic,
            description: self.description,
            ip_version: self.ip_version,
            connection_states: self.connection_states,
            source_filter,
            destination_filter,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct UpdatePolicyInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub action: Option<ModelFirewallAction>,
    #[serde(default)]
    pub allow_return_traffic: Option<bool>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub ip_version: Option<String>,
    #[serde(default)]
    pub connection_states: Option<Vec<String>>,
    #[serde(default, alias = "logging")]
    pub logging_enabled: Option<bool>,
    #[serde(alias = "reportFlowEnabled")]
    pub report_flow_enabled: Option<bool>,
    #[serde(flatten)]
    pub filters: PolicyFilterInput,
}

impl UpdatePolicyInput {
    pub(super) fn into_request(self) -> UpdateFirewallPolicyRequest {
        debug_assert!(!self.filters.has_group_refs());
        let (source_filter, destination_filter) = self.filters.into_filters();
        UpdateFirewallPolicyRequest {
            name: self.name,
            action: self.action,
            allow_return_traffic: self.allow_return_traffic,
            enabled: self.enabled,
            description: self.description,
            ip_version: self.ip_version,
            connection_states: self.connection_states,
            source_filter,
            destination_filter,
            logging_enabled: self.logging_enabled,
            report_flow_enabled: self.report_flow_enabled,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct PolicyFilterInput {
    #[serde(default)]
    source_filter: Option<TrafficFilterSpec>,
    #[serde(default)]
    destination_filter: Option<TrafficFilterSpec>,

    #[serde(default)]
    src_network: Option<Vec<String>>,
    #[serde(default)]
    src_ip: Option<Vec<String>>,
    #[serde(default)]
    src_port: Option<Vec<String>>,
    #[serde(default)]
    dst_network: Option<Vec<String>>,
    #[serde(default)]
    dst_ip: Option<Vec<String>>,
    #[serde(default)]
    dst_port: Option<Vec<String>>,

    #[serde(default)]
    src_port_group: Option<String>,
    #[serde(default)]
    dst_port_group: Option<String>,
    #[serde(default)]
    src_address_group: Option<String>,
    #[serde(default)]
    dst_address_group: Option<String>,
}

impl PolicyFilterInput {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn from_cli(
        src_network: Option<Vec<String>>,
        src_ip: Option<Vec<String>>,
        src_port: Option<Vec<String>>,
        dst_network: Option<Vec<String>>,
        dst_ip: Option<Vec<String>>,
        dst_port: Option<Vec<String>>,
        src_port_group: Option<String>,
        dst_port_group: Option<String>,
        src_address_group: Option<String>,
        dst_address_group: Option<String>,
    ) -> Result<Self, CliError> {
        let mut input = Self {
            src_network,
            src_ip,
            src_port,
            dst_network,
            dst_ip,
            dst_port,
            src_port_group,
            dst_port_group,
            src_address_group,
            dst_address_group,
            ..Self::default()
        };
        input.resolve_inline_filters()?;
        Ok(input)
    }

    pub(super) fn resolve_inline_filters(&mut self) -> Result<(), CliError> {
        self.source_filter = resolve_filter_side(
            "src",
            self.source_filter.take(),
            self.src_network.take(),
            self.src_ip.take(),
            self.src_port.take(),
        )?;
        self.destination_filter = resolve_filter_side(
            "dst",
            self.destination_filter.take(),
            self.dst_network.take(),
            self.dst_ip.take(),
            self.dst_port.take(),
        )?;
        Ok(())
    }

    pub(super) fn has_group_refs(&self) -> bool {
        self.src_port_group.is_some()
            || self.dst_port_group.is_some()
            || self.src_address_group.is_some()
            || self.dst_address_group.is_some()
    }

    pub(super) fn resolve_group_refs(&mut self, controller: &Controller) -> Result<(), CliError> {
        let groups = controller.firewall_groups_snapshot();

        self.source_filter = merge_groups_into_filter(
            "src",
            self.source_filter.take(),
            self.src_address_group.take(),
            self.src_port_group.take(),
            &groups,
        )?;
        self.destination_filter = merge_groups_into_filter(
            "dst",
            self.destination_filter.take(),
            self.dst_address_group.take(),
            self.dst_port_group.take(),
            &groups,
        )?;
        Ok(())
    }

    pub(super) fn into_filters(self) -> (Option<TrafficFilterSpec>, Option<TrafficFilterSpec>) {
        (self.source_filter, self.destination_filter)
    }
}

fn default_true() -> bool {
    true
}

fn resolve_filter_side(
    field_prefix: &str,
    existing: Option<TrafficFilterSpec>,
    networks: Option<Vec<String>>,
    ips: Option<Vec<String>>,
    ports: Option<Vec<String>>,
) -> Result<Option<TrafficFilterSpec>, CliError> {
    if networks.is_some() && ips.is_some() {
        return Err(CliError::Validation {
            field: format!("{field_prefix}-filter"),
            reason: format!("cannot combine --{field_prefix}-network and --{field_prefix}-ip"),
        });
    }

    let has_shorthand = networks.is_some() || ips.is_some() || ports.is_some();
    if has_shorthand && existing.is_some() {
        let field_name = if field_prefix == "src" {
            "source_filter"
        } else {
            "destination_filter"
        };
        return Err(CliError::Validation {
            field: format!("{field_prefix}-filter"),
            reason: format!("cannot combine shorthand fields with {field_name}"),
        });
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

pub(super) fn parse_reorder_zone_pair(
    source_zone: Option<&str>,
    dest_zone: Option<&str>,
) -> Result<(EntityId, EntityId), CliError> {
    match (source_zone, dest_zone) {
        (Some(source_zone), Some(dest_zone)) => {
            Ok((EntityId::from(source_zone), EntityId::from(dest_zone)))
        }
        _ => Err(CliError::Validation {
            field: "zone-pair".into(),
            reason: "firewall policy reordering requires both --source-zone and --dest-zone".into(),
        }),
    }
}

/// Merge group shorthands into canonical source/destination filters.
///
/// Must be called after inline shorthand fields are already folded into
/// the filter.
///
/// Combinations supported:
/// * address group alone → `IpMatchingList`
/// * port group alone → `Port` carrying `PortSpec::MatchingList`
/// * address group + port group → `IpMatchingList` with `ports` companion
/// * existing inline-IP/network/address-group filter + port group →
///   port-group becomes the `ports` companion
/// * existing port-only filter + address group → upgraded to
///   `IpMatchingList` carrying the existing port spec
fn merge_groups_into_filter(
    side: &str,
    existing: Option<TrafficFilterSpec>,
    address_group: Option<String>,
    port_group: Option<String>,
    groups: &[std::sync::Arc<unifly_api::model::FirewallGroup>],
) -> Result<Option<TrafficFilterSpec>, CliError> {
    if address_group.is_none() && port_group.is_none() {
        return Ok(existing);
    }

    let port_group_spec = port_group
        .map(|name| resolve_port_group_spec(&name, groups))
        .transpose()?;
    let address_group_id = address_group
        .map(|name| resolve_address_group_id(&name, groups))
        .transpose()?;

    Ok(Some(match (existing, address_group_id, port_group_spec) {
        // No existing filter — build from scratch.
        (None, Some(list_id), None) => TrafficFilterSpec::IpMatchingList {
            list_id,
            match_opposite: false,
            ports: None,
        },
        (None, None, Some(spec)) => TrafficFilterSpec::Port { ports: spec },
        (None, Some(list_id), Some(spec)) => TrafficFilterSpec::IpMatchingList {
            list_id,
            match_opposite: false,
            ports: Some(spec),
        },

        // Existing filter — try to merge groups in as companions.
        (Some(filter), addr, port_spec) => merge_into_existing(side, filter, addr, port_spec)?,

        (None, None, None) => unreachable!("checked above"),
    }))
}

fn merge_into_existing(
    side: &str,
    filter: TrafficFilterSpec,
    address_group_id: Option<String>,
    port_group_spec: Option<PortSpec>,
) -> Result<TrafficFilterSpec, CliError> {
    match (filter, address_group_id, port_group_spec) {
        // address-group + existing address-side filter → conflict.
        (
            TrafficFilterSpec::Network { .. }
            | TrafficFilterSpec::IpAddress { .. }
            | TrafficFilterSpec::IpMatchingList { .. },
            Some(_),
            _,
        ) => Err(CliError::Validation {
            field: format!("{side}_address_group"),
            reason: format!(
                "--{side}-address-group conflicts with --{side}-network or --{side}-ip"
            ),
        }),

        // address-group + existing port-only filter → upgrade to
        // IpMatchingList carrying the port spec.
        (TrafficFilterSpec::Port { ports }, Some(list_id), None) => {
            Ok(TrafficFilterSpec::IpMatchingList {
                list_id,
                match_opposite: false,
                ports: Some(ports),
            })
        }

        // port-group + existing filter without ports → add as companion.
        (
            TrafficFilterSpec::Network {
                network_ids,
                match_opposite,
                ports: None,
            },
            None,
            Some(spec),
        ) => Ok(TrafficFilterSpec::Network {
            network_ids,
            match_opposite,
            ports: Some(spec),
        }),
        (
            TrafficFilterSpec::IpAddress {
                addresses,
                match_opposite,
                ports: None,
            },
            None,
            Some(spec),
        ) => Ok(TrafficFilterSpec::IpAddress {
            addresses,
            match_opposite,
            ports: Some(spec),
        }),
        (
            TrafficFilterSpec::IpMatchingList {
                list_id,
                match_opposite,
                ports: None,
            },
            None,
            Some(spec),
        ) => Ok(TrafficFilterSpec::IpMatchingList {
            list_id,
            match_opposite,
            ports: Some(spec),
        }),

        // port-group + existing filter that already has ports → two
        // port scopes (Port variant, or any *-with-ports variant, or
        // existing Port + address-group attempting upgrade).
        (_, _, Some(_)) => Err(CliError::Validation {
            field: format!("{side}_port_group"),
            reason: format!("--{side}-port-group conflicts with --{side}-port"),
        }),

        // No groups left — caller filtered this; preserve the filter.
        (filter, None, None) => Ok(filter),
    }
}

fn resolve_port_group_spec(
    name: &str,
    groups: &[std::sync::Arc<unifly_api::model::FirewallGroup>],
) -> Result<PortSpec, CliError> {
    let group = groups
        .iter()
        .find(|g| g.name == name)
        .ok_or_else(|| CliError::Validation {
            field: "port_group".into(),
            reason: format!("firewall group \"{name}\" not found"),
        })?;
    if group.group_type != FirewallGroupType::PortGroup {
        return Err(CliError::Validation {
            field: "port_group".into(),
            reason: format!(
                "firewall group \"{name}\" is a {}, not a port-group",
                group.group_type
            ),
        });
    }
    let list_id = group
        .external_id
        .as_ref()
        .ok_or_else(|| CliError::Validation {
            field: "port_group".into(),
            reason: format!("firewall group \"{name}\" has no external_id"),
        })?;
    Ok(PortSpec::MatchingList {
        list_id: list_id.clone(),
        match_opposite: false,
    })
}

fn resolve_address_group_id(
    name: &str,
    groups: &[std::sync::Arc<unifly_api::model::FirewallGroup>],
) -> Result<String, CliError> {
    let group = groups
        .iter()
        .find(|g| g.name == name)
        .ok_or_else(|| CliError::Validation {
            field: "address_group".into(),
            reason: format!("firewall group \"{name}\" not found"),
        })?;
    if group.group_type != FirewallGroupType::AddressGroup
        && group.group_type != FirewallGroupType::Ipv6AddressGroup
    {
        return Err(CliError::Validation {
            field: "address_group".into(),
            reason: format!(
                "firewall group \"{name}\" is a {}, not an address-group",
                group.group_type
            ),
        });
    }
    let list_id = group
        .external_id
        .as_ref()
        .ok_or_else(|| CliError::Validation {
            field: "address_group".into(),
            reason: format!("firewall group \"{name}\" has no external_id"),
        })?;
    Ok(list_id.clone())
}

#[cfg(test)]
mod tests {
    use super::{CreatePolicyInput, PolicyFilterInput, UpdatePolicyInput, parse_reorder_zone_pair};
    use crate::cli::error::CliError;
    use unifly_api::model::FirewallAction as ModelFirewallAction;
    use unifly_api::{EntityId, PortSpec, TrafficFilterSpec};

    #[test]
    fn policy_filter_input_accepts_single_filter_family() {
        let input = PolicyFilterInput::from_cli(
            Some(vec!["lan".into()]),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("network filter should resolve");
        let (source, _) = input.into_filters();

        assert!(matches!(source, Some(TrafficFilterSpec::Network { .. })));
    }

    #[test]
    fn policy_filter_input_rejects_multiple_filter_families() {
        let err = PolicyFilterInput::from_cli(
            Some(vec!["lan".into()]),
            Some(vec!["10.0.0.1".into()]),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        match err {
            Err(CliError::Validation { field, .. }) => assert_eq!(field, "src-filter"),
            Ok(_) => panic!("expected validation error, got success"),
            Err(other) => panic!("expected validation error, got {other:?}"),
        }
    }

    #[test]
    fn policy_filter_input_combines_ip_and_port() {
        let input = PolicyFilterInput::from_cli(
            None,
            None,
            None,
            None,
            Some(vec!["10.0.40.10".into()]),
            Some(vec!["80".into()]),
            None,
            None,
            None,
            None,
        )
        .expect("ip + port should succeed");
        let (_, destination) = input.into_filters();

        match destination {
            Some(TrafficFilterSpec::IpAddress {
                addresses, ports, ..
            }) => {
                assert_eq!(addresses, vec!["10.0.40.10"]);
                let Some(PortSpec::Values { items, .. }) = ports else {
                    panic!("expected PortSpec::Values, got {ports:?}")
                };
                assert_eq!(items, vec!["80"]);
            }
            other => panic!("expected IpAddress with ports, got {other:?}"),
        }
    }

    #[test]
    fn create_policy_input_deserializes_shorthand_fields() {
        let mut input: CreatePolicyInput = serde_json::from_value(serde_json::json!({
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

        input
            .filters
            .resolve_inline_filters()
            .expect("ip + port should resolve");
        let request = input.into_request();
        match request.destination_filter {
            Some(TrafficFilterSpec::IpAddress {
                addresses, ports, ..
            }) => {
                assert_eq!(addresses, &["10.0.40.10"]);
                let Some(PortSpec::Values { items, .. }) = ports else {
                    panic!("expected PortSpec::Values, got {ports:?}");
                };
                assert_eq!(items, &["80"]);
            }
            other => panic!("expected IpAddress filter with ports, got {other:?}"),
        }
    }

    #[test]
    fn create_policy_input_accepts_lowercase_action() {
        let input: CreatePolicyInput = serde_json::from_value(serde_json::json!({
            "name": "Allow Awair",
            "action": "allow",
            "source_zone_id": "d2864b8e-56fb-4945-b69f-6d424fa5b248",
            "destination_zone_id": "5888bc93-aaae-4242-ae2f-2050d76211fd"
        }))
        .expect("lowercase action should deserialize");

        let request = input.into_request();

        assert_eq!(request.action, ModelFirewallAction::Allow);
    }

    #[test]
    fn policy_filter_input_rejects_shorthand_plus_full_filter() {
        let mut input: PolicyFilterInput = serde_json::from_value(serde_json::json!({
            "dst_ip": ["10.0.0.1"],
            "destination_filter": {
                "type": "ip_address",
                "addresses": ["10.0.0.2"]
            }
        }))
        .expect("input should deserialize");

        let err = input
            .resolve_inline_filters()
            .expect_err("should reject mixed filter shapes");
        match err {
            CliError::Validation { reason, .. } => assert!(reason.contains("cannot combine")),
            other => panic!("expected validation error, got {other:?}"),
        }
    }

    #[test]
    fn update_policy_input_deserializes_group_shorthand() {
        let input: UpdatePolicyInput = serde_json::from_value(serde_json::json!({
            "dst_port_group": "HA"
        }))
        .expect("update group shorthand should deserialize");

        assert!(input.filters.has_group_refs());
    }

    #[test]
    fn update_policy_input_deserializes_dst_port_filter() {
        let mut input: UpdatePolicyInput = serde_json::from_value(serde_json::json!({
            "dst_port": ["80", "443"]
        }))
        .expect("update port shorthand should deserialize");

        input
            .filters
            .resolve_inline_filters()
            .expect("port shorthand should resolve");
        let request = input.into_request();

        let Some(TrafficFilterSpec::Port {
            ports: PortSpec::Values { items, .. },
        }) = request.destination_filter
        else {
            panic!(
                "expected destination port filter, got {:?}",
                request.destination_filter
            );
        };
        assert_eq!(items, vec!["80", "443"]);
    }

    #[test]
    fn merge_groups_into_existing_ip_address_adds_port_companion() {
        use super::merge_into_existing;

        let existing = TrafficFilterSpec::IpAddress {
            addresses: vec!["10.0.0.5".into()],
            match_opposite: false,
            ports: None,
        };
        let port_spec = PortSpec::MatchingList {
            list_id: "web-ports-uuid".into(),
            match_opposite: false,
        };

        let merged = merge_into_existing("dst", existing, None, Some(port_spec))
            .expect("ip + port-group should merge");

        let TrafficFilterSpec::IpAddress {
            addresses,
            ports: Some(PortSpec::MatchingList { list_id, .. }),
            ..
        } = merged
        else {
            panic!("expected IpAddress with port matching list, got {merged:?}")
        };
        assert_eq!(addresses, vec!["10.0.0.5"]);
        assert_eq!(list_id, "web-ports-uuid");
    }

    #[test]
    fn merge_groups_into_port_only_filter_upgrades_to_ip_matching_list() {
        use super::merge_into_existing;

        let existing = TrafficFilterSpec::Port {
            ports: PortSpec::Values {
                items: vec!["443".into()],
                match_opposite: false,
            },
        };

        let merged = merge_into_existing("dst", existing, Some("servers-uuid".into()), None)
            .expect("address-group + existing port should upgrade to IpMatchingList");

        let TrafficFilterSpec::IpMatchingList {
            list_id,
            ports: Some(PortSpec::Values { items, .. }),
            ..
        } = merged
        else {
            panic!("expected IpMatchingList with port values, got {merged:?}")
        };
        assert_eq!(list_id, "servers-uuid");
        assert_eq!(items, vec!["443"]);
    }

    #[test]
    fn merge_groups_rejects_two_address_scopes() {
        use super::merge_into_existing;

        let existing = TrafficFilterSpec::IpAddress {
            addresses: vec!["10.0.0.5".into()],
            match_opposite: false,
            ports: None,
        };
        let err = merge_into_existing("dst", existing, Some("group-uuid".into()), None);
        assert!(matches!(err, Err(CliError::Validation { .. })));
    }

    #[test]
    fn merge_groups_rejects_two_port_scopes() {
        use super::merge_into_existing;

        let existing = TrafficFilterSpec::IpAddress {
            addresses: vec!["10.0.0.5".into()],
            match_opposite: false,
            ports: Some(PortSpec::Values {
                items: vec!["443".into()],
                match_opposite: false,
            }),
        };
        let port_spec = PortSpec::MatchingList {
            list_id: "web-ports-uuid".into(),
            match_opposite: false,
        };
        let err = merge_into_existing("dst", existing, None, Some(port_spec));
        assert!(matches!(err, Err(CliError::Validation { .. })));
    }

    #[test]
    fn parse_reorder_zone_pair_requires_both_zones() {
        let err = parse_reorder_zone_pair(Some("src"), None)
            .expect_err("missing destination zone should fail");
        match err {
            CliError::Validation { field, .. } => assert_eq!(field, "zone-pair"),
            other => panic!("expected validation error, got {other:?}"),
        }
    }

    #[test]
    fn parse_reorder_zone_pair_returns_entity_ids() {
        let zone_pair = parse_reorder_zone_pair(
            Some("550e8400-e29b-41d4-a716-446655440000"),
            Some("550e8400-e29b-41d4-a716-446655440001"),
        )
        .expect("zone pair should parse");
        assert_eq!(
            zone_pair,
            (
                EntityId::from("550e8400-e29b-41d4-a716-446655440000"),
                EntityId::from("550e8400-e29b-41d4-a716-446655440001"),
            )
        );
    }
}
