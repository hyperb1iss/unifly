use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Add or update a local controller profile with guided setup
    Init,

    /// Guided setup for a cloud Site Manager profile
    CloudSetup,

    /// Display the saved configuration, with secrets masked
    Show,

    /// Set a configuration value
    Set {
        /// Config key (dot-separated path, e.g., "profiles.home.controller")
        key: String,

        /// Value to set
        value: String,
    },

    /// List configured profiles
    Profiles,

    /// Set the default profile
    Use {
        /// Profile name to set as default
        name: String,
    },

    /// Show or set the color theme (shared by CLI and TUI)
    Theme {
        /// Theme name to activate (omit to show current + list available)
        name: Option<String>,
    },

    /// Store a password in the system keyring
    SetPassword {
        /// Profile name
        #[arg(value_name = "PROFILE", conflicts_with = "profile_flag")]
        profile: Option<String>,

        /// Profile name
        #[arg(long = "profile", value_name = "PROFILE")]
        profile_flag: Option<String>,
    },
}
