use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct AlarmsArgs {
    #[command(subcommand)]
    pub command: AlarmsCommand,
}

#[derive(Debug, Subcommand)]
pub enum AlarmsCommand {
    /// List dashboard alerts (legacy alarms on older controllers)
    #[command(alias = "ls")]
    List {
        /// Only show unarchived alarms
        #[arg(long)]
        unarchived: bool,

        /// Max results
        #[arg(long, short = 'l', default_value = "100")]
        limit: u32,
    },

    /// Acknowledge an alert group (one legacy alarm on older controllers)
    Archive {
        /// Alert ID from alarms list; acknowledges its aggregated group on modern controllers
        id: String,
    },

    /// Acknowledge all alert groups (all legacy alarms on older controllers)
    ArchiveAll,
}
