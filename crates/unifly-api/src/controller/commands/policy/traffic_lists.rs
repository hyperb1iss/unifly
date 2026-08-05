use crate::command::{Command, CommandResult};
use crate::core_error::CoreError;

use super::super::{
    CommandContext, require_integration, require_uuid, traffic_matching_list_items,
};

pub(super) async fn route(ctx: &CommandContext, cmd: Command) -> Result<CommandResult, CoreError> {
    let integration = ctx.integration.as_ref();
    let site_id = ctx.site_id;

    match cmd {
        Command::CreateTrafficMatchingList(req) => {
            let (ic, sid) = require_integration(integration, site_id, "CreateTrafficMatchingList")?;
            let mut fields = serde_json::Map::new();
            fields.insert(
                "items".into(),
                serde_json::Value::Array(traffic_matching_list_items(
                    &req.entries,
                    req.raw_items.as_deref(),
                )),
            );
            if let Some(desc) = req.description {
                fields.insert("description".into(), serde_json::Value::String(desc));
            }
            let body = crate::integration_types::TrafficMatchingListCreateUpdate {
                name: req.name,
                list_type: req.list_type,
                fields,
            };
            let created = ic.create_traffic_matching_list(&sid, &body).await?;
            Ok(CommandResult::TrafficMatchingList(created.into()))
        }
        Command::UpdateTrafficMatchingList { id, update } => {
            let (ic, sid) = require_integration(integration, site_id, "UpdateTrafficMatchingList")?;
            let uuid = require_uuid(&id)?;
            let existing = ic.get_traffic_matching_list(&sid, &uuid).await?;
            let mut fields = serde_json::Map::new();
            let entries = if let Some(raw_items) = update.raw_items.as_deref() {
                serde_json::Value::Array(raw_items.to_vec())
            } else if let Some(new_entries) = &update.entries {
                serde_json::Value::Array(traffic_matching_list_items(new_entries, None))
            } else if let Some(existing_entries) = existing.extra.get("items") {
                existing_entries.clone()
            } else if let Some(existing_entries) = existing.extra.get("entries") {
                existing_entries.clone()
            } else {
                serde_json::Value::Array(Vec::new())
            };
            fields.insert("items".into(), entries);
            if let Some(desc) = update.description {
                fields.insert("description".into(), serde_json::Value::String(desc));
            } else if let Some(existing_desc) = existing.extra.get("description") {
                fields.insert("description".into(), existing_desc.clone());
            }
            let body = crate::integration_types::TrafficMatchingListCreateUpdate {
                name: update.name.unwrap_or(existing.name),
                list_type: existing.list_type,
                fields,
            };
            ic.update_traffic_matching_list(&sid, &uuid, &body).await?;
            Ok(CommandResult::Ok)
        }
        Command::DeleteTrafficMatchingList { id } => {
            let (ic, sid) = require_integration(integration, site_id, "DeleteTrafficMatchingList")?;
            let uuid = require_uuid(&id)?;
            ic.delete_traffic_matching_list(&sid, &uuid).await?;
            Ok(CommandResult::Ok)
        }
        _ => unreachable!("traffic_lists::route received non-traffic-list command"),
    }
}
