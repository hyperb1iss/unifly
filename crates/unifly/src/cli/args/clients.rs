use clap::{Args, Subcommand};

use super::ListArgs;

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
        /// Client name, hostname, ID, IP, or MAC address
        client: String,
    },

    /// Show a client's roam timeline (connects, disconnects, AP transitions)
    Roams {
        /// Client name, hostname, ID, IP, or MAC address
        client: String,

        /// Maximum number of events to return
        #[arg(long, default_value = "50")]
        limit: u32,
    },

    /// Show Wi-Fi experience metrics for a wireless client
    #[command(alias = "wifi-experience", alias = "wifiman")]
    Wifi {
        /// Client name, hostname, ID, IP, or MAC address
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

    /// Block a client from connecting (session API)
    Block {
        /// Client MAC address
        mac: String,
    },

    /// Unblock a previously blocked client (session API)
    Unblock {
        /// Client MAC address
        mac: String,
    },

    /// Disconnect/reconnect a wireless client (session API)
    Kick {
        /// Client MAC address
        mac: String,
    },

    /// Forget a client from controller history (session API)
    Forget {
        /// Client MAC address
        mac: String,
    },

    /// List all DHCP reservations (session API)
    #[command(alias = "res")]
    Reservations(ListArgs),

    /// Set a fixed IP (DHCP reservation) for a client (session API)
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

    /// Remove a fixed IP (DHCP reservation) from a client (session API)
    #[command(alias = "unreserve")]
    RemoveIp {
        /// Client MAC address
        mac: String,

        /// Network name or ID (removes from all networks if omitted)
        #[arg(long)]
        network: Option<String>,
    },
}
