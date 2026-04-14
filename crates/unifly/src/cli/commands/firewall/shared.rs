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

pub(super) fn build_filter_spec(
    field_prefix: &str,
    networks: Option<Vec<String>>,
    ips: Option<Vec<String>>,
    ports: Option<Vec<String>>,
) -> Result<Option<TrafficFilterSpec>, CliError> {
    // network + ip is invalid; port can combine with either
    if networks.is_some() && ips.is_some() {
        return Err(CliError::Validation {
            field: format!("{field_prefix}-filter"),
            reason: format!("cannot combine --{field_prefix}-network and --{field_prefix}-ip"),
        });
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

/// Merge `src_port_group` / `src_address_group` / `dst_port_group` /
/// `dst_address_group` shorthands into `source_filter` / `destination_filter`
/// on a `CreateFirewallPolicyRequest`.
///
/// Must be called *after* `resolve_filters()` so the shorthand fields
/// (`src_ip`, `src_port`, etc.) are already folded into the filter.
///
/// Combinations supported:
/// * address group alone → `IpMatchingList`
/// * port group alone → `Port` carrying `PortSpec::MatchingList`
/// * address group + port group → `IpMatchingList` with `ports` companion
/// * existing inline-IP/network/address-group filter + port group →
///   port-group becomes the `ports` companion
/// * existing port-only filter + address group → upgraded to
///   `IpMatchingList` carrying the existing port spec
pub(super) fn resolve_group_refs_create(
    controller: &Controller,
    req: &mut CreateFirewallPolicyRequest,
) -> Result<(), CliError> {
    let groups = controller.firewall_groups_snapshot();

    req.source_filter = merge_groups_into_filter(
        "src",
        req.source_filter.take(),
        req.src_address_group.take(),
        req.src_port_group.take(),
        &groups,
    )?;
    req.destination_filter = merge_groups_into_filter(
        "dst",
        req.destination_filter.take(),
        req.dst_address_group.take(),
        req.dst_port_group.take(),
        &groups,
    )?;
    Ok(())
}

/// Same as [`resolve_group_refs_create`] but for update requests.
pub(super) fn resolve_group_refs_update(
    controller: &Controller,
    req: &mut UpdateFirewallPolicyRequest,
) -> Result<(), CliError> {
    let groups = controller.firewall_groups_snapshot();

    req.source_filter = merge_groups_into_filter(
        "src",
        req.source_filter.take(),
        req.src_address_group.take(),
        req.src_port_group.take(),
        &groups,
    )?;
    req.destination_filter = merge_groups_into_filter(
        "dst",
        req.destination_filter.take(),
        req.dst_address_group.take(),
        req.dst_port_group.take(),
        &groups,
    )?;
    Ok(())
}

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
    use super::{build_filter_spec, parse_reorder_zone_pair};
    use crate::cli::error::CliError;
    use unifly_api::{EntityId, PortSpec, TrafficFilterSpec};

    #[test]
    fn build_filter_spec_accepts_single_filter_family() {
        let spec = build_filter_spec("src", Some(vec!["lan".into()]), None, None);
        assert!(matches!(spec, Ok(Some(TrafficFilterSpec::Network { .. }))));
    }

    #[test]
    fn build_filter_spec_rejects_multiple_filter_families() {
        let err = build_filter_spec(
            "src",
            Some(vec!["lan".into()]),
            Some(vec!["10.0.0.1".into()]),
            None,
        );

        match err {
            Err(CliError::Validation { field, .. }) => assert_eq!(field, "src-filter"),
            Ok(_) => panic!("expected validation error, got success"),
            Err(other) => panic!("expected validation error, got {other:?}"),
        }
    }

    #[test]
    fn build_filter_spec_combines_ip_and_port() {
        let spec = build_filter_spec(
            "dst",
            None,
            Some(vec!["10.0.40.10".into()]),
            Some(vec!["80".into()]),
        )
        .expect("ip + port should succeed");

        match spec {
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
