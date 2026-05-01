use crate::command::{Command, CommandResult};
use crate::core_error::CoreError;
use crate::model::FirewallAction;

use super::super::{CommandContext, build_endpoint_json, require_integration, require_uuid};

#[allow(clippy::too_many_lines)]
pub(super) async fn route(ctx: &CommandContext, cmd: Command) -> Result<CommandResult, CoreError> {
    let integration = ctx.integration.as_ref();
    let site_id = ctx.site_id;

    match cmd {
        Command::CreateFirewallPolicy(req) => {
            let (ic, sid) = require_integration(integration, site_id, "CreateFirewallPolicy")?;
            let action_str = match req.action {
                FirewallAction::Allow => "ALLOW",
                FirewallAction::Block => "DROP",
                FirewallAction::Reject => "REJECT",
            };
            let source =
                build_endpoint_json(&req.source_zone_id.to_string(), req.source_filter.as_ref());
            let destination = build_endpoint_json(
                &req.destination_zone_id.to_string(),
                req.destination_filter.as_ref(),
            );
            let ip_version = req.ip_version.as_deref().unwrap_or("IPV4_AND_IPV6");
            let body = crate::integration_types::FirewallPolicyCreateUpdate {
                name: req.name,
                description: req.description,
                enabled: req.enabled,
                action: serde_json::json!({ "type": action_str, "allowReturnTraffic": req.allow_return_traffic }),
                source,
                destination,
                ip_protocol_scope: serde_json::json!({ "ipVersion": ip_version }),
                logging_enabled: req.logging_enabled,
                ipsec_filter: None,
                schedule: None,
                connection_state_filter: req.connection_states,
            };
            let resp = ic.create_firewall_policy(&sid, &body).await?;
            Ok(CommandResult::CreatedId(crate::model::EntityId::Uuid(
                resp.id,
            )))
        }
        Command::UpdateFirewallPolicy { id, update } => {
            let (ic, sid) = require_integration(integration, site_id, "UpdateFirewallPolicy")?;
            let uuid = require_uuid(&id)?;
            let existing = ic.get_firewall_policy(&sid, &uuid).await?;

            let source = if let Some(ref spec) = update.source_filter {
                let zone_id = existing
                    .source
                    .as_ref()
                    .and_then(|s| s.zone_id)
                    .map(|u| u.to_string())
                    .unwrap_or_default();
                build_endpoint_json(&zone_id, Some(spec))
            } else {
                serde_json::to_value(&existing.source).unwrap_or_default()
            };

            let destination = if let Some(ref spec) = update.destination_filter {
                let zone_id = existing
                    .destination
                    .as_ref()
                    .and_then(|d| d.zone_id)
                    .map(|u| u.to_string())
                    .unwrap_or_default();
                build_endpoint_json(&zone_id, Some(spec))
            } else {
                serde_json::to_value(&existing.destination).unwrap_or_default()
            };

            let action = if update.action.is_some() || update.allow_return_traffic.is_some() {
                let action_type = update.action.map_or_else(
                    || {
                        existing
                            .action
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("ALLOW")
                            .to_owned()
                    },
                    |a| {
                        match a {
                            FirewallAction::Allow => "ALLOW",
                            FirewallAction::Block => "DROP",
                            FirewallAction::Reject => "REJECT",
                        }
                        .to_owned()
                    },
                );
                let allow_return = update.allow_return_traffic.unwrap_or_else(|| {
                    existing
                        .action
                        .get("allowReturnTraffic")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true)
                });
                serde_json::json!({ "type": action_type, "allowReturnTraffic": allow_return })
            } else {
                existing.action
            };

            let ip_protocol_scope = if let Some(ref version) = update.ip_version {
                serde_json::json!({ "ipVersion": version })
            } else {
                existing
                    .ip_protocol_scope
                    .unwrap_or_else(|| serde_json::json!({ "ipVersion": "IPV4_AND_IPV6" }))
            };

            let connection_state_filter = update.connection_states.or_else(|| {
                existing
                    .extra
                    .get("connectionStateFilter")
                    .and_then(serde_json::Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                            .collect::<Vec<_>>()
                    })
            });

            let payload = crate::integration_types::FirewallPolicyCreateUpdate {
                name: update.name.unwrap_or(existing.name),
                description: update.description.or(existing.description),
                enabled: update.enabled.unwrap_or(existing.enabled),
                action,
                source,
                destination,
                ip_protocol_scope,
                logging_enabled: update.logging_enabled.unwrap_or(existing.logging_enabled),
                ipsec_filter: existing
                    .extra
                    .get("ipsecFilter")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned),
                schedule: existing.extra.get("schedule").cloned(),
                connection_state_filter,
            };

            ic.update_firewall_policy(&sid, &uuid, &payload).await?;
            Ok(CommandResult::Ok)
        }
        Command::DeleteFirewallPolicy { id } => {
            let (ic, sid) = require_integration(integration, site_id, "DeleteFirewallPolicy")?;
            let uuid = require_uuid(&id)?;
            ic.delete_firewall_policy(&sid, &uuid).await?;
            Ok(CommandResult::Ok)
        }
        Command::PatchFirewallPolicy {
            id,
            enabled,
            logging,
        } => {
            let (ic, sid) = require_integration(integration, site_id, "PatchFirewallPolicy")?;
            let uuid = require_uuid(&id)?;
            let body = crate::integration_types::FirewallPolicyPatch {
                enabled,
                logging_enabled: logging,
            };
            ic.patch_firewall_policy(&sid, &uuid, &body).await?;
            Ok(CommandResult::Ok)
        }
        Command::ReorderFirewallPolicies {
            zone_pair,
            before_system_ids,
            after_system_ids,
        } => {
            let (ic, sid) = require_integration(integration, site_id, "ReorderFirewallPolicies")?;
            let source_zone_uuid = require_uuid(&zone_pair.0)?;
            let destination_zone_uuid = require_uuid(&zone_pair.1)?;
            let before: Result<Vec<uuid::Uuid>, _> =
                before_system_ids.iter().map(require_uuid).collect();
            let after: Result<Vec<uuid::Uuid>, _> =
                after_system_ids.iter().map(require_uuid).collect();
            let body = crate::integration_types::FirewallPolicyOrdering {
                before_system_defined: before?,
                after_system_defined: after?,
            };
            ic.set_firewall_policy_ordering(&sid, &source_zone_uuid, &destination_zone_uuid, &body)
                .await?;
            Ok(CommandResult::Ok)
        }
        Command::CreateFirewallZone(req) => {
            let (ic, sid) = require_integration(integration, site_id, "CreateFirewallZone")?;
            let network_uuids: Result<Vec<uuid::Uuid>, _> =
                req.network_ids.iter().map(require_uuid).collect();
            let body = crate::integration_types::FirewallZoneCreateUpdate {
                name: req.name,
                network_ids: network_uuids?,
            };
            ic.create_firewall_zone(&sid, &body).await?;
            Ok(CommandResult::Ok)
        }
        Command::UpdateFirewallZone { id, update } => {
            let (ic, sid) = require_integration(integration, site_id, "UpdateFirewallZone")?;
            let uuid = require_uuid(&id)?;
            let existing = ic.get_firewall_zone(&sid, &uuid).await?;
            let network_ids = if let Some(ids) = update.network_ids {
                let uuids: Result<Vec<uuid::Uuid>, _> = ids.iter().map(require_uuid).collect();
                uuids?
            } else {
                existing.network_ids
            };
            let body = crate::integration_types::FirewallZoneCreateUpdate {
                name: update.name.unwrap_or(existing.name),
                network_ids,
            };
            ic.update_firewall_zone(&sid, &uuid, &body).await?;
            Ok(CommandResult::Ok)
        }
        Command::DeleteFirewallZone { id } => {
            let (ic, sid) = require_integration(integration, site_id, "DeleteFirewallZone")?;
            let uuid = require_uuid(&id)?;
            ic.delete_firewall_zone(&sid, &uuid).await?;
            Ok(CommandResult::Ok)
        }
        _ => unreachable!("firewall::route received non-firewall command"),
    }
}
