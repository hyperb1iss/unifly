use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

use super::ListArgs;

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
        /// Network ID (UUID) or name
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

        /// DNS server override (can be repeated)
        #[arg(long = "dns")]
        dns_servers: Option<Vec<String>>,

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
