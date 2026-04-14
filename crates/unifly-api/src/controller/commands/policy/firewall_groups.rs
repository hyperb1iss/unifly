use serde_json::json;

use crate::command::{Command, CommandResult};
use crate::core_error::CoreError;
use crate::model::FirewallGroupType;

use super::super::{CommandContext, require_session};

pub(super) async fn route(ctx: &CommandContext, cmd: Command) -> Result<CommandResult, CoreError> {
    let session = ctx.session.as_ref();

    match cmd {
        Command::CreateFirewallGroup(req) => {
            let session = require_session(session)?;

            let group_type = match req.group_type {
                FirewallGroupType::PortGroup => "port-group",
                FirewallGroupType::AddressGroup => "address-group",
                FirewallGroupType::Ipv6AddressGroup => "ipv6-address-group",
            };

            let body = json!({
                "name": req.name,
                "group_type": group_type,
                "group_members": req.group_members,
            });

            session.create_firewall_group(&body).await?;
            Ok(CommandResult::Ok)
        }
        Command::UpdateFirewallGroup { id, update } => {
            let session = require_session(session)?;
            let record_id = id.to_string();

            // Fetch existing to merge
            let groups = session.list_firewall_groups().await?;
            let existing = groups
                .iter()
                .find(|g| g.get("_id").and_then(serde_json::Value::as_str) == Some(&record_id))
                .ok_or_else(|| CoreError::NotFound {
                    entity_type: "firewall group".into(),
                    identifier: record_id.clone(),
                })?
                .clone();

            let mut body = existing;
            if let Some(name) = &update.name {
                body["name"] = json!(name);
            }
            if let Some(members) = &update.group_members {
                body["group_members"] = json!(members);
            }

            session.update_firewall_group(&record_id, &body).await?;
            Ok(CommandResult::Ok)
        }
        Command::DeleteFirewallGroup { id } => {
            let session = require_session(session)?;
            session.delete_firewall_group(&id.to_string()).await?;
            Ok(CommandResult::Ok)
        }
        _ => unreachable!("firewall_groups::route received non-firewall-group command"),
    }
}
