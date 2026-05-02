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

        /// Include the wired clients and adopted devices (APs, downstream
        /// switches) currently observed on each port. Adds a `connections`
        /// array in JSON output (with a `kind` discriminator per entry —
        /// `"client"` or `"device"`) and a `Conns` count column
        /// (`<clients>/<devices>`) in the table view.
        #[arg(long)]
        with_clients: bool,
    },

    /// Export switch port configuration as JSONC for `port-set --from-file`
    ///
    /// Sparse by default — only ports with active overrides are emitted.
    /// Pass `--all` to emit every port at its current state (useful for
    /// first-time bootstrapping a config file).
    PortsExport {
        /// Device ID (UUID) or MAC address
        device: String,

        /// Emit every port, including those without an override entry
        #[arg(long)]
        all: bool,

        /// Annotate each port with `// last-seen <ISO8601>: <mac> (<name>, <kind>)`
        /// comments for currently-connected wired clients and adopted
        /// devices. `<kind>` is `client` or `device`. Useful for drift
        /// detection — the comment block records what was observed on
        /// each labelled port at export time. The marker prefix
        /// `// last-seen ` is a stable parse anchor (don't hand-edit
        /// those lines).
        #[arg(long)]
        with_clients: bool,
    },

    /// Configure a switch port (session API)
    PortSet {
        /// Device ID (UUID) or MAC address
        device: String,

        /// Port index to configure (1-based). Required for single-port
        /// updates; mutually exclusive with `--from-file` (which scopes
        /// to the `index` of each entry in the file).
        #[arg(
            value_name = "PORT_IDX",
            required_unless_present = "from_file",
            conflicts_with = "from_file"
        )]
        port: Option<u32>,

        /// Operational mode
        #[arg(long, value_enum, conflicts_with_all = ["from_file", "reset"])]
        mode: Option<PortModeArg>,

        /// Native (untagged) VLAN: network name or session _id
        #[arg(long, value_name = "NETWORK", conflicts_with_all = ["from_file", "reset"])]
        native_vlan: Option<String>,

        /// Comma-separated list of tagged VLAN networks (names or session _ids)
        #[arg(
            long,
            value_name = "NETWORK,...",
            value_delimiter = ',',
            conflicts_with_all = ["from_file", "reset"]
        )]
        tagged_vlans: Option<Vec<String>>,

        /// User-facing port label
        #[arg(long, conflicts_with_all = ["from_file", "reset"])]
        name: Option<String>,

        /// PoE mode for this port
        #[arg(long, value_enum, conflicts_with_all = ["from_file", "reset"])]
        poe: Option<PoeArg>,

        /// Configured link speed
        #[arg(long, value_enum, conflicts_with_all = ["from_file", "reset"])]
        speed: Option<SpeedArg>,

        /// Apply a multi-port configuration from a JSONC file
        /// (`{"ports": [{"index": N, ...}, ...]}`). Splice semantics:
        /// ports not listed are left untouched. Per-port `"reset": true`
        /// removes that port's override entry.
        #[arg(long, short = 'F', value_name = "FILE")]
        from_file: Option<std::path::PathBuf>,

        /// Remove this port's `port_overrides` entry, returning it to
        /// controller defaults. Mutually exclusive with the per-field
        /// flags above. Equivalent to applying
        /// `{"ports": [{"index": <PORT_IDX>, "reset": true}]}` via `-F`.
        #[arg(long, conflicts_with = "from_file")]
        reset: bool,
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
