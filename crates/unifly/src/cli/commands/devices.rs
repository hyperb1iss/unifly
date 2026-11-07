//! Device command handlers.

mod handler;
mod render;
mod switching;

use unifly_api::Controller;

use crate::cli::args::{DevicesArgs, GlobalOpts};
use crate::cli::error::CliError;

pub async fn handle(
    controller: &Controller,
    args: DevicesArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    handler::handle(controller, args, global).await
}
