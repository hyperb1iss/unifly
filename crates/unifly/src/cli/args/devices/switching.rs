use clap::Subcommand;

use super::ListArgs;

/// Read a site-wide switching collection or one record by UUID.
#[derive(Debug, Subcommand)]
pub enum SwitchingCommand {
    /// List records, with local filtering and pagination
    #[command(alias = "ls")]
    List(ListArgs),
    /// Get one record by its Integration API UUID
    Get { id: String },
}
