use clap::{Args, Subcommand};

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
        /// Voucher ID (UUID), code, or name
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
