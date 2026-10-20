//! DPI reference data command handlers.

use tabled::Tabled;
use unifly_api::{Command as CoreCommand, Controller, DpiApplication, DpiCategory};

use crate::cli::args::{DpiArgs, DpiCommand, GlobalOpts};
use crate::cli::error::CliError;
use crate::cli::output;

use super::util;

// ── Table rows ──────────────────────────────────────────────────────

#[derive(Tabled)]
struct DpiAppRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Category")]
    category_id: String,
    #[tabled(rename = "TX Bytes")]
    tx_bytes: String,
    #[tabled(rename = "RX Bytes")]
    rx_bytes: String,
}

fn dpi_app_row(a: &DpiApplication, p: &output::Painter) -> DpiAppRow {
    DpiAppRow {
        id: p.id(&a.id.to_string()),
        name: p.name(&a.name),
        category_id: p.muted(&a.category_id.to_string()),
        tx_bytes: p.number(&a.tx_bytes.to_string()),
        rx_bytes: p.number(&a.rx_bytes.to_string()),
    }
}

#[derive(Tabled)]
struct DpiCategoryRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Apps")]
    app_count: String,
    #[tabled(rename = "TX Bytes")]
    tx_bytes: String,
    #[tabled(rename = "RX Bytes")]
    rx_bytes: String,
}

fn dpi_category_row(c: &DpiCategory, p: &output::Painter) -> DpiCategoryRow {
    DpiCategoryRow {
        id: p.id(&c.id.to_string()),
        name: p.name(&c.name),
        app_count: p.number(&c.apps.len().to_string()),
        tx_bytes: p.number(&c.tx_bytes.to_string()),
        rx_bytes: p.number(&c.rx_bytes.to_string()),
    }
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: DpiArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    let p = output::Painter::new(global);

    match args.command {
        DpiCommand::Apps(list) => {
            let apps = util::apply_list_args(
                controller.list_dpi_applications().await?,
                &list,
                global.quiet,
            );
            let out = output::render_list(
                &global.output,
                &apps,
                |a| dpi_app_row(a, &p),
                |a| a.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        DpiCommand::Status => {
            let enabled = controller.is_dpi_enabled().await?;
            if !global.quiet {
                eprintln!("DPI is {}", if enabled { "enabled" } else { "disabled" });
            }
            Ok(())
        }

        DpiCommand::Enable => {
            controller
                .execute(CoreCommand::SetDpiEnabled { enabled: true })
                .await?;
            if !global.quiet {
                eprintln!("DPI enabled");
            }
            Ok(())
        }

        DpiCommand::Disable => {
            controller
                .execute(CoreCommand::SetDpiEnabled { enabled: false })
                .await?;
            if !global.quiet {
                eprintln!("DPI disabled");
            }
            Ok(())
        }

        DpiCommand::Categories(list) => {
            let cats =
                util::apply_list_args(controller.list_dpi_categories().await?, &list, global.quiet);
            let out = output::render_list(
                &global.output,
                &cats,
                |c| dpi_category_row(c, &p),
                |c| c.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
    }
}
