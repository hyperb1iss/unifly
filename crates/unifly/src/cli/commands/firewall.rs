//! Firewall command handlers (policies + zones + groups).

mod groups;
mod policies;
mod shared;
mod zones;

use unifly_api::Controller;

use crate::cli::args::{FirewallArgs, FirewallCommand, GlobalOpts};
use crate::cli::error::CliError;
use crate::cli::output;

use super::util;

pub async fn handle(
    controller: &Controller,
    args: FirewallArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    let painter = output::Painter::new(global);

    match args.command {
        FirewallCommand::Policies(args) => {
            util::ensure_integration_access(controller, "firewall policies").await?;
            policies::handle(controller, args.command, global, &painter).await
        }
        FirewallCommand::Zones(args) => {
            util::ensure_integration_access(controller, "firewall zones").await?;
            zones::handle(controller, args.command, global, &painter).await
        }
        FirewallCommand::Groups(args) => {
            util::ensure_session_access(controller, "firewall groups").await?;
            groups::handle(controller, args.command, global, &painter).await
        }
    }
}
