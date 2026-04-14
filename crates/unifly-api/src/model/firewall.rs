// ── Firewall domain types ──

use serde::{Deserialize, Serialize};

use super::common::{DataSource, EntityOrigin};
use super::entity_id::EntityId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FirewallAction {
    Allow,
    Block,
    Reject,
}

impl<'de> Deserialize<'de> for FirewallAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.to_lowercase().as_str() {
            "allow" => Ok(Self::Allow),
            "block" => Ok(Self::Block),
            "reject" => Ok(Self::Reject),
            _ => Err(serde::de::Error::unknown_variant(
                &s,
                &["allow", "block", "reject"],
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IpVersion {
    Ipv4,
    Ipv6,
    Both,
}

/// Firewall Zone -- container for networks, policies operate between zones.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallZone {
    pub id: EntityId,
    pub name: String,
    pub network_ids: Vec<EntityId>,
    pub origin: Option<EntityOrigin>,

    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) source: DataSource,
}

// ── Traffic filter types ─────────────────────────────────────────

/// Source endpoint with zone and optional traffic filter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyEndpoint {
    pub zone_id: Option<EntityId>,
    pub filter: Option<TrafficFilter>,
}

/// Traffic filter applied to a source or destination.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TrafficFilter {
    Network {
        network_ids: Vec<EntityId>,
        match_opposite: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        mac_addresses: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ports: Option<PortSpec>,
    },
    IpAddress {
        addresses: Vec<IpSpec>,
        match_opposite: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        mac_addresses: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ports: Option<PortSpec>,
    },
    MacAddress {
        mac_addresses: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ports: Option<PortSpec>,
    },
    Port {
        ports: PortSpec,
    },
    Region {
        regions: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ports: Option<PortSpec>,
    },
    Application {
        application_ids: Vec<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ports: Option<PortSpec>,
    },
    ApplicationCategory {
        category_ids: Vec<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ports: Option<PortSpec>,
    },
    Domain {
        domains: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ports: Option<PortSpec>,
    },
    /// Catch-all for filter types not yet modeled.
    Other {
        raw_type: String,
    },
}

/// Port specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PortSpec {
    Values {
        items: Vec<String>,
        match_opposite: bool,
    },
    MatchingList {
        list_id: EntityId,
        match_opposite: bool,
    },
}

/// IP address specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IpSpec {
    Address { value: String },
    Range { start: String, stop: String },
    Subnet { value: String },
    MatchingList { list_id: EntityId },
}

impl TrafficFilter {
    /// Human-readable summary for table display.
    pub fn summary(&self) -> String {
        match self {
            Self::Network {
                network_ids,
                match_opposite,
                ..
            } => {
                let prefix = if *match_opposite { "NOT " } else { "" };
                format!("{prefix}net({} networks)", network_ids.len())
            }
            Self::IpAddress {
                addresses,
                match_opposite,
                ..
            } => {
                let prefix = if *match_opposite { "NOT " } else { "" };
                let items: Vec<String> = addresses
                    .iter()
                    .map(|a| match a {
                        IpSpec::Address { value } | IpSpec::Subnet { value } => value.clone(),
                        IpSpec::Range { start, stop } => format!("{start}-{stop}"),
                        IpSpec::MatchingList { list_id } => format!("list:{list_id}"),
                    })
                    .collect();
                let display = if items.len() <= 2 {
                    items.join(", ")
                } else {
                    format!("{}, {} +{} more", items[0], items[1], items.len() - 2)
                };
                format!("{prefix}ip({display})")
            }
            Self::MacAddress { mac_addresses, .. } => {
                format!("mac({})", mac_addresses.len())
            }
            Self::Port { ports } => summarize_ports(ports),
            Self::Region { regions, .. } => format!("region({})", regions.join(",")),
            Self::Application {
                application_ids, ..
            } => {
                format!("app({} apps)", application_ids.len())
            }
            Self::ApplicationCategory { category_ids, .. } => {
                format!("cat({} categories)", category_ids.len())
            }
            Self::Domain { domains, .. } => {
                if domains.len() <= 2 {
                    format!("domain({})", domains.join(", "))
                } else {
                    format!("domain({} +{} more)", domains[0], domains.len() - 1)
                }
            }
            Self::Other { raw_type } => format!("({raw_type})"),
        }
    }
}

fn summarize_ports(spec: &PortSpec) -> String {
    match spec {
        PortSpec::Values {
            items,
            match_opposite,
        } => {
            let prefix = if *match_opposite { "NOT " } else { "" };
            format!("{prefix}port({})", items.join(","))
        }
        PortSpec::MatchingList {
            list_id,
            match_opposite,
        } => {
            let prefix = if *match_opposite { "NOT " } else { "" };
            format!("{prefix}port(list:{list_id})")
        }
    }
}

/// Firewall Policy -- a rule between two zones.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallPolicy {
    pub id: EntityId,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub index: Option<i32>,

    pub action: FirewallAction,
    pub ip_version: IpVersion,

    // Structured source/destination with traffic filters
    pub source: PolicyEndpoint,
    pub destination: PolicyEndpoint,

    // Human-readable summaries (computed from filters)
    pub source_summary: Option<String>,
    pub destination_summary: Option<String>,

    // Protocol and schedule display fields
    pub protocol_summary: Option<String>,
    pub schedule: Option<String>,
    pub ipsec_mode: Option<String>,

    pub connection_states: Vec<String>,
    pub logging_enabled: bool,

    pub origin: Option<EntityOrigin>,

    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) data_source: DataSource,
}

// ── NAT Policy types ────────────────────────────────────────────────

/// NAT policy type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NatType {
    Masquerade,
    Source,
    Destination,
}

/// NAT Policy -- masquerade, source NAT, or destination NAT rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatPolicy {
    pub id: EntityId,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub nat_type: NatType,
    pub interface_id: Option<EntityId>,
    pub protocol: Option<String>,
    pub src_address: Option<String>,
    pub src_port: Option<String>,
    pub dst_address: Option<String>,
    pub dst_port: Option<String>,
    pub translated_address: Option<String>,
    pub translated_port: Option<String>,
    pub origin: Option<EntityOrigin>,

    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) data_source: DataSource,
}

// ── Firewall Group types ───────────────────────────────────────────

/// Type of firewall group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FirewallGroupType {
    PortGroup,
    AddressGroup,
    Ipv6AddressGroup,
}

impl std::fmt::Display for FirewallGroupType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PortGroup => write!(f, "port-group"),
            Self::AddressGroup => write!(f, "address-group"),
            Self::Ipv6AddressGroup => write!(f, "ipv6-address-group"),
        }
    }
}

/// Firewall Group -- port group, address group, or IPv6 address group
/// managed via the legacy Session API `rest/firewallgroup`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallGroup {
    pub id: EntityId,
    pub external_id: Option<String>,
    pub name: String,
    pub group_type: FirewallGroupType,
    pub group_members: Vec<String>,

    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) source: DataSource,
}

/// ACL Rule action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AclAction {
    Allow,
    Block,
}

/// ACL Rule type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AclRuleType {
    Ipv4,
    Mac,
}

/// ACL Rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AclRule {
    pub id: EntityId,
    pub name: String,
    pub enabled: bool,
    pub rule_type: AclRuleType,
    pub action: AclAction,
    pub source_summary: Option<String>,
    pub destination_summary: Option<String>,
    pub origin: Option<EntityOrigin>,

    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) source: DataSource,
}
