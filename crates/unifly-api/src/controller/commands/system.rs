use crate::command::{Command, CommandResult};
use crate::core_error::CoreError;
use crate::model::Voucher;

use super::{CommandContext, require_integration, require_session, require_uuid};

#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub(super) async fn route(ctx: &CommandContext, cmd: Command) -> Result<CommandResult, CoreError> {
    let integration = ctx.integration.as_ref();
    let session = ctx.session.as_ref();
    let site_id = ctx.site_id;

    match cmd {
        Command::SetDpiEnabled { enabled } => {
            let session = require_session(session)?;
            session
                .set_site_setting("dpi", &serde_json::json!({ "enabled": enabled }))
                .await?;
            Ok(CommandResult::Ok)
        }
        Command::ArchiveAlarm { id } => {
            let session = require_session(session)?;
            crate::controller::alarms::archive(session, &id.to_string()).await?;
            Ok(CommandResult::Ok)
        }
        Command::ArchiveAllAlarms => {
            let session = require_session(session)?;
            crate::controller::alarms::archive_all(session).await?;
            Ok(CommandResult::Ok)
        }
        Command::CreateBackup => {
            let session = require_session(session)?;
            session.create_backup().await?;
            Ok(CommandResult::Ok)
        }
        Command::DeleteBackup { filename } => {
            let session = require_session(session)?;
            session.delete_backup(&filename).await?;
            Ok(CommandResult::Ok)
        }
        Command::CreateVouchers(req) => {
            let (ic, sid) = require_integration(integration, site_id, "CreateVouchers")?;
            #[allow(clippy::as_conversions, clippy::cast_possible_wrap)]
            let body = crate::integration_types::VoucherCreateRequest {
                name: req.name.unwrap_or_else(|| "Voucher".into()),
                count: Some(req.count as i32),
                time_limit_minutes: i64::from(req.time_limit_minutes.unwrap_or(60)),
                authorized_guest_limit: req.authorized_guest_limit.map(i64::from),
                data_usage_limit_m_bytes: req.data_usage_limit_mb.map(|m| m as i64),
                rx_rate_limit_kbps: req.rx_rate_limit_kbps.map(|r| r as i64),
                tx_rate_limit_kbps: req.tx_rate_limit_kbps.map(|r| r as i64),
            };
            let vouchers = ic.create_vouchers(&sid, &body).await?;
            let domain_vouchers: Vec<Voucher> = vouchers.into_iter().map(Voucher::from).collect();
            Ok(CommandResult::Vouchers(domain_vouchers))
        }
        Command::DeleteVoucher { id } => {
            let (ic, sid) = require_integration(integration, site_id, "DeleteVoucher")?;
            let uuid = require_uuid(&id)?;
            ic.delete_voucher(&sid, &uuid).await?;
            Ok(CommandResult::Ok)
        }
        Command::PurgeVouchers { filter } => {
            let (ic, sid) = require_integration(integration, site_id, "PurgeVouchers")?;
            ic.purge_vouchers(&sid, &filter).await?;
            Ok(CommandResult::Ok)
        }
        Command::CreateSite { name } => {
            let session = require_session(session)?;
            session.create_site(&name).await?;
            Ok(CommandResult::Ok)
        }
        Command::DeleteSite { name } => {
            let session = require_session(session)?;
            session.delete_site(&name).await?;
            Ok(CommandResult::Ok)
        }
        Command::InviteAdmin { name, email, role } => {
            let session = require_session(session)?;
            session.invite_admin(&name, &email, &role).await?;
            Ok(CommandResult::Ok)
        }
        Command::RevokeAdmin { id } => {
            let session = require_session(session)?;
            session.revoke_admin(&id.to_string()).await?;
            Ok(CommandResult::Ok)
        }
        Command::UpdateAdmin { id, role } => {
            let session = require_session(session)?;
            session
                .update_admin(&id.to_string(), role.as_deref())
                .await?;
            Ok(CommandResult::Ok)
        }
        Command::RebootController => {
            let session = require_session(session)?;
            session.reboot_controller().await?;
            Ok(CommandResult::Ok)
        }
        Command::PoweroffController => {
            let session = require_session(session)?;
            session.poweroff_controller().await?;
            Ok(CommandResult::Ok)
        }
        _ => unreachable!("system::route received non-system command"),
    }
}
