use crate::command::{Command, CommandResult};
use crate::core_error::CoreError;
use crate::model::FirewallAction;

use super::super::{
    CommandContext, build_acl_filter_value, merge_acl_filter_value, require_integration,
    require_uuid,
};

pub(super) async fn route(ctx: &CommandContext, cmd: Command) -> Result<CommandResult, CoreError> {
    let integration = ctx.integration.as_ref();
    let site_id = ctx.site_id;

    match cmd {
        Command::CreateAclRule(req) => {
            let (ic, sid) = require_integration(integration, site_id, "CreateAclRule")?;
            let action_str = match req.action {
                FirewallAction::Allow => "ALLOW",
                FirewallAction::Block => "BLOCK",
                FirewallAction::Reject => "REJECT",
            };
            let body = crate::integration_types::AclRuleCreateUpdate {
                name: req.name,
                rule_type: req.rule_type,
                action: action_str.into(),
                enabled: req.enabled,
                description: req.description,
                source_filter: Some(build_acl_filter_value(
                    &req.source_zone_id,
                    req.source_port.as_deref(),
                    req.protocol.as_deref(),
                )),
                destination_filter: Some(build_acl_filter_value(
                    &req.destination_zone_id,
                    req.destination_port.as_deref(),
                    req.protocol.as_deref(),
                )),
                enforcing_device_filter: req.enforcing_device_filter,
            };
            let created = ic.create_acl_rule(&sid, &body).await?;
            Ok(CommandResult::AclRule(created.into()))
        }
        Command::UpdateAclRule { id, update } => {
            let (ic, sid) = require_integration(integration, site_id, "UpdateAclRule")?;
            let uuid = require_uuid(&id)?;
            let existing = ic.get_acl_rule(&sid, &uuid).await?;
            let action_str = match update.action {
                Some(FirewallAction::Allow) => "ALLOW".into(),
                Some(FirewallAction::Block) => "BLOCK".into(),
                Some(FirewallAction::Reject) => "REJECT".into(),
                None => existing.action,
            };
            let body = crate::integration_types::AclRuleCreateUpdate {
                name: update.name.unwrap_or(existing.name),
                rule_type: update.rule_type.unwrap_or(existing.rule_type),
                action: action_str,
                enabled: update.enabled.unwrap_or(existing.enabled),
                description: update.description.or(existing.description),
                source_filter: merge_acl_filter_value(
                    existing.source_filter,
                    update.source_zone_id.as_ref(),
                    update.source_port.as_deref(),
                    update.protocol.as_deref(),
                ),
                destination_filter: merge_acl_filter_value(
                    existing.destination_filter,
                    update.destination_zone_id.as_ref(),
                    update.destination_port.as_deref(),
                    update.protocol.as_deref(),
                ),
                enforcing_device_filter: existing.enforcing_device_filter,
            };
            ic.update_acl_rule(&sid, &uuid, &body).await?;
            Ok(CommandResult::Ok)
        }
        Command::DeleteAclRule { id } => {
            let (ic, sid) = require_integration(integration, site_id, "DeleteAclRule")?;
            let uuid = require_uuid(&id)?;
            ic.delete_acl_rule(&sid, &uuid).await?;
            Ok(CommandResult::Ok)
        }
        Command::ReorderAclRules { ordered_ids } => {
            let (ic, sid) = require_integration(integration, site_id, "ReorderAclRules")?;
            let uuids: Result<Vec<uuid::Uuid>, _> = ordered_ids.iter().map(require_uuid).collect();
            let body = crate::integration_types::AclRuleOrdering {
                ordered_acl_rule_ids: uuids?,
            };
            ic.set_acl_rule_ordering(&sid, &body).await?;
            Ok(CommandResult::Ok)
        }
        _ => unreachable!("acl::route received non-acl command"),
    }
}
