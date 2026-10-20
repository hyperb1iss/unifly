//! RADIUS profile command handlers.

use tabled::Tabled;
use unifly_api::{Controller, RadiusProfile};

use crate::cli::args::{GlobalOpts, RadiusArgs, RadiusCommand};
use crate::cli::error::CliError;
use crate::cli::output;

use super::util;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct RadiusProfileRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
}

fn radius_profile_row(r: &RadiusProfile, p: &output::Painter) -> RadiusProfileRow {
    RadiusProfileRow {
        id: p.id(&r.id.to_string()),
        name: p.name(&r.name),
    }
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: RadiusArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    let p = output::Painter::new(global);

    match args.command {
        RadiusCommand::Profiles(list) => {
            let profiles = util::apply_list_args(controller.list_radius_profiles().await?, &list);
            let out = output::render_list(
                &global.output,
                &profiles,
                |r| radius_profile_row(r, &p),
                |r| r.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
    }
}
