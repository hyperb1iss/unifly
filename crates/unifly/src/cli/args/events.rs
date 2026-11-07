use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct EventsArgs {
    #[command(subcommand)]
    pub command: EventsCommand,
}

#[derive(Debug, Subcommand)]
pub enum EventsCommand {
    /// List recent events (session API)
    #[command(alias = "ls")]
    List {
        /// Max results
        #[arg(long, short = 'l', default_value = "100")]
        limit: u32,

        /// Hours of history to include
        #[arg(long, default_value = "24")]
        within: u32,

        /// Include firewall-block events (intrusion-prevention events are always included)
        #[arg(long)]
        include_firewall: bool,
    },

    /// Stream real-time events via WebSocket (session API)
    Watch {
        /// Event types to filter (comma-separated)
        #[arg(long, value_delimiter = ',')]
        types: Option<Vec<String>>,
    },
}
