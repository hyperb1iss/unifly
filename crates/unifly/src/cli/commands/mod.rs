//! Command dispatch: bridges CLI args -> core Commands -> output formatting.

pub mod acl;
pub mod admin;
pub mod alarms;
pub mod api;
pub mod clients;
pub mod cloud;
pub mod config_cmd;
pub mod countries;
pub mod devices;
pub mod dns;
pub mod dpi;
pub mod events;
pub mod firewall;
pub mod hotspot;
pub mod nat;
pub mod networks;
pub mod radius;
pub mod settings;
pub mod sites;
pub mod stats;
pub mod system;
pub mod topology;
pub mod traffic_lists;
pub mod util;
pub mod vpn;
pub mod wans;
pub mod wifi;

use std::future::Future;
use std::pin::Pin;

use unifly_api::Controller;

use crate::cli::args::{Command, GlobalOpts};
use crate::cli::error::CliError;

type Handled<'a> = Pin<Box<dyn Future<Output = Result<(), CliError>> + 'a>>;

/// Dispatch a controller-bound command to the appropriate handler.
#[allow(clippy::future_not_send)]
pub async fn dispatch(
    cmd: Command,
    controller: &Controller,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    route(cmd, controller, global).await
}

/// Pick the handler for `cmd`. The match names every `Command` variant, so a
/// new command fails to compile until it is routed here. Each arm boxes its
/// handler instead of awaiting it, which keeps the match flat enough for the
/// cognitive complexity lint.
fn route<'a>(cmd: Command, controller: &'a Controller, global: &'a GlobalOpts) -> Handled<'a> {
    match cmd {
        Command::Acl(args) => Box::pin(acl::handle(controller, args, global)),
        Command::Admin(args) => Box::pin(admin::handle(controller, args, global)),
        Command::Alarms(args) => Box::pin(alarms::handle(controller, args, global)),
        Command::Api(args) => Box::pin(api::handle(controller, args, global)),
        Command::Clients(args) => Box::pin(clients::handle(controller, args, global)),
        Command::Countries => Box::pin(countries::handle(controller, global)),
        Command::Devices(args) => Box::pin(devices::handle(controller, args, global)),
        Command::Dns(args) => Box::pin(dns::handle(controller, args, global)),
        Command::Dpi(args) => Box::pin(dpi::handle(controller, args, global)),
        Command::Events(args) => Box::pin(events::handle(controller, args, global)),
        Command::Firewall(args) => Box::pin(firewall::handle(controller, args, global)),
        Command::Hotspot(args) => Box::pin(hotspot::handle(controller, args, global)),
        Command::Nat(args) => Box::pin(nat::handle(controller, args, global)),
        Command::Networks(args) => Box::pin(networks::handle(controller, args, global)),
        Command::Radius(args) => Box::pin(radius::handle(controller, args, global)),
        Command::Settings(args) => Box::pin(settings::handle(controller, args, global)),
        Command::Sites(args) => Box::pin(sites::handle(controller, args, global)),
        Command::Stats(args) => Box::pin(stats::handle(controller, args, global)),
        Command::System(args) => Box::pin(system::handle(controller, args, global)),
        Command::Topology => Box::pin(topology::handle(controller, global)),
        Command::TrafficLists(args) => Box::pin(traffic_lists::handle(controller, args, global)),
        Command::Vpn(args) => Box::pin(vpn::handle(controller, args, global)),
        Command::Wans(args) => Box::pin(wans::handle(controller, args, global)),
        Command::Wifi(args) => Box::pin(wifi::handle(controller, args, global)),
        // The binary runs these before it builds a controller.
        Command::Cloud(_) => local_only("cloud"),
        Command::Config(_) => local_only("config"),
        Command::Completions(_) => local_only("completions"),
        Command::Skill(_) => local_only("skill"),
        #[cfg(feature = "tui")]
        Command::Tui(_) => local_only("tui"),
    }
}

fn local_only(command: &str) -> Handled<'static> {
    let err = CliError::ApiError {
        code: "internal".into(),
        message: format!("`unifly {command}` does not run through controller dispatch"),
        request_id: None,
    };
    Box::pin(std::future::ready(Err(err)))
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use unifly_api::{DEFAULT_DEMO_SEED, DemoFleetOptions};

    use super::*;
    use crate::cli::args::Cli;

    #[tokio::test]
    async fn local_commands_fail_cleanly_instead_of_panicking() {
        let controller = Controller::demo(DemoFleetOptions::with_seed(DEFAULT_DEMO_SEED));
        for argv in [
            &["unifly", "skill"][..],
            &["unifly", "completions", "bash"][..],
            &["unifly", "config", "show"][..],
            &["unifly", "cloud", "hosts"][..],
        ] {
            let cli = Cli::try_parse_from(argv).expect("valid arguments");
            let err = dispatch(cli.command, &controller, &cli.global)
                .await
                .expect_err("local commands never reach a controller handler");
            assert!(
                err.to_string()
                    .contains("does not run through controller dispatch"),
                "{argv:?}: {err}"
            );
        }
    }
}
