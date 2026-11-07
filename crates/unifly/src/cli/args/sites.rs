use clap::{Args, Subcommand};

use super::ListArgs;

#[derive(Debug, Args)]
pub struct SitesArgs {
    #[command(subcommand)]
    pub command: SitesCommand,
}

#[derive(Debug, Subcommand)]
pub enum SitesCommand {
    /// List local sites
    #[command(alias = "ls")]
    List(ListArgs),

    /// Create a new site (session API)
    Create {
        /// Display name (the controller generates its internal reference)
        #[arg(long, alias = "description", required = true)]
        name: String,
    },

    /// Delete a site (session API)
    Delete {
        /// Site name
        name: String,
    },
}
