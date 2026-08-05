use crate::command::{Command, CommandResult};
use crate::core_error::CoreError;

use super::super::{
    CommandContext, build_create_dns_policy_fields, build_update_dns_policy_fields,
    dns_policy_type_name, require_integration, require_uuid,
};

pub(super) async fn route(ctx: &CommandContext, cmd: Command) -> Result<CommandResult, CoreError> {
    let integration = ctx.integration.as_ref();
    let site_id = ctx.site_id;

    match cmd {
        Command::CreateDnsPolicy(req) => {
            let (ic, sid) = require_integration(integration, site_id, "CreateDnsPolicy")?;
            let policy_type_str = dns_policy_type_name(req.policy_type);
            let fields = build_create_dns_policy_fields(&req)?;
            let body = crate::integration_types::DnsPolicyCreateUpdate {
                policy_type: policy_type_str.to_owned(),
                enabled: req.enabled,
                fields,
            };
            let created = ic.create_dns_policy(&sid, &body).await?;
            Ok(crate::convert::dns_policy_from_integration(created)
                .map_or(CommandResult::Ok, CommandResult::DnsPolicy))
        }
        Command::UpdateDnsPolicy { id, update } => {
            let (ic, sid) = require_integration(integration, site_id, "UpdateDnsPolicy")?;
            let uuid = require_uuid(&id)?;
            let existing = ic.get_dns_policy(&sid, &uuid).await?;
            let fields = build_update_dns_policy_fields(&existing, &update)?;

            let body = crate::integration_types::DnsPolicyCreateUpdate {
                policy_type: existing.policy_type,
                enabled: update.enabled.unwrap_or(existing.enabled),
                fields,
            };
            ic.update_dns_policy(&sid, &uuid, &body).await?;
            Ok(CommandResult::Ok)
        }
        Command::DeleteDnsPolicy { id } => {
            let (ic, sid) = require_integration(integration, site_id, "DeleteDnsPolicy")?;
            let uuid = require_uuid(&id)?;
            ic.delete_dns_policy(&sid, &uuid).await?;
            Ok(CommandResult::Ok)
        }
        _ => unreachable!("dns::route received non-dns command"),
    }
}
