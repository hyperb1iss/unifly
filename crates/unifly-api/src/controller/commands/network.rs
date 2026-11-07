use crate::command::Command;
use crate::core_error::CoreError;

use super::{
    CommandContext, apply_ipv4_update, build_create_network_payload,
    build_create_wifi_broadcast_payload, build_update_wifi_broadcast_payload, require_integration,
    require_uuid,
};

#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub(super) async fn route(
    ctx: &CommandContext,
    cmd: Command,
) -> Result<crate::command::CommandResult, CoreError> {
    let integration = ctx.integration.as_ref();
    let site_id = ctx.site_id;

    match cmd {
        Command::CreateNetwork(req) => {
            let (ic, sid) = require_integration(integration, site_id, "CreateNetwork")?;
            let body = build_create_network_payload(req)?;
            let created = ic.create_network(&sid, &body).await?;
            Ok(crate::command::CommandResult::Network(created.into()))
        }
        Command::UpdateNetwork { id, update } => {
            let (ic, sid) = require_integration(integration, site_id, "UpdateNetwork")?;
            let uuid = require_uuid(&id)?;
            let existing = ic.get_network(&sid, &uuid).await?;
            let dhcp_serving = crate::model::Network::from(existing.clone())
                .dhcp
                .is_some_and(|dhcp| dhcp.enabled);
            let mut extra = existing.extra;
            if let Some(v) = update.isolation_enabled {
                extra.insert("isolationEnabled".into(), serde_json::Value::Bool(v));
            }
            if let Some(v) = update.internet_access_enabled {
                extra.insert("internetAccessEnabled".into(), serde_json::Value::Bool(v));
            }
            if let Some(v) = update.mdns_forwarding_enabled {
                extra.insert("mdnsForwardingEnabled".into(), serde_json::Value::Bool(v));
            }
            if let Some(v) = update.ipv6_enabled {
                if v {
                    extra
                        .entry("ipv6Configuration".into())
                        .or_insert_with(|| serde_json::json!({ "type": "PREFIX_DELEGATION" }));
                } else {
                    extra.remove("ipv6Configuration");
                }
            }
            apply_ipv4_update(&mut extra, &existing.management, dhcp_serving, &update)?;
            let body = crate::integration_types::NetworkCreateUpdate {
                name: update.name.unwrap_or(existing.name),
                enabled: update.enabled.unwrap_or(existing.enabled),
                management: existing.management,
                vlan_id: update.vlan_id.map_or(existing.vlan_id, i32::from),
                dhcp_guarding: existing.dhcp_guarding,
                extra,
            };
            ic.update_network(&sid, &uuid, &body).await?;
            Ok(crate::command::CommandResult::Ok)
        }
        Command::DeleteNetwork { id, force: _ } => {
            let (ic, sid) = require_integration(integration, site_id, "DeleteNetwork")?;
            let uuid = require_uuid(&id)?;
            ic.delete_network(&sid, &uuid).await?;
            Ok(crate::command::CommandResult::Ok)
        }
        Command::CreateWifiBroadcast(req) => {
            let (ic, sid) = require_integration(integration, site_id, "CreateWifiBroadcast")?;
            let body = build_create_wifi_broadcast_payload(&req)?;
            let created = ic.create_wifi_broadcast(&sid, &body).await?;
            Ok(crate::command::CommandResult::WifiBroadcast(created.into()))
        }
        Command::UpdateWifiBroadcast { id, update } => {
            let (ic, sid) = require_integration(integration, site_id, "UpdateWifiBroadcast")?;
            let uuid = require_uuid(&id)?;
            let existing = ic.get_wifi_broadcast(&sid, &uuid).await?;
            let payload = build_update_wifi_broadcast_payload(&existing, &update);
            ic.update_wifi_broadcast(&sid, &uuid, &payload).await?;
            Ok(crate::command::CommandResult::Ok)
        }
        Command::DeleteWifiBroadcast { id, force: _ } => {
            let (ic, sid) = require_integration(integration, site_id, "DeleteWifiBroadcast")?;
            let uuid = require_uuid(&id)?;
            ic.delete_wifi_broadcast(&sid, &uuid).await?;
            Ok(crate::command::CommandResult::Ok)
        }
        _ => unreachable!("network::route received non-network command"),
    }
}
