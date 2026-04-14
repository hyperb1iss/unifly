use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

use super::ListArgs;

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

    /// Manage firewall groups (port groups, address groups)
    Groups(FirewallGroupsArgs),
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

        /// Allow return traffic (default: true)
        #[arg(long, default_value = "true", action = clap::ArgAction::Set)]
        allow_return_traffic: bool,

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

        /// Source port group name (resolves to firewall group)
        #[arg(long)]
        src_port_group: Option<String>,

        /// Destination port group name (resolves to firewall group)
        #[arg(long)]
        dst_port_group: Option<String>,

        /// Source address group name (resolves to firewall group)
        #[arg(long)]
        src_address_group: Option<String>,

        /// Destination address group name (resolves to firewall group)
        #[arg(long)]
        dst_address_group: Option<String>,

        /// Connection states to match (comma-separated: NEW, ESTABLISHED, RELATED, INVALID)
        #[arg(long, value_delimiter = ',')]
        states: Option<Vec<String>>,

        /// IP version: IPV4_ONLY, IPV6_ONLY, IPV4_AND_IPV6
        #[arg(long)]
        ip_version: Option<String>,

        /// Create from JSON file (complex policies)
        #[arg(long, short = 'F', conflicts_with_all = &["name", "action", "source_zone", "dest_zone"])]
        from_file: Option<PathBuf>,

        /// Place policy after system-defined rules (index ~40000 instead of ~10000)
        #[arg(long)]
        after_system: bool,
    },

    /// Update a firewall policy
    Update {
        /// Firewall policy ID (UUID)
        id: String,

        /// Allow return traffic
        #[arg(long, action = clap::ArgAction::Set)]
        allow_return_traffic: Option<bool>,

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

        /// Source port group name (resolves to firewall group)
        #[arg(long)]
        src_port_group: Option<String>,

        /// Destination port group name (resolves to firewall group)
        #[arg(long)]
        dst_port_group: Option<String>,

        /// Source address group name (resolves to firewall group)
        #[arg(long)]
        src_address_group: Option<String>,

        /// Destination address group name (resolves to firewall group)
        #[arg(long)]
        dst_address_group: Option<String>,

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

        /// Place policies after system-defined rules (use with --set)
        #[arg(long)]
        after_system: bool,
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
        #[arg(long, required_unless_present = "from_file")]
        name: Option<String>,

        /// Network IDs to attach (comma-separated UUIDs)
        #[arg(long, value_delimiter = ',')]
        networks: Option<Vec<String>>,

        /// Create from JSON file (overrides individual flags)
        #[arg(long, short = 'F', conflicts_with_all = &["name"])]
        from_file: Option<PathBuf>,
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

        /// Load update payload from JSON file
        #[arg(long, short = 'F')]
        from_file: Option<PathBuf>,
    },

    /// Delete a custom firewall zone
    Delete {
        /// Zone ID (UUID)
        id: String,
    },
}

// --- Firewall Groups ---

#[derive(Debug, Args)]
pub struct FirewallGroupsArgs {
    #[command(subcommand)]
    pub command: FirewallGroupsCommand,
}

#[derive(Debug, Clone, ValueEnum)]
#[allow(clippy::enum_variant_names)]
pub enum FirewallGroupTypeArg {
    PortGroup,
    AddressGroup,
    Ipv6AddressGroup,
}

#[derive(Debug, Subcommand)]
pub enum FirewallGroupsCommand {
    /// List all firewall groups
    #[command(alias = "ls")]
    List {
        #[command(flatten)]
        list: ListArgs,

        /// Filter by group type
        #[arg(long, value_enum, rename_all = "kebab-case")]
        r#type: Option<FirewallGroupTypeArg>,
    },

    /// Get a specific firewall group
    Get {
        /// Firewall group ID
        id: String,
    },

    /// Create a firewall group
    Create {
        /// Group name
        #[arg(long, required_unless_present = "from_file")]
        name: Option<String>,

        /// Group type: port-group, address-group, ipv6-address-group
        #[arg(
            long,
            value_enum,
            rename_all = "kebab-case",
            default_value = "port-group"
        )]
        r#type: FirewallGroupTypeArg,

        /// Members (comma-separated ports/ranges or IPs/CIDRs)
        #[arg(long, value_delimiter = ',')]
        members: Option<Vec<String>>,

        /// Create from JSON file
        #[arg(long, short = 'F', conflicts_with_all = &["name"])]
        from_file: Option<PathBuf>,
    },

    /// Update a firewall group
    Update {
        /// Firewall group ID
        id: String,

        /// Group name
        #[arg(long)]
        name: Option<String>,

        /// Members (replaces existing, comma-separated)
        #[arg(long, value_delimiter = ',')]
        members: Option<Vec<String>>,

        /// Load update payload from JSON file
        #[arg(long, short = 'F')]
        from_file: Option<PathBuf>,
    },

    /// Delete a firewall group
    Delete {
        /// Firewall group ID
        id: String,
    },
}
