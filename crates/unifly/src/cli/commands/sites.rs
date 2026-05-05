//! Site command handlers.

use std::sync::Arc;

use tabled::Tabled;
use unifly_api::{Command as CoreCommand, Controller, Site};

use crate::cli::args::{GlobalOpts, SitesArgs, SitesCommand};
use crate::cli::error::CliError;
use crate::cli::output;

use super::util;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct SiteRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Internal")]
    internal: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Devices")]
    devices: String,
    #[tabled(rename = "Clients")]
    clients: String,
}

fn site_row(s: &Arc<Site>, p: &output::Painter) -> SiteRow {
    SiteRow {
        id: p.id(&s.id.to_string()),
        internal: p.id(&s.internal_name),
        name: p.name(&s.name),
        devices: p.number(&s.device_count.map(|c| c.to_string()).unwrap_or_default()),
        clients: p.number(&s.client_count.map(|c| c.to_string()).unwrap_or_default()),
    }
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: SitesArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    let p = output::Painter::new(global);

    match args.command {
        SitesCommand::List(list) => {
            let all = controller.sites_snapshot();
            let snap = util::apply_list_args(all.iter().cloned(), &list, |s, filter| {
                util::matches_json_filter(s, filter)
            });
            let out = output::render_list(
                &global.output,
                &snap,
                |s| site_row(s, &p),
                |s| s.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        SitesCommand::Create { name, description } => {
            controller
                .execute(CoreCommand::CreateSite { name, description })
                .await?;
            if !global.quiet {
                eprintln!("Site created");
            }
            Ok(())
        }

        SitesCommand::Delete { name } => {
            if !util::confirm(
                &format!("Delete site '{name}'? This is destructive."),
                global.yes,
            )? {
                return Ok(());
            }
            controller.execute(CoreCommand::DeleteSite { name }).await?;
            if !global.quiet {
                eprintln!("Site deleted");
            }
            Ok(())
        }
    }
}
