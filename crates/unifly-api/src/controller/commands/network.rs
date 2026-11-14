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
            reject_settings_the_network_cannot_take(&existing.management, &update)?;
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
            match update.ipv6_enabled {
                // IPv6 is on when the network has an IPv6 configuration.
                // One cannot be made up here: it needs an interface type,
                // a client address assignment, and for prefix delegation
                // the WAN interface the prefix comes from.
                Some(true)
                    if extra
                        .get("ipv6Configuration")
                        .is_none_or(serde_json::Value::is_null) =>
                {
                    return Err(CoreError::ValidationFailed {
                        message: "IPv6 cannot be turned on from here: it needs settings \
                                  (address assignment and a prefix source) that only the \
                                  controller's own interface collects"
                            .into(),
                    });
                }
                Some(false) => {
                    extra.remove("ipv6Configuration");
                }
                Some(true) | None => {}
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
        Command::DeleteNetwork { id, force } => {
            let (ic, sid) = require_integration(integration, site_id, "DeleteNetwork")?;
            let uuid = require_uuid(&id)?;
            ic.delete_network(&sid, &uuid, force).await?;
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

/// Refuse settings the network's kind has no field for. The controller
/// keeps isolation for gateway- and switch-managed networks only, and
/// internet access, mDNS forwarding, and IPv6 for gateway-managed ones.
/// Sent anyway they are dropped or the whole update is rejected.
fn reject_settings_the_network_cannot_take(
    management: &str,
    update: &crate::command::UpdateNetworkRequest,
) -> Result<(), CoreError> {
    let gateway_only = [
        ("internet access", update.internet_access_enabled.is_some()),
        ("mDNS forwarding", update.mdns_forwarding_enabled.is_some()),
        ("IPv6", update.ipv6_enabled.is_some()),
    ];
    let refused = match management {
        "SWITCH" => gateway_only
            .iter()
            .find(|(_, named)| *named)
            .map(|(setting, _)| (*setting, "a switch-managed")),
        "UNMANAGED" => std::iter::once(&("isolation", update.isolation_enabled.is_some()))
            .chain(&gateway_only)
            .find(|(_, named)| *named)
            .map(|(setting, _)| (*setting, "an unmanaged (VLAN-only)")),
        // A gateway-managed network takes them all, and a kind this
        // version does not know is the controller's to judge.
        _ => None,
    };
    match refused {
        Some((setting, kind)) => Err(CoreError::ValidationFailed {
            message: format!("{kind} network has no {setting} setting"),
        }),
        None => Ok(()),
    }
}
