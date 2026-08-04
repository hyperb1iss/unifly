use clap::{Args, ValueEnum};

/// Controller URL, site, auth, and output options shared by all commands.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct GlobalOpts {
    /// Controller profile to use
    #[arg(long, short = 'p', env = "UNIFI_PROFILE", global = true)]
    pub profile: Option<String>,

    /// Controller URL (overrides profile)
    #[arg(long, short = 'c', env = "UNIFI_URL", global = true)]
    pub controller: Option<String>,

    /// Site name or UUID
    #[arg(long, short = 's', env = "UNIFI_SITE", global = true)]
    pub site: Option<String>,

    /// Integration API key
    #[arg(long, env = "UNIFI_API_KEY", global = true, hide_env = true)]
    pub api_key: Option<String>,

    /// Cloud console/host ID (for cloud auth mode)
    #[arg(long, env = "UNIFI_HOST_ID", global = true, hide = true)]
    pub host_id: Option<String>,

    /// Output format
    #[arg(
        long,
        short = 'o',
        env = "UNIFI_OUTPUT",
        default_value = "table",
        global = true
    )]
    pub output: OutputFormat,

    /// When to use color output
    #[arg(long, default_value = "auto", global = true)]
    pub color: ColorMode,

    /// Color theme (e.g., nord, dracula, silkcircuit-neon)
    #[arg(long, env = "UNIFLY_THEME", global = true)]
    pub theme: Option<String>,

    /// Increase verbosity (-v, -vv, -vvv)
    #[arg(long, short = 'v', action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Suppress non-error output
    #[arg(long, short = 'q', global = true)]
    pub quiet: bool,

    /// Skip confirmation prompts
    #[arg(long, short = 'y', global = true)]
    pub yes: bool,

    /// TOTP token for MFA-enabled controllers (prefer UNIFI_TOTP env var)
    #[arg(long, env = "UNIFI_TOTP", global = true, hide = true, hide_env = true)]
    pub totp: Option<String>,

    /// Disable session caching (forces fresh login)
    #[arg(long, global = true)]
    pub no_cache: bool,

    /// Sanitize PII for demo recordings (uses [demo] config section)
    #[arg(long, env = "UNIFI_DEMO", global = true)]
    pub demo: bool,

    /// Accept self-signed TLS certificates
    #[arg(long, short = 'k', env = "UNIFI_INSECURE", global = true)]
    pub insecure: bool,

    /// Request timeout in seconds (default 30, profiles may override)
    #[arg(long, env = "UNIFI_TIMEOUT", global = true)]
    pub timeout: Option<u64>,

    /// Disable tachyonfx animations in the TUI (honours `NO_EFFECTS=1`)
    #[arg(long, global = true)]
    pub no_effects: bool,
}

/// Fallback request timeout when neither `--timeout` nor the profile sets one.
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

impl GlobalOpts {
    /// Resolve the effective timeout: flag/env, then profile, then default.
    #[must_use]
    pub fn timeout_secs(&self, profile_timeout: Option<u64>) -> u64 {
        self.timeout
            .or(profile_timeout)
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
    }
}

#[derive(Debug, Clone, ValueEnum)]
pub enum OutputFormat {
    /// Pretty table (default, interactive)
    Table,
    /// Pretty-printed JSON
    Json,
    /// Compact single-line JSON
    JsonCompact,
    /// YAML
    Yaml,
    /// Plain text, one value per line (scripting)
    Plain,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum ColorMode {
    /// Auto-detect (color if terminal is interactive)
    Auto,
    /// Always emit color codes
    Always,
    /// Never emit color codes
    Never,
}

/// Shared pagination and filtering arguments for list commands.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Max results per page (1-200)
    #[arg(long, short = 'l', default_value = "25")]
    pub limit: u32,

    /// Pagination offset
    #[arg(long, default_value = "0")]
    pub offset: u32,

    /// Fetch all pages automatically
    #[arg(long, short = 'a')]
    pub all: bool,

    /// Filter expression (Integration API syntax)
    /// Examples: "name.eq('MyNetwork')", "state.in('ONLINE','OFFLINE')"
    #[arg(long, short = 'f')]
    pub filter: Option<String>,
}

/// Arguments for the `tui` subcommand (real-time terminal dashboard).
#[cfg(feature = "tui")]
#[derive(Debug, Args)]
pub struct TuiArgs {
    /// Log file path
    #[arg(long, default_value_os_t = default_tui_log_path())]
    pub log_file: std::path::PathBuf,
}

#[cfg(feature = "tui")]
fn default_tui_log_path() -> std::path::PathBuf {
    std::env::temp_dir().join("unifly-tui.log")
}

/// Shell to generate completions for.
#[derive(Debug, Args)]
pub struct CompletionsArgs {
    pub shell: clap_complete::Shell,
}
