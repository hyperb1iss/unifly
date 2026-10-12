// ── Network domain types ──

use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};

use super::common::{DataSource, EntityOrigin};
use super::entity_id::EntityId;

/// Network management type (from Integration API taxonomy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkManagement {
    Gateway,
    Switch,
    Unmanaged,
}

/// Session network purpose (from Session API).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkPurpose {
    Corporate,
    Guest,
    Wan,
    VlanOnly,
}

/// IPv6 configuration mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Ipv6Mode {
    PrefixDelegation,
    Static,
}

impl std::fmt::Display for Ipv6Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PrefixDelegation => f.write_str("Prefix Delegation"),
            Self::Static => f.write_str("Static"),
        }
    }
}

/// DHCP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhcpConfig {
    pub enabled: bool,
    pub range_start: Option<Ipv4Addr>,
    pub range_stop: Option<Ipv4Addr>,
    pub lease_time_secs: Option<u64>,
    pub dns_servers: Vec<IpAddr>,
    pub gateway: Option<Ipv4Addr>,
}

/// The canonical Network type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct Network {
    pub id: EntityId,
    pub name: String,
    pub enabled: bool,

    // Classification
    pub management: Option<NetworkManagement>,
    pub purpose: Option<NetworkPurpose>,
    pub is_default: bool,

    // VLAN
    pub vlan_id: Option<u16>,

    // IPv4
    pub subnet: Option<String>,
    pub gateway_ip: Option<Ipv4Addr>,

    // DHCP
    pub dhcp: Option<DhcpConfig>,

    // IPv6
    pub ipv6_enabled: bool,
    pub ipv6_mode: Option<Ipv6Mode>,
    pub ipv6_prefix: Option<String>,
    pub dhcpv6_enabled: bool,
    pub slaac_enabled: bool,

    // Advanced DHCP / Boot
    pub ntp_server: Option<IpAddr>,
    pub pxe_enabled: bool,
    pub tftp_server: Option<String>,

    // Firewall zone association
    pub firewall_zone_id: Option<EntityId>,

    // Flags
    pub isolation_enabled: bool,
    pub internet_access_enabled: bool,
    pub mdns_forwarding_enabled: bool,
    pub cellular_backup_enabled: bool,

    pub origin: Option<EntityOrigin>,

    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) source: DataSource,
}
