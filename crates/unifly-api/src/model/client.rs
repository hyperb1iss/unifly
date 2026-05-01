// ── Client domain types ──

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};

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
