use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

use super::ListArgs;

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
        /// DNS policy ID (UUID) or domain
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
