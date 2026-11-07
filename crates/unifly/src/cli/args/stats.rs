use clap::{Args, Subcommand, ValueEnum};

#[derive(Debug, Args)]
pub struct StatsArgs {
    #[command(subcommand)]
    pub command: StatsCommand,
}

#[derive(Debug, Subcommand)]
pub enum StatsCommand {
    /// Site-level statistics (session API)
    Site(StatsQuery),

    /// Per-device statistics (session API)
    Device(StatsQuery),

    /// Per-client statistics (session API)
    Client(StatsQuery),

    /// Gateway statistics (session API)
    Gateway(StatsQuery),

    /// DPI traffic analysis (session API)
    Dpi {
        /// Analysis type: by-app or by-cat
        #[arg(long, default_value = "by-app", value_enum)]
        group_by: DpiGroupBy,

        /// Filter by MAC addresses (comma-separated)
        #[arg(long, value_delimiter = ',')]
        macs: Option<Vec<String>>,
    },
}

#[derive(Debug, Args)]
pub struct StatsQuery {
    /// Reporting interval
    #[arg(long, default_value = "hourly", value_enum)]
    pub interval: StatsInterval,

    /// Start time (RFC3339 or Unix seconds)
    #[arg(long)]
    pub start: Option<String>,

    /// End time (RFC3339 or Unix seconds)
    #[arg(long)]
    pub end: Option<String>,

    /// Attributes to include (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub attrs: Option<Vec<String>>,

    /// Filter by MAC addresses (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub macs: Option<Vec<String>>,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum StatsInterval {
    #[value(name = "5m")]
    FiveMinutes,
    Hourly,
    Daily,
    Monthly,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum DpiGroupBy {
    ByApp,
    ByCat,
}
