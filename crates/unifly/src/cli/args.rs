//! Clap derive structures for the `unifly` CLI.
//!
//! Defines the complete command tree, global flags, and shared types.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

// ── Top-Level CLI ────────────────────────────────────────────────────

/// unifly -- kubectl-style CLI for UniFi network management
#[derive(Debug, Parser)]
#[command(
    name = "unifly",
    version,
    about = "Manage UniFi networks from the command line",
    long_about = "A powerful CLI for administering UniFi network controllers.\n\n\
        Uses the official Integration API (v10.1.84) as primary interface,\n\
        with legacy API fallback for features not yet in the official spec.",
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

// ── Global Options ───────────────────────────────────────────────────

#[derive(Debug, Args)]
pub struct GlobalOpts {
    /// Controller profile to use
    #[arg(long, short = 'p', env = "UNIFI_PROFILE", global = true)]
    pub profile: Option<String>,

    /// Controller URL (overrides profile)
    #[arg(long, short = 'c', env = "UNIFI_URL", global = true)]
    pub controller: Option<String>,

    /// Site name or UUID
    #[arg(long, short = 's', env = "UNIFI_SITE", global = true)]
    pub site: Option<String>,

    /// Integration API key
    #[arg(long, env = "UNIFI_API_KEY", global = true, hide_env = true)]
    pub api_key: Option<String>,

    /// TOTP token for MFA-enabled controllers
    #[arg(long, env = "UNIFI_TOTP", global = true, hide_env = true)]
    pub totp: Option<String>,

    /// Output format
    #[arg(
        long,
        short = 'o',
        env = "UNIFI_OUTPUT",
        default_value = "table",
        global = true
    )]
    pub output: OutputFormat,

    /// When to use color output
    #[arg(long, default_value = "auto", global = true)]
    pub color: ColorMode,

    /// Increase verbosity (-v, -vv, -vvv)
    #[arg(long, short = 'v', action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Suppress non-error output
    #[arg(long, short = 'q', global = true)]
    pub quiet: bool,

    /// Skip confirmation prompts
    #[arg(long, short = 'y', global = true)]
    pub yes: bool,

    /// Accept self-signed TLS certificates
    #[arg(long, short = 'k', env = "UNIFI_INSECURE", global = true)]
    pub insecure: bool,

    /// Request timeout in seconds
    #[arg(long, env = "UNIFI_TIMEOUT", default_value = "30", global = true)]
    pub timeout: u64,
}

// ── Output & Color Enums ─────────────────────────────────────────────

#[derive(Debug, Clone, ValueEnum)]
pub enum OutputFormat {
    /// Pretty table (default, interactive)
    Table,
    /// Pretty-printed JSON
    Json,
    /// Compact single-line JSON
    JsonCompact,
    /// YAML
    Yaml,
    /// Plain text, one value per line (scripting)
    Plain,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum ColorMode {
    /// Auto-detect (color if terminal is interactive)
    Auto,
    /// Always emit color codes
    Always,
    /// Never emit color codes
    Never,
}

// ── Top-Level Command Enum ───────────────────────────────────────────

#[derive(Debug, Subcommand)]
pub enum Command {
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

    /// Manage networks and VLANs
    #[command(alias = "net", alias = "n")]
    Networks(NetworksArgs),

    /// View RADIUS profiles
    Radius(RadiusArgs),

    /// Manage sites
    Sites(SitesArgs),

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

    /// View VPN servers and tunnels
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

// ── TUI Arguments ───────────────────────────────────────────────────

/// Arguments for the `tui` subcommand (real-time terminal dashboard).
#[cfg(feature = "tui")]
#[derive(Debug, Args)]
pub struct TuiArgs {
    /// Theme name (e.g., nord, dracula, silkcircuit-neon)
    #[arg(long, env = "UNIFLY_THEME")]
    pub theme: Option<String>,

    /// Log file path
    #[arg(long, default_value = "/tmp/unifly-tui.log")]
    pub log_file: std::path::PathBuf,
}

// ── Shared List Arguments ────────────────────────────────────────────

/// Shared pagination and filtering arguments for all list commands.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Max results per page (1-200)
    #[arg(long, short = 'l', default_value = "25")]
    pub limit: u32,

    /// Pagination offset
    #[arg(long, default_value = "0")]
    pub offset: u32,

    /// Fetch all pages automatically
    #[arg(long, short = 'a')]
    pub all: bool,

    /// Filter expression (Integration API syntax)
    /// Examples: "name.eq('MyNetwork')", "state.in('ONLINE','OFFLINE')"
    #[arg(long, short = 'f')]
    pub filter: Option<String>,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  DEVICES
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct DevicesArgs {
    #[command(subcommand)]
    pub command: DevicesCommand,
}

#[derive(Debug, Subcommand)]
pub enum DevicesCommand {
    /// List adopted devices
    #[command(alias = "ls")]
    List(ListArgs),

    /// Get adopted device details
    Get {
        /// Device ID (UUID) or MAC address
        device: String,
    },

    /// Adopt a pending device
    Adopt {
        /// MAC address of the device to adopt
        #[arg(value_name = "MAC")]
        mac: String,

        /// Ignore device limit on the site
        #[arg(long)]
        ignore_limit: bool,
    },

    /// Remove (unadopt) a device
    Remove {
        /// Device ID (UUID) or MAC address
        device: String,
    },

    /// Restart a device
    Restart {
        /// Device ID (UUID) or MAC address
        device: String,
    },

    /// Toggle locate LED (blink to identify device)
    Locate {
        /// Device MAC address
        device: String,

        /// Turn locate on (default) or off
        #[arg(long, default_value = "true", action = clap::ArgAction::Set)]
        on: bool,
    },

    /// Power-cycle a PoE port
    PortCycle {
        /// Device ID (UUID) or MAC address
        device: String,

        /// Port index to power-cycle
        #[arg(value_name = "PORT_IDX")]
        port: u32,
    },

    /// Get real-time device statistics
    Stats {
        /// Device ID (UUID) or MAC address
        device: String,
    },

    /// List devices pending adoption
    Pending(ListArgs),

    /// Upgrade device firmware (legacy API)
    Upgrade {
        /// Device MAC address
        device: String,

        /// External firmware URL (optional)
        #[arg(long)]
        url: Option<String>,
    },

    /// Force re-provision device configuration (legacy API)
    Provision {
        /// Device MAC address
        device: String,
    },

    /// Run WAN speed test (legacy API, gateway only)
    Speedtest,

    /// List device tags
    Tags(ListArgs),
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  CLIENTS
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct ClientsArgs {
    #[command(subcommand)]
    pub command: ClientsCommand,
}

#[derive(Debug, Subcommand)]
pub enum ClientsCommand {
    /// List connected clients
    #[command(alias = "ls")]
    List(ListArgs),

    /// Find clients by IP, name, hostname, or MAC (case-insensitive substring match)
    #[command(alias = "search")]
    Find {
        /// Search query (matches against IP, name, hostname, MAC)
        query: String,
    },

    /// Get connected client details
    Get {
        /// Client ID (UUID) or MAC address
        client: String,
    },

    /// Authorize guest access
    Authorize {
        /// Client ID (UUID)
        client: String,

        /// Authorization duration in minutes
        #[arg(long, required = true)]
        minutes: u32,

        /// Data usage limit in MB
        #[arg(long)]
        data_limit_mb: Option<u64>,

        /// Download rate limit in Kbps
        #[arg(long)]
        rx_limit_kbps: Option<u64>,

        /// Upload rate limit in Kbps
        #[arg(long)]
        tx_limit_kbps: Option<u64>,
    },

    /// Revoke guest access
    Unauthorize {
        /// Client ID (UUID)
        client: String,
    },

    /// Block a client from connecting (legacy API)
    Block {
        /// Client MAC address
        mac: String,
    },

    /// Unblock a previously blocked client (legacy API)
    Unblock {
        /// Client MAC address
        mac: String,
    },

    /// Disconnect/reconnect a wireless client (legacy API)
    Kick {
        /// Client MAC address
        mac: String,
    },

    /// Forget a client from controller history (legacy API)
    Forget {
        /// Client MAC address
        mac: String,
    },

    /// Set a fixed IP (DHCP reservation) for a client (legacy API)
    #[command(alias = "reserve")]
    SetIp {
        /// Client MAC address
        mac: String,

        /// IPv4 address to reserve
        #[arg(long)]
        ip: String,

        /// Network name or ID (auto-detected from IP if omitted)
        #[arg(long)]
        network: Option<String>,
    },

    /// Remove a fixed IP (DHCP reservation) from a client (legacy API)
    #[command(alias = "unreserve")]
    RemoveIp {
        /// Client MAC address
        mac: String,
    },
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  NETWORKS
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct NetworksArgs {
    #[command(subcommand)]
    pub command: NetworksCommand,
}

#[derive(Debug, Subcommand)]
pub enum NetworksCommand {
    /// List all networks
    #[command(alias = "ls")]
    List(ListArgs),

    /// Get network details
    Get {
        /// Network ID (UUID)
        id: String,
    },

    /// Create a new network
    Create {
        /// Network name
        #[arg(long, required_unless_present = "from_file")]
        name: Option<String>,

        /// Management type: gateway, switch, or unmanaged
        #[arg(long, required_unless_present = "from_file", value_enum)]
        management: Option<NetworkManagement>,

        /// VLAN ID (1-4009)
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..=4009))]
        vlan: Option<u16>,

        /// Enable the network (default: true)
        #[arg(long, default_value = "true", action = clap::ArgAction::Set)]
        enabled: bool,

        /// IPv4 host address with prefix (e.g., 192.168.1.1/24)
        #[arg(long)]
        ipv4_host: Option<String>,

        /// Enable DHCP server
        #[arg(long)]
        dhcp: bool,

        /// DHCP range start
        #[arg(long)]
        dhcp_start: Option<String>,

        /// DHCP range end
        #[arg(long)]
        dhcp_stop: Option<String>,

        /// DHCP lease time in seconds
        #[arg(long)]
        dhcp_lease: Option<u32>,

        /// Firewall zone ID to assign
        #[arg(long)]
        zone: Option<String>,

        /// Enable network isolation
        #[arg(long)]
        isolated: bool,

        /// Enable internet access (gateway managed only)
        #[arg(long, default_value = "true", action = clap::ArgAction::Set)]
        internet: bool,

        /// Create from JSON file (overrides individual flags)
        #[arg(long, short = 'F', conflicts_with_all = &["name", "management"])]
        from_file: Option<PathBuf>,
    },

    /// Update an existing network
    Update {
        /// Network ID (UUID)
        id: String,

        /// Load full update payload from JSON file
        #[arg(long, short = 'F')]
        from_file: Option<PathBuf>,

        /// Network name
        #[arg(long)]
        name: Option<String>,

        /// Enable/disable the network
        #[arg(long, action = clap::ArgAction::Set)]
        enabled: Option<bool>,

        /// VLAN ID (1-4009)
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..=4009))]
        vlan: Option<u16>,
    },

    /// Delete a network
    Delete {
        /// Network ID (UUID)
        id: String,

        /// Force delete even if referenced
        #[arg(long)]
        force: bool,
    },

    /// Show network cross-references (what uses this network)
    Refs {
        /// Network ID (UUID)
        id: String,
    },
}

#[derive(Debug, Clone, ValueEnum)]
pub enum NetworkManagement {
    /// Gateway-managed network (full IP/DHCP/NAT)
    Gateway,
    /// Switch-managed (L3 switch) network
    Switch,
    /// Unmanaged (VLAN only, no IP management)
    Unmanaged,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  WIFI
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct WifiArgs {
    #[command(subcommand)]
    pub command: WifiCommand,
}

#[derive(Debug, Subcommand)]
pub enum WifiCommand {
    /// List WiFi broadcasts
    #[command(alias = "ls")]
    List(ListArgs),

    /// Get WiFi broadcast details
    Get {
        /// WiFi broadcast ID (UUID)
        id: String,
    },

    /// Create a WiFi broadcast
    Create {
        /// SSID name
        #[arg(long, required_unless_present = "from_file")]
        name: Option<String>,

        /// Broadcast type
        #[arg(long, default_value = "standard", value_enum)]
        broadcast_type: WifiBroadcastType,

        /// Network to associate (UUID or 'native')
        #[arg(long, required_unless_present = "from_file")]
        network: Option<String>,

        /// Security mode
        #[arg(long, default_value = "wpa2-personal", value_enum)]
        security: WifiSecurity,

        /// WPA passphrase (8-63 characters)
        #[arg(long)]
        passphrase: Option<String>,

        /// Broadcasting frequencies (2.4, 5, 6 GHz)
        #[arg(long, value_delimiter = ',')]
        frequencies: Option<Vec<f32>>,

        /// Hide SSID name
        #[arg(long)]
        hidden: bool,

        /// Enable band steering (standard type only)
        #[arg(long)]
        band_steering: bool,

        /// Enable fast roaming
        #[arg(long)]
        fast_roaming: bool,

        /// Create from JSON file
        #[arg(long, short = 'F', conflicts_with_all = &["name", "network"])]
        from_file: Option<PathBuf>,
    },

    /// Update a WiFi broadcast
    Update {
        /// WiFi broadcast ID (UUID)
        id: String,

        /// Load full payload from JSON file
        #[arg(long, short = 'F')]
        from_file: Option<PathBuf>,

        /// Update SSID name
        #[arg(long)]
        name: Option<String>,

        /// Update passphrase
        #[arg(long)]
        passphrase: Option<String>,

        /// Enable/disable
        #[arg(long, action = clap::ArgAction::Set)]
        enabled: Option<bool>,
    },

    /// Delete a WiFi broadcast
    Delete {
        /// WiFi broadcast ID (UUID)
        id: String,

        /// Force delete even if referenced
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Clone, ValueEnum)]
pub enum WifiBroadcastType {
    /// Full-featured WiFi with band steering, MLO, hotspot
    Standard,
    /// Simplified IoT-focused WiFi
    IotOptimized,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum WifiSecurity {
    Open,
    Wpa2Personal,
    Wpa3Personal,
    Wpa2Wpa3Personal,
    Wpa2Enterprise,
    Wpa3Enterprise,
    Wpa2Wpa3Enterprise,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  FIREWALL
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct FirewallArgs {
    #[command(subcommand)]
    pub command: FirewallCommand,
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum FirewallCommand {
    /// Manage firewall policies
    Policies(FirewallPoliciesArgs),

    /// Manage firewall zones
    Zones(FirewallZonesArgs),
}

// --- Firewall Policies ---

#[derive(Debug, Args)]
pub struct FirewallPoliciesArgs {
    #[command(subcommand)]
    pub command: FirewallPoliciesCommand,
}

#[derive(Debug, Subcommand)]
pub enum FirewallPoliciesCommand {
    /// List all firewall policies
    #[command(alias = "ls")]
    List(ListArgs),

    /// Get a specific firewall policy
    Get {
        /// Firewall policy ID (UUID)
        id: String,
    },

    /// Create a firewall policy
    Create {
        /// Policy name
        #[arg(long, required_unless_present = "from_file")]
        name: Option<String>,

        /// Action: allow, block, or reject
        #[arg(long, required_unless_present = "from_file", value_enum)]
        action: Option<FirewallAction>,

        /// Source zone ID (UUID)
        #[arg(long, required_unless_present = "from_file")]
        source_zone: Option<String>,

        /// Destination zone ID (UUID)
        #[arg(long, required_unless_present = "from_file")]
        dest_zone: Option<String>,

        /// Enable the policy (default: true)
        #[arg(long, default_value = "true", action = clap::ArgAction::Set)]
        enabled: bool,

        /// Policy description
        #[arg(long)]
        description: Option<String>,

        /// Enable logging for matched traffic
        #[arg(long)]
        logging: bool,

        /// Source network IDs or names (comma-separated)
        #[arg(long, value_delimiter = ',')]
        src_network: Option<Vec<String>>,

        /// Source IP addresses (IPs, CIDRs, or ranges like "10.0.0.1-10.0.0.100")
        #[arg(long, value_delimiter = ',')]
        src_ip: Option<Vec<String>>,

        /// Source ports (single ports or ranges like "8000-9000")
        #[arg(long, value_delimiter = ',')]
        src_port: Option<Vec<String>>,

        /// Destination network IDs or names (comma-separated)
        #[arg(long, value_delimiter = ',')]
        dst_network: Option<Vec<String>>,

        /// Destination IP addresses (IPs, CIDRs, or ranges)
        #[arg(long, value_delimiter = ',')]
        dst_ip: Option<Vec<String>>,

        /// Destination ports (single ports or ranges)
        #[arg(long, value_delimiter = ',')]
        dst_port: Option<Vec<String>>,

        /// Connection states to match (comma-separated: NEW, ESTABLISHED, RELATED, INVALID)
        #[arg(long, value_delimiter = ',')]
        states: Option<Vec<String>>,

        /// IP version: IPV4_ONLY, IPV6_ONLY, IPV4_AND_IPV6
        #[arg(long)]
        ip_version: Option<String>,

        /// Create from JSON file (complex policies)
        #[arg(long, short = 'F', conflicts_with_all = &["name", "action", "source_zone", "dest_zone"])]
        from_file: Option<PathBuf>,
    },

    /// Update a firewall policy
    Update {
        /// Firewall policy ID (UUID)
        id: String,

        /// Source network IDs or names (comma-separated)
        #[arg(long, value_delimiter = ',')]
        src_network: Option<Vec<String>>,

        /// Source IP addresses (IPs, CIDRs, or ranges)
        #[arg(long, value_delimiter = ',')]
        src_ip: Option<Vec<String>>,

        /// Source ports
        #[arg(long, value_delimiter = ',')]
        src_port: Option<Vec<String>>,

        /// Destination network IDs or names (comma-separated)
        #[arg(long, value_delimiter = ',')]
        dst_network: Option<Vec<String>>,

        /// Destination IP addresses (IPs, CIDRs, or ranges)
        #[arg(long, value_delimiter = ',')]
        dst_ip: Option<Vec<String>>,

        /// Destination ports
        #[arg(long, value_delimiter = ',')]
        dst_port: Option<Vec<String>>,

        /// Connection states to match
        #[arg(long, value_delimiter = ',')]
        states: Option<Vec<String>>,

        /// IP version: IPV4_ONLY, IPV6_ONLY, IPV4_AND_IPV6
        #[arg(long)]
        ip_version: Option<String>,

        /// Load full payload from JSON file
        #[arg(long, short = 'F')]
        from_file: Option<PathBuf>,
    },

    /// Patch a firewall policy (quick toggle enabled/logging)
    Patch {
        /// Firewall policy ID (UUID)
        id: String,

        /// Enable or disable the policy
        #[arg(long, action = clap::ArgAction::Set)]
        enabled: Option<bool>,

        /// Enable or disable logging for matched traffic
        #[arg(long, action = clap::ArgAction::Set)]
        logging: Option<bool>,
    },

    /// Delete a firewall policy
    Delete {
        /// Firewall policy ID (UUID)
        id: String,
    },

    /// Get or set policy ordering between zones
    Reorder {
        /// Source zone ID (UUID)
        #[arg(long, required = true)]
        source_zone: String,

        /// Destination zone ID (UUID)
        #[arg(long, required = true)]
        dest_zone: String,

        /// Get current ordering (default if --set not provided)
        #[arg(long, conflicts_with = "set")]
        get: bool,

        /// Set ordering from comma-separated policy IDs
        #[arg(long, value_delimiter = ',')]
        set: Option<Vec<String>>,
    },
}

#[derive(Debug, Clone, ValueEnum)]
pub enum FirewallAction {
    Allow,
    Block,
    Reject,
}

// --- Firewall Zones ---

#[derive(Debug, Args)]
pub struct FirewallZonesArgs {
    #[command(subcommand)]
    pub command: FirewallZonesCommand,
}

#[derive(Debug, Subcommand)]
pub enum FirewallZonesCommand {
    /// List all firewall zones
    #[command(alias = "ls")]
    List(ListArgs),

    /// Get a specific firewall zone
    Get {
        /// Zone ID (UUID)
        id: String,
    },

    /// Create a custom firewall zone
    Create {
        /// Zone name
        #[arg(long, required = true)]
        name: String,

        /// Network IDs to attach (comma-separated UUIDs)
        #[arg(long, value_delimiter = ',')]
        networks: Option<Vec<String>>,
    },

    /// Update a firewall zone
    Update {
        /// Zone ID (UUID)
        id: String,

        /// Zone name
        #[arg(long)]
        name: Option<String>,

        /// Network IDs to attach (replaces existing)
        #[arg(long, value_delimiter = ',')]
        networks: Option<Vec<String>>,
    },

    /// Delete a custom firewall zone
    Delete {
        /// Zone ID (UUID)
        id: String,
    },
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  ACL
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct AclArgs {
    #[command(subcommand)]
    pub command: AclCommand,
}

#[derive(Debug, Subcommand)]
pub enum AclCommand {
    /// List ACL rules
    #[command(alias = "ls")]
    List(ListArgs),

    /// Get an ACL rule
    Get {
        /// ACL rule ID (UUID)
        id: String,
    },

    /// Create an ACL rule
    Create {
        /// Rule name
        #[arg(long, required_unless_present = "from_file")]
        name: Option<String>,

        /// Rule type: ipv4 or mac
        #[arg(long, required_unless_present = "from_file", value_enum)]
        rule_type: Option<AclRuleType>,

        /// Action: allow or block
        #[arg(long, required_unless_present = "from_file", value_enum)]
        action: Option<AclAction>,

        /// Source zone ID (UUID)
        #[arg(long, required_unless_present = "from_file")]
        source_zone: Option<String>,

        /// Destination zone ID (UUID)
        #[arg(long, required_unless_present = "from_file")]
        dest_zone: Option<String>,

        /// Optional IP protocol filter (e.g. TCP, UDP, ICMP)
        #[arg(long)]
        protocol: Option<String>,

        /// Optional source port/range (e.g. 80 or 1000-2000)
        #[arg(long)]
        source_port: Option<String>,

        /// Optional destination port/range (e.g. 443 or 3000-4000)
        #[arg(long)]
        destination_port: Option<String>,

        /// Create from JSON file
        #[arg(long, short = 'F', conflicts_with_all = &["name", "rule_type", "source_zone", "dest_zone", "protocol", "source_port", "destination_port"])]
        from_file: Option<PathBuf>,
    },

    /// Update an ACL rule
    Update {
        /// ACL rule ID (UUID)
        id: String,

        /// Load full payload from JSON file
        #[arg(long, short = 'F')]
        from_file: Option<PathBuf>,
    },

    /// Delete an ACL rule
    Delete {
        /// ACL rule ID (UUID)
        id: String,
    },

    /// Get or set ACL rule ordering
    Reorder {
        /// Get current ordering
        #[arg(long, conflicts_with = "set")]
        get: bool,

        /// Set ordering from comma-separated rule IDs
        #[arg(long, value_delimiter = ',')]
        set: Option<Vec<String>>,
    },
}

#[derive(Debug, Clone, ValueEnum)]
pub enum AclRuleType {
    /// IP-based ACL rule (IPv4 with protocol filters)
    Ipv4,
    /// MAC address-based ACL rule
    Mac,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum AclAction {
    Allow,
    Block,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  DNS
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct DnsArgs {
    #[command(subcommand)]
    pub command: DnsCommand,
}

#[derive(Debug, Subcommand)]
pub enum DnsCommand {
    /// List DNS policies
    #[command(alias = "ls")]
    List(ListArgs),

    /// Get a DNS policy
    Get {
        /// DNS policy ID (UUID)
        id: String,
    },

    /// Create a DNS policy
    Create {
        /// Record type
        #[arg(long, required_unless_present = "from_file", value_enum)]
        record_type: Option<DnsRecordType>,

        /// Domain name
        #[arg(long, required_unless_present = "from_file")]
        domain: Option<String>,

        /// Target value (IP address, target domain, mail server, etc.)
        #[arg(long, required_unless_present = "from_file")]
        value: Option<String>,

        /// TTL in seconds (0-86400)
        #[arg(long, default_value = "3600", value_parser = clap::value_parser!(u32).range(0..=86400))]
        ttl: u32,

        /// MX priority (MX records only)
        #[arg(long)]
        priority: Option<u16>,

        /// Create from JSON file
        #[arg(long, short = 'F', conflicts_with_all = &["record_type", "domain"])]
        from_file: Option<PathBuf>,
    },

    /// Update a DNS policy
    Update {
        /// DNS policy ID (UUID)
        id: String,

        /// Load full payload from JSON file
        #[arg(long, short = 'F')]
        from_file: Option<PathBuf>,
    },

    /// Delete a DNS policy
    Delete {
        /// DNS policy ID (UUID)
        id: String,
    },
}

#[derive(Debug, Clone, ValueEnum)]
pub enum DnsRecordType {
    A,
    Aaaa,
    Cname,
    Mx,
    Txt,
    Srv,
    Forward,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  TRAFFIC MATCHING LISTS
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct TrafficListsArgs {
    #[command(subcommand)]
    pub command: TrafficListsCommand,
}

#[derive(Debug, Subcommand)]
pub enum TrafficListsCommand {
    /// List traffic matching lists
    #[command(alias = "ls")]
    List(ListArgs),

    /// Get a traffic matching list
    Get {
        /// Traffic list ID (UUID)
        id: String,
    },

    /// Create a traffic matching list
    Create {
        /// List name
        #[arg(long, required_unless_present = "from_file")]
        name: Option<String>,

        /// List type
        #[arg(long, required_unless_present = "from_file", value_enum)]
        list_type: Option<TrafficListType>,

        /// Items (comma-separated ports, IPs, or subnets)
        #[arg(long, value_delimiter = ',', required_unless_present = "from_file")]
        items: Option<Vec<String>>,

        /// Create from JSON file
        #[arg(long, short = 'F', conflicts_with_all = &["name", "list_type"])]
        from_file: Option<PathBuf>,
    },

    /// Update a traffic matching list
    Update {
        /// Traffic list ID (UUID)
        id: String,

        /// Load full payload from JSON file
        #[arg(long, short = 'F')]
        from_file: Option<PathBuf>,
    },

    /// Delete a traffic matching list
    Delete {
        /// Traffic list ID (UUID)
        id: String,
    },
}

#[derive(Debug, Clone, ValueEnum)]
pub enum TrafficListType {
    /// Port list
    Ports,
    /// IPv4 address/subnet list
    Ipv4,
    /// IPv6 address/subnet list
    Ipv6,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  HOTSPOT
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct HotspotArgs {
    #[command(subcommand)]
    pub command: HotspotCommand,
}

#[derive(Debug, Subcommand)]
pub enum HotspotCommand {
    /// List vouchers
    #[command(alias = "ls")]
    List {
        /// Max results (1-1000)
        #[arg(long, short = 'l', default_value = "100")]
        limit: u32,

        /// Pagination offset
        #[arg(long, default_value = "0")]
        offset: u32,
    },

    /// Get voucher details
    Get {
        /// Voucher ID (UUID)
        id: String,
    },

    /// Generate new vouchers
    Create {
        /// Voucher name/label
        #[arg(long, required = true)]
        name: String,

        /// Number of vouchers to generate
        #[arg(long, default_value = "1")]
        count: u32,

        /// Time limit in minutes
        #[arg(long, required = true)]
        minutes: u32,

        /// Max guests per voucher
        #[arg(long)]
        guest_limit: Option<u32>,

        /// Data usage limit in MB
        #[arg(long)]
        data_limit_mb: Option<u64>,

        /// Download rate limit in Kbps
        #[arg(long)]
        rx_limit_kbps: Option<u64>,

        /// Upload rate limit in Kbps
        #[arg(long)]
        tx_limit_kbps: Option<u64>,
    },

    /// Delete a single voucher
    Delete {
        /// Voucher ID (UUID)
        id: String,
    },

    /// Bulk delete vouchers by filter
    Purge {
        /// Filter expression (required)
        #[arg(long, required = true)]
        filter: String,
    },
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  VPN
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct VpnArgs {
    #[command(subcommand)]
    pub command: VpnCommand,
}

#[derive(Debug, Subcommand)]
pub enum VpnCommand {
    /// List VPN servers
    Servers(ListArgs),

    /// List site-to-site VPN tunnels
    Tunnels(ListArgs),
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  SITES
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct SitesArgs {
    #[command(subcommand)]
    pub command: SitesCommand,
}

#[derive(Debug, Subcommand)]
pub enum SitesCommand {
    /// List local sites
    #[command(alias = "ls")]
    List(ListArgs),

    /// Create a new site (legacy API)
    Create {
        /// Site name (internal reference)
        #[arg(long, required = true)]
        name: String,

        /// Site description (display name)
        #[arg(long, required = true)]
        description: String,
    },

    /// Delete a site (legacy API)
    Delete {
        /// Site name
        name: String,
    },
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  EVENTS
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct EventsArgs {
    #[command(subcommand)]
    pub command: EventsCommand,
}

#[derive(Debug, Subcommand)]
pub enum EventsCommand {
    /// List recent events (legacy API)
    #[command(alias = "ls")]
    List {
        /// Max results
        #[arg(long, short = 'l', default_value = "100")]
        limit: u32,

        /// Hours of history to include
        #[arg(long, default_value = "24")]
        within: u32,
    },

    /// Stream real-time events via WebSocket (legacy API)
    Watch {
        /// Event types to filter (comma-separated)
        #[arg(long, value_delimiter = ',')]
        types: Option<Vec<String>>,
    },
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  ALARMS
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct AlarmsArgs {
    #[command(subcommand)]
    pub command: AlarmsCommand,
}

#[derive(Debug, Subcommand)]
pub enum AlarmsCommand {
    /// List alarms (legacy API)
    #[command(alias = "ls")]
    List {
        /// Only show unarchived alarms
        #[arg(long)]
        unarchived: bool,

        /// Max results
        #[arg(long, short = 'l', default_value = "100")]
        limit: u32,
    },

    /// Archive a single alarm (legacy API)
    Archive {
        /// Alarm ID
        id: String,
    },

    /// Archive all alarms (legacy API)
    ArchiveAll,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  STATS
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct StatsArgs {
    #[command(subcommand)]
    pub command: StatsCommand,
}

#[derive(Debug, Subcommand)]
pub enum StatsCommand {
    /// Site-level statistics (legacy API)
    Site(StatsQuery),

    /// Per-device statistics (legacy API)
    Device(StatsQuery),

    /// Per-client statistics (legacy API)
    Client(StatsQuery),

    /// Gateway statistics (legacy API)
    Gateway(StatsQuery),

    /// DPI traffic analysis (legacy API)
    Dpi {
        /// Analysis type: by-app or by-cat
        #[arg(long, default_value = "by-app", value_enum)]
        group_by: DpiGroupBy,

        /// Filter by MAC addresses (comma-separated)
        #[arg(long, value_delimiter = ',')]
        macs: Option<Vec<String>>,
    },
}

#[derive(Debug, Args)]
pub struct StatsQuery {
    /// Reporting interval
    #[arg(long, default_value = "hourly", value_enum)]
    pub interval: StatsInterval,

    /// Start time (ISO 8601 or Unix timestamp)
    #[arg(long)]
    pub start: Option<String>,

    /// End time (ISO 8601 or Unix timestamp)
    #[arg(long)]
    pub end: Option<String>,

    /// Attributes to include (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub attrs: Option<Vec<String>>,

    /// Filter by MAC addresses (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub macs: Option<Vec<String>>,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum StatsInterval {
    #[value(name = "5m")]
    FiveMinutes,
    Hourly,
    Daily,
    Monthly,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum DpiGroupBy {
    ByApp,
    ByCat,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  SYSTEM
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct SystemArgs {
    #[command(subcommand)]
    pub command: SystemCommand,
}

#[derive(Debug, Subcommand)]
pub enum SystemCommand {
    /// Application version info (Integration API)
    Info,

    /// Site health summary (legacy API)
    Health,

    /// Controller system info (legacy API)
    Sysinfo,

    /// Backup management (legacy API)
    Backup(BackupArgs),

    /// Reboot controller hardware (legacy API, UDM only)
    Reboot,

    /// Power off controller hardware (legacy API, UDM only)
    Poweroff,
}

#[derive(Debug, Args)]
pub struct BackupArgs {
    #[command(subcommand)]
    pub command: BackupCommand,
}

#[derive(Debug, Subcommand)]
pub enum BackupCommand {
    /// Create a new backup
    Create,

    /// List existing backups
    #[command(alias = "ls")]
    List,

    /// Download a backup file
    Download {
        /// Backup filename
        filename: String,

        /// Destination path (default: current directory)
        #[arg(long = "path")]
        path: Option<PathBuf>,
    },

    /// Delete a backup
    Delete {
        /// Backup filename
        filename: String,
    },
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  ADMIN
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct AdminArgs {
    #[command(subcommand)]
    pub command: AdminCommand,
}

#[derive(Debug, Subcommand)]
pub enum AdminCommand {
    /// List site administrators (legacy API)
    #[command(alias = "ls")]
    List,

    /// Invite a new administrator (legacy API)
    Invite {
        /// Admin name
        #[arg(long, required = true)]
        name: String,

        /// Admin email
        #[arg(long, required = true)]
        email: String,

        /// Role: admin or readonly
        #[arg(long, default_value = "admin")]
        role: String,
    },

    /// Remove administrator access (legacy API)
    Revoke {
        /// Admin ID
        admin: String,
    },

    /// Update administrator role (legacy API)
    Update {
        /// Admin ID
        admin: String,

        /// New role
        #[arg(long, required = true)]
        role: String,
    },
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  DPI
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct DpiArgs {
    #[command(subcommand)]
    pub command: DpiCommand,
}

#[derive(Debug, Subcommand)]
pub enum DpiCommand {
    /// List DPI applications
    Apps(ListArgs),

    /// List DPI categories
    Categories(ListArgs),
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  RADIUS
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct RadiusArgs {
    #[command(subcommand)]
    pub command: RadiusCommand,
}

#[derive(Debug, Subcommand)]
pub enum RadiusCommand {
    /// List RADIUS profiles
    Profiles(ListArgs),
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  WANS
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct WansArgs {
    #[command(subcommand)]
    pub command: WansCommand,
}

#[derive(Debug, Subcommand)]
pub enum WansCommand {
    /// List WAN interfaces
    #[command(alias = "ls")]
    List(ListArgs),
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  CONFIG
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Create initial config file with guided setup
    Init,

    /// Display current resolved configuration
    Show,

    /// Set a configuration value
    Set {
        /// Config key (dot-separated path, e.g., "profiles.home.controller")
        key: String,

        /// Value to set
        value: String,
    },

    /// List configured profiles
    Profiles,

    /// Set the default profile
    Use {
        /// Profile name to set as default
        name: String,
    },

    /// Store a password in the system keyring
    SetPassword {
        /// Profile name
        #[arg(long)]
        profile: Option<String>,
    },
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  COMPLETIONS
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Args)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    pub shell: clap_complete::Shell,
}
