use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct SettingsArgs {
    #[command(subcommand)]
    pub command: SettingsCommand,
}

#[derive(Debug, Subcommand)]
pub enum SettingsCommand {
    /// List all site setting sections (session API)
    #[command(alias = "ls")]
    List {
        /// Include `x_`-prefixed credential fields in JSON/YAML output
        #[arg(long)]
        with_secrets: bool,
    },

    /// Get a specific setting section by key (session API)
    Get {
        /// Setting section key (e.g. "ips", "dpi", "usg", "radio_ai")
        key: String,

        /// Show `x_`-prefixed credential fields instead of hiding them
        #[arg(long)]
        with_secrets: bool,
    },

    /// Update a field in a setting section (session API).
    /// Fields prefixed with `x_` contain credentials and internal secrets.
    Set(SettingsSetArgs),

    /// Export all settings as JSON (session API).
    /// `x_`-prefixed credential fields are left out unless `--with-secrets`
    /// is given.
    Export {
        /// Include `x_`-prefixed credential fields in the export
        #[arg(long)]
        with_secrets: bool,
    },
}

#[derive(Debug, Args)]
pub struct SettingsSetArgs {
    /// Setting section key (e.g. "dpi", "ips", "usg")
    pub key: String,

    /// Field name within the section
    #[arg(required_unless_present = "data", requires = "value")]
    pub field: Option<String>,

    /// Field value (parsed as bool/number/string)
    #[arg(requires = "field")]
    pub value: Option<String>,

    /// Merge a JSON object into the section
    #[arg(long, conflicts_with_all = ["field", "value"])]
    pub data: Option<String>,
}
