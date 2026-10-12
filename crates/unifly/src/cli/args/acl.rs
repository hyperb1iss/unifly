use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

use super::ListArgs;

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
        /// ACL rule ID (UUID) or name
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

        /// Rule name
        #[arg(long)]
        name: Option<String>,

        /// Rule type: ipv4 or mac
        #[arg(long, value_enum)]
        rule_type: Option<AclRuleType>,

        /// Action: allow or block
        #[arg(long, value_enum)]
        action: Option<AclAction>,

        /// Source zone ID (UUID)
        #[arg(long)]
        source_zone: Option<String>,

        /// Destination zone ID (UUID)
        #[arg(long)]
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

        /// Enable or disable the rule
        #[arg(long, action = clap::ArgAction::Set)]
        enabled: Option<bool>,

        /// Load full payload from JSON file
        #[arg(long, short = 'F', conflicts_with_all = &["name", "rule_type", "action", "source_zone", "dest_zone", "protocol", "source_port", "destination_port", "enabled"])]
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
