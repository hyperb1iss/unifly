// ── Device domain types ──

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

use super::common::{Bandwidth, DataSource, EntityOrigin};
use super::entity_id::{EntityId, MacAddress};

/// Canonical device type -- normalized from both API surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DeviceType {
    Gateway,
    Switch,
    AccessPoint,
    Other,
}

/// Device operational state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DeviceState {
    Online,
    Offline,
    PendingAdoption,
    Updating,
    GettingReady,
    Adopting,
    Deleting,
    ConnectionInterrupted,
    Isolated,
    Unknown,
}

impl DeviceState {
    pub fn is_online(&self) -> bool {
        matches!(self, Self::Online)
    }

    pub fn is_transitional(&self) -> bool {
        matches!(
            self,
            Self::Updating | Self::GettingReady | Self::Adopting | Self::PendingAdoption
        )
    }
}

/// Port on a switch or gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Port {
    pub index: u32,
    pub name: Option<String>,
    pub state: PortState,
    pub speed_mbps: Option<u32>,
    pub max_speed_mbps: Option<u32>,
    pub connector: Option<PortConnector>,
    pub poe: Option<PoeInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortState {
    Up,
    Down,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortConnector {
    Rj45,
    Sfp,
    SfpPlus,
    Sfp28,
    Qsfp28,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoeInfo {
    pub standard: Option<String>,
    pub enabled: bool,
    pub state: PortState,
}

/// High-level operational mode of a switch port's VLAN profile.
///
/// Derived from the Session API's `port_table` / `port_overrides` fields
/// (`tagged_vlan_mgmt`, `op_mode`) into a single normalized value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortMode {
    /// Single untagged VLAN (no tagged VLANs allowed).
    Access,
    /// Untagged native VLAN plus one or more tagged VLANs.
    Trunk,
    /// Port mirrors another port (SPAN/RSPAN).
    Mirror,
    /// Mode could not be determined from the available data.
    Unknown,
}

/// Spanning-Tree Protocol state for a port, as reported by the switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StpState {
    Disabled,
    Blocking,
    Listening,
    Learning,
    Forwarding,
    Broken,
    Unknown,
}

/// PoE operating mode for a switch port (configuration, not live state).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PoeMode {
    /// Automatic negotiation (802.3af/at/bt).
    Auto,
    /// PoE explicitly disabled on this port.
    Off,
    /// Passive 24V (legacy).
    Passive24V,
    /// PoE passthrough (for specific switches).
    Passthrough,
    /// Unknown or vendor-specific mode.
    Other,
}

/// Configured auto-negotiation / link speed for a port.
///
/// `Auto` means negotiate; other variants pin the link to a fixed speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortSpeedSetting {
    Auto,
    Mbps10,
    Mbps100,
    Mbps1000,
    Mbps2500,
    Mbps5000,
    Mbps10000,
}

impl PortSpeedSetting {
    /// Numeric link speed in Mbps, or `None` for `Auto`.
    ///
    /// Useful for comparing a configured pinned speed against the live
    /// negotiated speed without round-tripping through strings.
    pub fn as_mbps(self) -> Option<u32> {
        match self {
            Self::Auto => None,
            Self::Mbps10 => Some(10),
            Self::Mbps100 => Some(100),
            Self::Mbps1000 => Some(1000),
            Self::Mbps2500 => Some(2500),
            Self::Mbps5000 => Some(5000),
            Self::Mbps10000 => Some(10000),
        }
    }
}

/// VLAN and physical profile for a switch port, merged from the Session API's
/// `port_table` (live state) and `port_overrides` (user configuration).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortProfile {
    /// 1-based port index as shown in the UniFi UI.
    pub index: u32,
    /// User-configured port label (from overrides) or auto-generated name.
    pub name: Option<String>,
    /// Link state (up/down/unknown).
    pub link_state: PortState,
    /// Operational mode (access / trunk / mirror / unknown).
    pub mode: PortMode,
    /// Session `_id` of the native (untagged) network, if any.
    pub native_network_id: Option<String>,
    /// Resolved VLAN id of the native network, if known.
    pub native_vlan_id: Option<u16>,
    /// Resolved display name of the native network, if known.
    pub native_network_name: Option<String>,
    /// Session `_id`s of explicitly tagged networks.
    pub tagged_network_ids: Vec<String>,
    /// Resolved VLAN ids of explicitly tagged networks (best-effort).
    pub tagged_vlan_ids: Vec<u16>,
    /// Resolved display names of explicitly tagged networks (best-effort).
    pub tagged_network_names: Vec<String>,
    /// Whether the trunk carries all tagged VLANs (UniFi "tagged_vlan_mgmt=auto").
    pub tagged_all: bool,
    /// Configured PoE mode, if the port supports PoE.
    pub poe_mode: Option<PoeMode>,
    /// Configured link speed setting.
    pub speed_setting: Option<PortSpeedSetting>,
    /// Current negotiated link speed in Mbps, from live state.
    pub link_speed_mbps: Option<u32>,
    /// STP state reported by the switch.
    pub stp_state: StpState,
    /// Reference to a named port profile (portconf), if one is applied.
    pub port_profile_id: Option<String>,
}

/// Radio on an access point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Radio {
    pub frequency_ghz: f32,
    pub channel: Option<u32>,
    pub channel_width_mhz: Option<u32>,
    pub wlan_standard: Option<String>,
    pub tx_retries_pct: Option<f64>,
    pub channel_utilization_pct: Option<f64>,
}

/// Real-time device statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceStats {
    pub uptime_secs: Option<u64>,
    pub cpu_utilization_pct: Option<f64>,
    pub memory_utilization_pct: Option<f64>,
    pub load_average_1m: Option<f64>,
    pub load_average_5m: Option<f64>,
    pub load_average_15m: Option<f64>,
    pub uplink_bandwidth: Option<Bandwidth>,
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub next_heartbeat: Option<DateTime<Utc>>,
}

/// The canonical Device type. Merges data from Integration + Session API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct Device {
    pub id: EntityId,
    pub mac: MacAddress,
    pub ip: Option<IpAddr>,
    pub wan_ipv6: Option<String>,
    pub name: Option<String>,
    pub model: Option<String>,
    pub device_type: DeviceType,
    pub state: DeviceState,

    // Firmware
    pub firmware_version: Option<String>,
    pub firmware_updatable: bool,

    // Lifecycle
    pub adopted_at: Option<DateTime<Utc>>,
    pub provisioned_at: Option<DateTime<Utc>>,
    pub last_seen: Option<DateTime<Utc>>,

    // Hardware
    pub serial: Option<String>,
    pub supported: bool,

    // Interfaces
    pub ports: Vec<Port>,
    pub radios: Vec<Radio>,

    // Uplink
    pub uplink_device_id: Option<EntityId>,
    pub uplink_device_mac: Option<MacAddress>,
    /// 1-based switch port index this device is uplinked through, if the
    /// uplink is wired and the upstream switch reported the remote port.
    /// `None` for wireless uplinks and root devices.
    pub uplink_port_idx: Option<u32>,

    // Features (from Integration API)
    pub has_switching: bool,
    pub has_access_point: bool,

    // Real-time stats (populated from statistics endpoint or WebSocket)
    pub stats: DeviceStats,

    // Client count (if known)
    pub client_count: Option<u32>,

    // Metadata
    pub origin: Option<EntityOrigin>,

    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) source: DataSource,
    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) updated_at: DateTime<Utc>,
}
