use clap::{Args, Subcommand};

use super::ListArgs;

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

    /// Upgrade device firmware (session API)
    Upgrade {
        /// Device MAC address
        device: String,

        /// External firmware URL (optional)
        #[arg(long)]
        url: Option<String>,
    },

    /// Force re-provision device configuration (session API)
    Provision {
        /// Device MAC address
        device: String,
    },

    /// Run WAN speed test (session API, gateway only)
    Speedtest,

    /// List device tags
    Tags(ListArgs),

    /// List switch ports with VLAN configuration (session API)
    Ports {
        /// Device ID (UUID) or MAC address
        device: String,
    },

    /// Configure a switch port (session API)
    PortSet {
        /// Device ID (UUID) or MAC address
        device: String,

        /// Port index to configure (1-based)
        #[arg(value_name = "PORT_IDX")]
        port: u32,

        /// Operational mode
        #[arg(long, value_enum)]
        mode: Option<PortModeArg>,

        /// Native (untagged) VLAN: network name or session _id
        #[arg(long, value_name = "NETWORK")]
        native_vlan: Option<String>,

        /// Comma-separated list of tagged VLAN networks (names or session _ids)
        #[arg(long, value_name = "NETWORK,...", value_delimiter = ',')]
        tagged_vlans: Option<Vec<String>>,

        /// User-facing port label
        #[arg(long)]
        name: Option<String>,

        /// PoE mode for this port
        #[arg(long, value_enum)]
        poe: Option<PoeArg>,

        /// Configured link speed
        #[arg(long, value_enum)]
        speed: Option<SpeedArg>,
    },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
#[value(rename_all = "lower")]
pub enum PortModeArg {
    Access,
    Trunk,
    Mirror,
}

/// PoE mode for a switch port. UniFi PoE has no separate "on" mode --
/// `auto` IS the on/negotiate mode -- so the CLI exposes only the four
/// distinct wire values to keep `port-set --poe X` round-trippable
/// against `devices ports` output.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
#[value(rename_all = "lower")]
pub enum PoeArg {
    Off,
    Auto,
    Pasv24,
    Passthrough,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
#[value(rename_all = "lower")]
pub enum SpeedArg {
    Auto,
    #[value(name = "10")]
    Mbps10,
    #[value(name = "100")]
    Mbps100,
    #[value(name = "1000")]
    Mbps1000,
    #[value(name = "2500")]
    Mbps2500,
    #[value(name = "5000")]
    Mbps5000,
    #[value(name = "10000")]
    Mbps10000,
}
