//! WAN interface command handlers.

use tabled::Tabled;
use unifly_api::{Controller, WanInterface};

use crate::cli::args::{GlobalOpts, WansArgs, WansCommand};
use crate::cli::error::CliError;
use crate::cli::output;

use super::util;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct WanRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "IP")]
    ip: String,
    #[tabled(rename = "Gateway")]
    gateway: String,
}

fn wan_row(w: &WanInterface, p: &output::Painter) -> WanRow {
    WanRow {
        id: p.id(&w.id.to_string()),
        name: p.name(&w.name.clone().unwrap_or_default()),
        ip: p.ip(&w.ip.map(|ip| ip.to_string()).unwrap_or_default()),
        gateway: p.ip(&w.gateway.map(|gw| gw.to_string()).unwrap_or_default()),
    }
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: WansArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    let p = output::Painter::new(global);

    match args.command {
        WansCommand::List(list) => {
            let wans = util::apply_list_args(controller.list_wans().await?, &list);
            let out = output::render_list(
                &global.output,
                &wans,
                |w| wan_row(w, &p),
                |w| w.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
    }
}
