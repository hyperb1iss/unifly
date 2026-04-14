// ── Unified domain model ──
//
// Every type in this module is the canonical representation of a UniFi
// entity. They merge data from both the Integration API and the Session
// API into a single clean interface that consumers (CLI/TUI) depend on.

pub mod common;
pub mod entity_id;

pub mod client;
pub mod device;
pub mod dns;
pub mod event;
pub mod firewall;
pub mod hotspot;
pub mod network;
pub mod session_resources;
pub mod site;
pub mod supporting;
pub mod wifi;

// ── Re-exports ──────────────────────────────────────────────────────
// Flat access: `use unifly_core::model::*` gives you everything.

// Core identity
pub use entity_id::{EntityId, MacAddress};

// Common building blocks
pub use common::{Bandwidth, DataSource, EntityOrigin, Throughput};

// Site
pub use site::Site;

// Device
pub use device::{
    Device, DeviceState, DeviceStats, DeviceType, PoeInfo, PoeMode, Port, PortConnector, PortMode,
    PortProfile, PortSpeedSetting, PortState, Radio, StpState,
};

// Client
pub use client::{Client, ClientType, GuestAuth, WirelessInfo};

// Network
pub use network::{DhcpConfig, Ipv6Mode, Network, NetworkManagement, NetworkPurpose};

// WiFi
pub use wifi::{WifiBroadcast, WifiBroadcastType, WifiSecurityMode};

// Firewall
pub use firewall::{
    AclAction, AclRule, AclRuleType, FirewallAction, FirewallGroup, FirewallGroupType,
    FirewallPolicy, FirewallZone, IpSpec, IpVersion, NatPolicy, NatType, PolicyEndpoint, PortSpec,
    TrafficFilter,
};

// DNS
pub use dns::{DnsPolicy, DnsPolicyType};

// Hotspot
pub use hotspot::Voucher;

// Events
pub use event::{Alarm, Event, EventCategory, EventSeverity};

// Supporting types
pub use supporting::{
    DeviceTag, IpsecSa, MagicSiteToSiteVpnConfig, RadiusProfile, RemoteAccessVpnServer,
    SiteToSiteVpn, TrafficMatchingList, VpnClientConnection, VpnClientProfile, VpnServer,
    VpnSetting, VpnTunnel, WanInterface, WireGuardPeer,
};

// Session-only resources
pub use session_resources::{
    Admin, Backup, Country, DeviceStatsSample, DpiApplication, DpiCategory, HealthSummary,
    SiteStatsSample, StatEntry, StatReport, StatsInterval, SysInfo, SystemInfo,
};
