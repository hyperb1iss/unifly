use std::path::PathBuf;

use clap::{Args, Subcommand};

use super::ListArgs;

#[derive(Debug, Args)]
pub struct NatArgs {
    #[command(subcommand)]
    pub command: NatCommand,
}

#[derive(Debug, Subcommand)]
pub enum NatCommand {
    /// Manage NAT policies
    Policies(NatPoliciesArgs),
}

#[derive(Debug, Args)]
pub struct NatPoliciesArgs {
    #[command(subcommand)]
    pub command: NatPoliciesCommand,
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum NatPoliciesCommand {
    /// List all NAT policies
    #[command(alias = "ls")]
    List(ListArgs),

    /// Get a specific NAT policy
    Get {
        /// NAT policy ID (UUID)
        id: String,
    },

    /// Create a NAT policy
    Create {
        /// Policy name
        #[arg(long, required_unless_present = "from_file")]
        name: Option<String>,

        /// NAT type: masquerade, source, or destination
        #[arg(long = "type", required_unless_present = "from_file")]
        nat_type: Option<String>,

        /// Network/VLAN interface ID (UUID)
        #[arg(long)]
        interface_id: Option<String>,

        /// Protocol: tcp, udp, tcp_udp, or all
        #[arg(long)]
        protocol: Option<String>,

        /// Source IP address or CIDR
        #[arg(long)]
        src_address: Option<String>,

        /// Source port
        #[arg(long)]
        src_port: Option<String>,

        /// Destination IP address or CIDR
        #[arg(long)]
        dst_address: Option<String>,

        /// Destination port
        #[arg(long)]
        dst_port: Option<String>,

        /// Translated (rewritten) IP address
        #[arg(long)]
        translated_address: Option<String>,

        /// Translated (rewritten) port
        #[arg(long)]
        translated_port: Option<String>,

        /// Enable the policy (default: true)
        #[arg(long, default_value = "true", action = clap::ArgAction::Set)]
        enabled: bool,

        /// Policy description
        #[arg(long)]
        description: Option<String>,

        /// Create from JSON/JSONC file
        #[arg(long, short = 'F', conflicts_with_all = &["name", "nat_type"])]
        from_file: Option<PathBuf>,
    },

    /// Update a NAT policy
    Update {
        /// NAT policy ID (UUID or legacy _id)
        id: String,

        /// Policy name (the display label; mutually exclusive with --description)
        #[arg(long, conflicts_with = "description")]
        name: Option<String>,

        /// NAT type: masquerade, source, or destination
        #[arg(long = "type")]
        nat_type: Option<String>,

        /// Network/VLAN interface ID (UUID)
        #[arg(long)]
        interface_id: Option<String>,

        /// Protocol: tcp, udp, tcp_udp, or all
        #[arg(long)]
        protocol: Option<String>,

        /// Source IP address or CIDR
        #[arg(long)]
        src_address: Option<String>,

        /// Source port
        #[arg(long)]
        src_port: Option<String>,

        /// Destination IP address or CIDR
        #[arg(long)]
        dst_address: Option<String>,

        /// Destination port
        #[arg(long)]
        dst_port: Option<String>,

        /// Translated (rewritten) IP address
        #[arg(long)]
        translated_address: Option<String>,

        /// Translated (rewritten) port
        #[arg(long)]
        translated_port: Option<String>,

        /// Enable or disable the policy
        #[arg(long, action = clap::ArgAction::Set)]
        enabled: Option<bool>,

        /// Policy description (the display label; mutually exclusive with --name)
        #[arg(long, conflicts_with = "name")]
        description: Option<String>,

        /// Load full payload from JSON/JSONC file
        #[arg(long, short = 'F', conflicts_with_all = &[
            "name", "nat_type", "interface_id", "protocol",
            "src_address", "src_port", "dst_address", "dst_port",
            "translated_address", "translated_port", "enabled", "description",
        ])]
        from_file: Option<PathBuf>,
    },

    /// Delete a NAT policy
    Delete {
        /// NAT policy ID (UUID)
        id: String,
    },
}
