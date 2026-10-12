use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

use super::ListArgs;

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
        /// WiFi broadcast ID (UUID) or name
        id: String,
    },

    /// List neighboring / rogue APs detected by your access points
    #[command(alias = "rogueap")]
    Neighbors {
        /// Only show APs seen within this many seconds
        #[arg(long)]
        within: Option<i64>,

        /// Maximum number of results to display
        #[arg(long)]
        limit: Option<usize>,

        /// Show all results (no limit)
        #[arg(long, conflicts_with = "limit")]
        all: bool,
    },

    /// List per-radio regulatory channel availability
    Channels,

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
