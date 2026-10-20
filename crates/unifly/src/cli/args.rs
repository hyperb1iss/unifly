//! Clap derive structures for the `unifly` CLI.
//!
//! This module is now a thin shell that re-exports resource-specific
//! clap definitions from focused submodules.

use clap::{Parser, Subcommand};

#[path = "args/acl.rs"]
mod acl;
#[path = "args/admin.rs"]
mod admin;
#[path = "args/alarms.rs"]
mod alarms;
#[path = "args/api.rs"]
mod api;
#[path = "args/clients.rs"]
mod clients;
#[path = "args/cloud.rs"]
mod cloud;
#[path = "args/common.rs"]
mod common;
#[path = "args/config.rs"]
mod config;
#[path = "args/devices.rs"]
mod devices;
#[path = "args/dns.rs"]
mod dns;
#[path = "args/dpi.rs"]
mod dpi;
#[path = "args/events.rs"]
mod events;
#[path = "args/filter.rs"]
mod filter;
#[path = "args/firewall.rs"]
mod firewall;
#[path = "args/hotspot.rs"]
mod hotspot;
#[path = "args/nat.rs"]
mod nat;
#[path = "args/networks.rs"]
mod networks;
#[path = "args/radius.rs"]
mod radius;
#[path = "args/settings.rs"]
mod settings;
#[path = "args/sites.rs"]
mod sites;
#[path = "args/skill.rs"]
mod skill;
#[path = "args/stats.rs"]
mod stats;
#[path = "args/system.rs"]
mod system;
#[path = "args/traffic_lists.rs"]
mod traffic_lists;
#[path = "args/vpn.rs"]
mod vpn;
#[path = "args/wans.rs"]
mod wans;
#[path = "args/wifi.rs"]
mod wifi;

pub use acl::*;
pub use admin::*;
pub use alarms::*;
pub use api::*;
pub use clients::*;
pub use cloud::*;
pub use common::*;
pub use config::*;
pub use devices::*;
pub use dns::*;
pub use dpi::*;
pub use events::*;
pub use filter::*;
pub use firewall::*;
pub use hotspot::*;
pub use nat::*;
pub use networks::*;
pub use radius::*;
pub use settings::*;
pub use sites::*;
pub use skill::*;
pub use stats::*;
pub use system::*;
pub use traffic_lists::*;
pub use vpn::*;
pub use wans::*;
pub use wifi::*;

/// unifly -- kubectl-style CLI for UniFi network management
#[derive(Debug, Parser)]
#[command(
    name = "unifly",
    version,
    about = "Manage UniFi networks from the command line",
    long_about = "A powerful CLI for administering UniFi network controllers.\n\n\
        Uses the official Integration API (v10.1.84) as primary interface,\n\
        with session API fallback for features not yet in the official spec.",
    propagate_version = true,
    subcommand_required = true,
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalOpts,

    #[command(subcommand)]
    pub command: Command,
}

// ── Top-Level Command Enum ───────────────────────────────────────────

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Send a raw API request (GET, POST, PUT, PATCH, or DELETE)
    Api(ApiArgs),

    /// Manage ACL rules
    Acl(AclArgs),

    /// Administrator management
    Admin(AdminArgs),

    /// Manage alarms
    Alarms(AlarmsArgs),

    /// Manage connected clients
    #[command(alias = "cl")]
    Clients(ClientsArgs),

    /// Generate shell completions
    Completions(CompletionsArgs),

    /// Query the Site Manager cloud fleet API
    Cloud(CloudArgs),

    /// Manage CLI configuration and profiles
    Config(ConfigArgs),

    /// List available country codes
    Countries,

    /// Manage adopted and pending devices
    #[command(alias = "dev", alias = "d")]
    Devices(DevicesArgs),

    /// Manage DNS policies (local DNS records)
    Dns(DnsArgs),

    /// DPI reference data
    Dpi(DpiArgs),

    /// View and stream events
    Events(EventsArgs),

    /// Manage firewall policies and zones
    #[command(alias = "fw")]
    Firewall(FirewallArgs),

    /// Manage hotspot vouchers
    Hotspot(HotspotArgs),

    /// Manage NAT policies (masquerade, source NAT, destination NAT)
    Nat(NatArgs),

    /// Manage networks and VLANs
    #[command(alias = "net", alias = "n")]
    Networks(NetworksArgs),

    /// View RADIUS profiles
    Radius(RadiusArgs),

    /// View and modify site settings (session API)
    Settings(SettingsArgs),

    /// Manage sites
    Sites(SitesArgs),

    /// Print version-matched agent guidance, or install the agent skill, offline
    Skill(SkillArgs),

    /// Query statistics and reports
    Stats(StatsArgs),

    /// System operations and info
    #[command(alias = "sys")]
    System(SystemArgs),

    /// Show network topology (devices, clients, connections)
    #[command(alias = "topo")]
    Topology,

    /// Manage traffic matching lists
    TrafficLists(TrafficListsArgs),

    /// View VPN servers and tunnels, and manage legacy VPN resources
    Vpn(VpnArgs),

    /// View WAN interfaces
    Wans(WansArgs),

    /// Manage WiFi broadcasts (SSIDs)
    #[command(alias = "w")]
    Wifi(WifiArgs),

    /// Launch the real-time terminal dashboard
    #[cfg(feature = "tui")]
    Tui(TuiArgs),
}
