use crate::integration_types;
use crate::model::common::DataSource;
use crate::model::entity_id::EntityId;
use crate::model::firewall::{
    AclAction, AclRule, AclRuleType, FirewallAction, FirewallGroup, FirewallGroupType,
    FirewallPolicy, FirewallZone, IpSpec, PolicyEndpoint, PortSpec, TrafficFilter,
};

use super::helpers::origin_from_metadata;

// ── Firewall Policy ──────────────────────────────────────────────

impl From<integration_types::FirewallPolicyResponse> for FirewallPolicy {
    fn from(p: integration_types::FirewallPolicyResponse) -> Self {
        let action = p.action.get("type").and_then(|v| v.as_str()).map_or(
            FirewallAction::Block,
            |a| match a {
                "ALLOW" => FirewallAction::Allow,
                "REJECT" => FirewallAction::Reject,
                _ => FirewallAction::Block,
            },
        );

        #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
        let index = p
            .extra
            .get("index")
            .and_then(serde_json::Value::as_i64)
            .map(|i| i as i32);

        let source_endpoint =
            convert_policy_endpoint(p.source.as_ref(), p.extra.get("sourceFirewallZoneId"));
        let destination_endpoint = convert_dest_policy_endpoint(
            p.destination.as_ref(),
            p.extra.get("destinationFirewallZoneId"),
        );

        let source_summary = source_endpoint.filter.as_ref().map(TrafficFilter::summary);
        let destination_summary = destination_endpoint
            .filter
            .as_ref()
            .map(TrafficFilter::summary);

        let ip_version = p
            .ip_protocol_scope
            .as_ref()
            .and_then(|v| v.get("ipVersion"))
            .and_then(|v| v.as_str())
            .map_or(crate::model::firewall::IpVersion::Both, |s| match s {
                "IPV4_ONLY" | "IPV4" => crate::model::firewall::IpVersion::Ipv4,
                "IPV6_ONLY" | "IPV6" => crate::model::firewall::IpVersion::Ipv6,
                _ => crate::model::firewall::IpVersion::Both,
            });

        let ipsec_mode = p
            .extra
            .get("ipsecFilter")
            .and_then(|v| v.as_str())
            .map(String::from);

        let connection_states = p
            .extra
            .get("connectionStateFilter")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        FirewallPolicy {
            id: EntityId::Uuid(p.id),
            name: p.name,
            description: p.description,
            enabled: p.enabled,
            index,
            action,
            ip_version,
            source: source_endpoint,
            destination: destination_endpoint,
            source_summary,
            destination_summary,
            protocol_summary: None,
            schedule: None,
            ipsec_mode,
            connection_states,
            logging_enabled: p.logging_enabled,
            origin: p.metadata.as_ref().and_then(origin_from_metadata),
            data_source: DataSource::IntegrationApi,
        }
    }
}

fn convert_policy_endpoint(
    endpoint: Option<&integration_types::FirewallPolicySource>,
    flat_zone_id: Option<&serde_json::Value>,
) -> PolicyEndpoint {
    if let Some(ep) = endpoint {
        PolicyEndpoint {
            zone_id: ep.zone_id.map(EntityId::Uuid),
            filter: ep
                .traffic_filter
                .as_ref()
                .map(convert_source_traffic_filter),
        }
    } else {
        let zone_id = flat_zone_id
            .and_then(|v| v.as_str())
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .map(EntityId::Uuid);
        PolicyEndpoint {
            zone_id,
            filter: None,
        }
    }
}

fn convert_dest_policy_endpoint(
    endpoint: Option<&integration_types::FirewallPolicyDestination>,
    flat_zone_id: Option<&serde_json::Value>,
) -> PolicyEndpoint {
    if let Some(ep) = endpoint {
        PolicyEndpoint {
            zone_id: ep.zone_id.map(EntityId::Uuid),
            filter: ep.traffic_filter.as_ref().map(convert_dest_traffic_filter),
        }
    } else {
        let zone_id = flat_zone_id
            .and_then(|v| v.as_str())
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .map(EntityId::Uuid);
        PolicyEndpoint {
            zone_id,
            filter: None,
        }
    }
}

fn convert_source_traffic_filter(f: &integration_types::SourceTrafficFilter) -> TrafficFilter {
    use integration_types::SourceTrafficFilter as S;
    match f {
        S::Network {
            network_filter,
            mac_address_filter,
            port_filter,
        } => TrafficFilter::Network {
            network_ids: network_filter
                .network_ids
                .iter()
                .copied()
                .map(EntityId::Uuid)
                .collect(),
            match_opposite: network_filter.match_opposite,
            mac_addresses: mac_address_filter
                .as_ref()
                .map(|m| m.mac_addresses.clone())
                .unwrap_or_default(),
            ports: port_filter.as_ref().map(convert_port_filter),
        },
        S::IpAddress {
            ip_address_filter,
            mac_address_filter,
            port_filter,
        } => TrafficFilter::IpAddress {
            addresses: convert_ip_address_filter(ip_address_filter),
            match_opposite: ip_filter_match_opposite(ip_address_filter),
            mac_addresses: mac_address_filter
                .as_ref()
                .map(|m| m.mac_addresses.clone())
                .unwrap_or_default(),
            ports: port_filter.as_ref().map(convert_port_filter),
        },
        S::MacAddress {
            mac_address_filter,
            port_filter,
        } => TrafficFilter::MacAddress {
            mac_addresses: mac_address_filter.mac_addresses.clone(),
            ports: port_filter.as_ref().map(convert_port_filter),
        },
        S::Port { port_filter } => TrafficFilter::Port {
            ports: convert_port_filter(port_filter),
        },
        S::Region {
            region_filter,
            port_filter,
        } => TrafficFilter::Region {
            regions: region_filter.regions.clone(),
            ports: port_filter.as_ref().map(convert_port_filter),
        },
        S::Unknown => TrafficFilter::Other {
            raw_type: "UNKNOWN".into(),
        },
    }
}

fn convert_dest_traffic_filter(f: &integration_types::DestTrafficFilter) -> TrafficFilter {
    use integration_types::DestTrafficFilter as D;
    match f {
        D::Network {
            network_filter,
            port_filter,
        } => TrafficFilter::Network {
            network_ids: network_filter
                .network_ids
                .iter()
                .copied()
                .map(EntityId::Uuid)
                .collect(),
            match_opposite: network_filter.match_opposite,
            mac_addresses: Vec::new(),
            ports: port_filter.as_ref().map(convert_port_filter),
        },
        D::IpAddress {
            ip_address_filter,
            port_filter,
        } => TrafficFilter::IpAddress {
            addresses: convert_ip_address_filter(ip_address_filter),
            match_opposite: ip_filter_match_opposite(ip_address_filter),
            mac_addresses: Vec::new(),
            ports: port_filter.as_ref().map(convert_port_filter),
        },
        D::Port { port_filter } => TrafficFilter::Port {
            ports: convert_port_filter(port_filter),
        },
        D::Region {
            region_filter,
            port_filter,
        } => TrafficFilter::Region {
            regions: region_filter.regions.clone(),
            ports: port_filter.as_ref().map(convert_port_filter),
        },
        D::Application {
            application_filter,
            port_filter,
        } => TrafficFilter::Application {
            application_ids: application_filter.application_ids.clone(),
            ports: port_filter.as_ref().map(convert_port_filter),
        },
        D::ApplicationCategory {
            application_category_filter,
            port_filter,
        } => TrafficFilter::ApplicationCategory {
            category_ids: application_category_filter.application_category_ids.clone(),
            ports: port_filter.as_ref().map(convert_port_filter),
        },
        D::Domain {
            domain_filter,
            port_filter,
        } => {
            let domains = match domain_filter {
                integration_types::DomainFilter::Specific { domains } => domains.clone(),
                integration_types::DomainFilter::Unknown => Vec::new(),
            };
            TrafficFilter::Domain {
                domains,
                ports: port_filter.as_ref().map(convert_port_filter),
            }
        }
        D::Unknown => TrafficFilter::Other {
            raw_type: "UNKNOWN".into(),
        },
    }
}

fn convert_port_filter(pf: &integration_types::PortFilter) -> PortSpec {
    match pf {
        integration_types::PortFilter::Ports {
            items,
            match_opposite,
        } => PortSpec::Values {
            items: items
                .iter()
                .map(|item| match item {
                    integration_types::PortItem::Number { value } => value.clone(),
                    integration_types::PortItem::Range {
                        start_port,
                        end_port,
                    } => format!("{start_port}-{end_port}"),
                    integration_types::PortItem::Unknown => "?".into(),
                })
                .collect(),
            match_opposite: *match_opposite,
        },
        integration_types::PortFilter::TrafficMatchingList {
            traffic_matching_list_id,
            match_opposite,
        } => PortSpec::MatchingList {
            list_id: EntityId::Uuid(*traffic_matching_list_id),
            match_opposite: *match_opposite,
        },
        integration_types::PortFilter::Unknown => PortSpec::Values {
            items: Vec::new(),
            match_opposite: false,
        },
    }
}

fn convert_ip_address_filter(f: &integration_types::IpAddressFilter) -> Vec<IpSpec> {
    match f {
        integration_types::IpAddressFilter::Specific { items, .. } => items
            .iter()
            .map(|item| match item {
                integration_types::IpAddressItem::Address { value } => IpSpec::Address {
                    value: value.clone(),
                },
                integration_types::IpAddressItem::Range { start, stop } => IpSpec::Range {
                    start: start.clone(),
                    stop: stop.clone(),
                },
                integration_types::IpAddressItem::Subnet { value } => IpSpec::Subnet {
                    value: value.clone(),
                },
            })
            .collect(),
        integration_types::IpAddressFilter::TrafficMatchingList {
            traffic_matching_list_id,
            ..
        } => vec![IpSpec::MatchingList {
            list_id: EntityId::Uuid(*traffic_matching_list_id),
        }],
        integration_types::IpAddressFilter::Unknown => Vec::new(),
    }
}

fn ip_filter_match_opposite(f: &integration_types::IpAddressFilter) -> bool {
    match f {
        integration_types::IpAddressFilter::Specific { match_opposite, .. }
        | integration_types::IpAddressFilter::TrafficMatchingList { match_opposite, .. } => {
            *match_opposite
        }
        integration_types::IpAddressFilter::Unknown => false,
    }
}

// ── Firewall Zone ────────────────────────────────────────────────

impl From<integration_types::FirewallZoneResponse> for FirewallZone {
    fn from(z: integration_types::FirewallZoneResponse) -> Self {
        FirewallZone {
            id: EntityId::Uuid(z.id),
            name: z.name,
            network_ids: z.network_ids.into_iter().map(EntityId::Uuid).collect(),
            origin: origin_from_metadata(&z.metadata),
            source: DataSource::IntegrationApi,
        }
    }
}

// ── ACL Rule ─────────────────────────────────────────────────────

impl From<integration_types::AclRuleResponse> for AclRule {
    fn from(r: integration_types::AclRuleResponse) -> Self {
        let rule_type = match r.rule_type.as_str() {
            "MAC" => AclRuleType::Mac,
            _ => AclRuleType::Ipv4,
        };

        let action = match r.action.as_str() {
            "ALLOW" => AclAction::Allow,
            _ => AclAction::Block,
        };

        AclRule {
            id: EntityId::Uuid(r.id),
            name: r.name,
            enabled: r.enabled,
            rule_type,
            action,
            source_summary: None,
            destination_summary: None,
            origin: origin_from_metadata(&r.metadata),
            source: DataSource::IntegrationApi,
        }
    }
}

// ── Firewall Group ──────────────────────────────────────────────

/// Parse a firewall group from a `rest/firewallgroup` Session API response.
pub fn firewall_group_from_session(v: &serde_json::Value) -> Option<FirewallGroup> {
    let id_str = v.get("_id").and_then(|v| v.as_str())?;
    let name = v.get("name").and_then(|v| v.as_str())?.to_owned();
    let group_type_str = v
        .get("group_type")
        .and_then(|v| v.as_str())
        .unwrap_or("port-group");
    let group_type = match group_type_str {
        "address-group" => FirewallGroupType::AddressGroup,
        "ipv6-address-group" => FirewallGroupType::Ipv6AddressGroup,
        _ => FirewallGroupType::PortGroup,
    };
    let group_members = v
        .get("group_members")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let external_id = v
        .get("external_id")
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned);

    Some(FirewallGroup {
        id: EntityId::from(id_str.to_owned()),
        external_id,
        name,
        group_type,
        group_members,
        source: DataSource::SessionApi,
    })
}
