use unifly_api::model::FirewallAction as ModelFirewallAction;
use unifly_api::{EntityId, PortSpec, TrafficFilterSpec};

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
