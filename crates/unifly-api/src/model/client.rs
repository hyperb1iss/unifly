// ── Client domain types ──

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use super::common::{Bandwidth, DataSource};
use super::entity_id::{EntityId, MacAddress};

/// Client connection type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ClientType {
    Wired,
    Wireless,
    Vpn,
    Teleport,
    Unknown,
}

/// Guest authorization details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestAuth {
    pub authorized: bool,
    pub method: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub tx_bytes: Option<u64>,
    pub rx_bytes: Option<u64>,
    pub elapsed_minutes: Option<u64>,
}

/// Wireless connection details (only present for wireless clients).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WirelessInfo {
    pub ssid: Option<String>,
    pub bssid: Option<MacAddress>,
    pub channel: Option<u32>,
    pub frequency_ghz: Option<f32>,
    pub signal_dbm: Option<i32>,
    pub noise_dbm: Option<i32>,
    pub satisfaction: Option<u8>,
    pub tx_rate_kbps: Option<u64>,
    pub rx_rate_kbps: Option<u64>,
}

/// The canonical Client type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Client {
    pub id: EntityId,
    pub mac: MacAddress,
    pub ip: Option<IpAddr>,
    /// Every IPv6 address the client holds, widest reach first: global,
    /// then unique local, then link-local.
    #[serde(default)]
    pub ipv6_addresses: Vec<Ipv6Addr>,
    pub name: Option<String>,
    pub hostname: Option<String>,
    pub client_type: ClientType,

    // Connection
    pub connected_at: Option<DateTime<Utc>>,
    pub uplink_device_id: Option<EntityId>,
    pub uplink_device_mac: Option<MacAddress>,
    /// 1-based switch port index for wired clients, matching `devices ports`
    /// indexing. `None` for wireless clients and any wired client where the
    /// controller didn't report `sw_port`.
    pub switch_port: Option<u32>,
    pub network_id: Option<EntityId>,
    /// The network's name as the Session API reports it. Session
    /// `network_id`s are legacy record IDs that never match the Integration
    /// API's network UUIDs, so this name is how a hybrid or session client
    /// is tied back to its network.
    #[serde(default)]
    pub network_name: Option<String>,
    pub vlan: Option<u16>,

    // Wireless-specific
    pub wireless: Option<WirelessInfo>,

    // Guest
    pub guest_auth: Option<GuestAuth>,
    pub is_guest: bool,

    // Traffic
    pub tx_bytes: Option<u64>,
    pub rx_bytes: Option<u64>,
    pub bandwidth: Option<Bandwidth>,

    // Fingerprint (session API)
    pub os_name: Option<String>,
    pub device_class: Option<String>,

    // DHCP reservation (session API)
    pub use_fixedip: bool,
    pub fixed_ip: Option<Ipv4Addr>,

    // Blocking state (session API)
    pub blocked: bool,

    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) source: DataSource,
    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) updated_at: DateTime<Utc>,
}

impl Client {
    /// Every address the client holds: its IPv4 one, then its IPv6 ones.
    pub fn addresses(&self) -> impl Iterator<Item = IpAddr> + '_ {
        self.ip
            .into_iter()
            .chain(self.ipv6_addresses.iter().copied().map(IpAddr::V6))
    }

    /// Whether `address` is one of the client's, IPv4 or IPv6.
    pub fn has_address(&self, address: IpAddr) -> bool {
        self.addresses().any(|own| own == address)
    }

    /// The address to reach the client at: its IPv4 one, else the widest
    /// reaching IPv6 one short of link-local, which needs an interface to
    /// mean anything.
    pub fn primary_address(&self) -> Option<IpAddr> {
        self.ip.or_else(|| {
            self.ipv6_addresses
                .iter()
                .find(|ip| !ip.is_unicast_link_local())
                .copied()
                .map(IpAddr::V6)
        })
    }
}
