//! Hotspot voucher command handlers.

use std::sync::Arc;

use tabled::Tabled;
use unifly_api::model::Voucher;
use unifly_api::{Command as CoreCommand, Controller, CreateVouchersRequest, EntityId};

use crate::cli::args::{GlobalOpts, HotspotArgs, HotspotCommand};
use crate::cli::error::CliError;
use crate::cli::output;

use super::util;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct VoucherRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Code")]
    code: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Minutes")]
    minutes: String,
    #[tabled(rename = "Expired")]
    expired: String,
}

fn voucher_row(v: &Arc<Voucher>, p: &output::Painter) -> VoucherRow {
    VoucherRow {
        id: p.id(&v.id.to_string()),
        code: p.name(&v.code),
        name: p.name(&v.name.clone().unwrap_or_default()),
        minutes: p.number(
            &v.time_limit_minutes
                .map(|m| m.to_string())
                .unwrap_or_default(),
        ),
        expired: p.enabled(!v.expired),
    }
}

fn detail(v: &Arc<Voucher>) -> String {
    [
        format!("ID:         {}", v.id),
        format!("Code:       {}", v.code),
        format!("Name:       {}", v.name.as_deref().unwrap_or("-")),
        format!("Expired:    {}", v.expired),
        format!(
            "Minutes:    {}",
            v.time_limit_minutes
                .map_or_else(|| "-".into(), |m: u32| m.to_string())
        ),
        format!(
            "Data Limit: {} MB",
            v.data_usage_limit_mb
                .map_or_else(|| "-".into(), |m: u64| m.to_string())
        ),
        format!(
            "Guests:     {}/{}",
            v.authorized_guest_count.unwrap_or(0),
            v.authorized_guest_limit
                .map_or_else(|| "unlimited".into(), |l: u32| l.to_string())
        ),
    ]
    .join("\n")
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: HotspotArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    util::ensure_integration_access(controller, "hotspot").await?;

    let p = output::Painter::new(global);

    match args.command {
        HotspotCommand::List { limit, offset } => {
            let all = controller.vouchers_snapshot();
            let offset = usize::try_from(offset).unwrap_or(usize::MAX);
            let limit = usize::try_from(limit).unwrap_or(usize::MAX);
            let snap: Vec<_> = all.iter().skip(offset).take(limit).cloned().collect();
            let out = output::render_list(
                &global.output,
                &snap,
                |v| voucher_row(v, &p),
                |v| v.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        HotspotCommand::Get { id } => {
            let snap = controller.vouchers_snapshot();
            let found = snap.iter().find(|v| v.id.to_string() == id);
            match found {
                Some(v) => {
                    let out =
                        output::render_single(&global.output, v, detail, |v| v.id.to_string());
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "voucher".into(),
                        identifier: id,
                        list_command: "hotspot list".into(),
                    });
                }
            }
            Ok(())
        }

        HotspotCommand::Create {
            name,
            count,
            minutes,
            guest_limit,
            data_limit_mb,
            rx_limit_kbps,
            tx_limit_kbps,
        } => {
            let req = CreateVouchersRequest {
                count,
                name: Some(name),
                time_limit_minutes: Some(minutes),
                data_usage_limit_mb: data_limit_mb,
                rx_rate_limit_kbps: rx_limit_kbps,
                tx_rate_limit_kbps: tx_limit_kbps,
                authorized_guest_limit: guest_limit,
            };

            let result = controller.execute(CoreCommand::CreateVouchers(req)).await?;
            if let unifly_api::CommandResult::Vouchers(vouchers) = result {
                let vouchers: Vec<Arc<Voucher>> = vouchers.into_iter().map(Arc::new).collect();
                let out = output::render_list(
                    &global.output,
                    &vouchers,
                    |v| voucher_row(v, &p),
                    |v| v.id.to_string(),
                );
                output::print_output(&out, global.quiet);
            } else {
                output::warn_unrenderable_create(global.quiet);
            }
            if !global.quiet {
                eprintln!("{count} voucher(s) created");
            }
            Ok(())
        }

        HotspotCommand::Delete { id } => {
            let eid = EntityId::from(id.clone());
            if !util::confirm(&format!("Delete voucher {id}?"), global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::DeleteVoucher { id: eid })
                .await?;
            if !global.quiet {
                eprintln!("Voucher deleted");
            }
            Ok(())
        }

        HotspotCommand::Purge { filter } => {
            if !util::confirm("Purge vouchers matching filter?", global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::PurgeVouchers { filter })
                .await?;
            if !global.quiet {
                eprintln!("Vouchers purged");
            }
            Ok(())
        }
    }
}
