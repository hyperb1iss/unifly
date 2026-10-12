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

use unifly_api::Controller;

use crate::cli::args::{Command, GlobalOpts};
use crate::cli::error::CliError;

/// Dispatch a controller-bound command to the appropriate handler.
#[allow(clippy::future_not_send)]
pub async fn dispatch(
    cmd: Command,
    controller: &Controller,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match cmd {
        Command::Acl(args) => acl::handle(controller, args, global).await,
        Command::Admin(args) => admin::handle(controller, args, global).await,
        Command::Alarms(args) => alarms::handle(controller, args, global).await,
        Command::Api(args) => api::handle(controller, args, global).await,
        Command::Clients(args) => clients::handle(controller, args, global).await,
        Command::Countries => countries::handle(controller, global).await,
        Command::Devices(args) => devices::handle(controller, args, global).await,
        Command::Dns(args) => dns::handle(controller, args, global).await,
        Command::Dpi(args) => dpi::handle(controller, args, global).await,
        Command::Events(args) => events::handle(controller, args, global).await,
        Command::Firewall(args) => firewall::handle(controller, args, global).await,
        Command::Hotspot(args) => hotspot::handle(controller, args, global).await,
        Command::Nat(args) => nat::handle(controller, args, global).await,
        other => dispatch_extended(other, controller, global).await,
    }
}

#[allow(clippy::future_not_send)]
async fn dispatch_extended(
    cmd: Command,
    controller: &Controller,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match cmd {
        Command::Networks(args) => networks::handle(controller, args, global).await,
        Command::Radius(args) => radius::handle(controller, args, global).await,
        Command::Settings(args) => settings::handle(controller, args, global).await,
        Command::Sites(args) => sites::handle(controller, args, global).await,
        Command::Stats(args) => stats::handle(controller, args, global).await,
        Command::System(args) => system::handle(controller, args, global).await,
        Command::Topology => topology::handle(controller, global).await,
        Command::TrafficLists(args) => traffic_lists::handle(controller, args, global).await,
        Command::Vpn(args) => vpn::handle(controller, args, global).await,
        Command::Wans(args) => wans::handle(controller, args, global).await,
        Command::Wifi(args) => wifi::handle(controller, args, global).await,
        // Local commands, cloud fleet queries, and Tui are handled before dispatch.
        Command::Cloud(_) | Command::Config(_) | Command::Completions(_) | Command::Skill(_) => {
            unreachable!()
        }
        #[cfg(feature = "tui")]
        Command::Tui(_) => unreachable!(),
        _ => unreachable!(),
    }
}
