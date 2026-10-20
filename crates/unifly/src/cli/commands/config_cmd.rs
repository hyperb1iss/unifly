//! Config subcommand handlers.

mod handler;
mod interactive;
mod pending_secrets;
mod support;

use crate::cli::args::{ConfigArgs, GlobalOpts};
use crate::cli::error::CliError;

pub async fn handle(args: ConfigArgs, global: &GlobalOpts) -> Result<(), CliError> {
    handler::handle(args, global).await
}
