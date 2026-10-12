use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

use super::ListArgs;

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
        /// Traffic list ID (UUID) or name
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
