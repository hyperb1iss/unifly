use crate::command::Command;
use crate::core_error::CoreError;

use super::{
    CommandContext, apply_ipv4_update, build_create_network_payload,
    build_create_wifi_broadcast_payload, build_update_wifi_broadcast_payload, require_integration,
    require_uuid, wifi_uses_preshared_keys,
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
                // Turned on here it takes what the controller's own form
                // starts from: a prefix delegated from the WAN, handed out
                // by SLAAC, with the gateway advertised as the router.
                Some(true)
                    if extra
                        .get("ipv6Configuration")
                        .is_none_or(serde_json::Value::is_null) =>
                {
                    let wan = delegating_wan(&ctx.store, ic, &sid).await?;
                    extra.insert(
                        "ipv6Configuration".into(),
                        serde_json::json!({
                            "interfaceType": "PREFIX_DELEGATION",
                            "prefixDelegationWanInterfaceId": wan.to_string(),
                            "clientAddressAssignment": { "slaacEnabled": true },
                            "routerAdvertisement": { "priority": "HIGH" },
                        }),
                    );
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
            // One passphrase written beside per-network keys is ambiguous:
            // the controller could reject it, ignore it, or drop the keys.
            if update.passphrase.is_some() && wifi_uses_preshared_keys(&existing) {
                return Err(CoreError::ValidationFailed {
                    message: format!(
                        "{} gives each network its own preshared key, so it has no single \
                         passphrase to change; edit its keys in the UniFi console",
                        existing.name
                    ),
                });
            }
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

/// The WAN a network given IPv6 here takes its prefix from: the one the
/// site's other delegated networks use, else the first the controller
/// lists, which is its primary.
async fn delegating_wan(
    store: &crate::store::DataStore,
    integration: &crate::IntegrationClient,
    site_id: &uuid::Uuid,
) -> Result<uuid::Uuid, CoreError> {
    if let Some(id) = wan_in_use(&store.networks_snapshot()) {
        return Ok(id);
    }
    let wans = integration.list_wans(site_id, 0, 25).await?;
    wans.data
        .iter()
        .find_map(|wan| wan.fields.get("id")?.as_str()?.parse().ok())
        .ok_or_else(|| CoreError::ValidationFailed {
            message: "IPv6 needs a WAN to delegate a prefix from, and the controller lists none"
                .into(),
        })
}

/// The WAN the site's delegated networks take their prefix from.
fn wan_in_use(networks: &[std::sync::Arc<crate::model::Network>]) -> Option<uuid::Uuid> {
    networks
        .iter()
        .find_map(|network| match &network.ipv6_wan_id {
            Some(crate::model::EntityId::Uuid(id)) => Some(*id),
            _ => None,
        })
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::model::Network;

    fn network(name: &str, wan: Option<&str>) -> Arc<Network> {
        let mut record = serde_json::json!({
            "id": "8a1e5f3c-2b4d-4e6f-9a0b-1c2d3e4f5a01", "name": name, "enabled": true,
            "management": "GATEWAY", "vlanId": 30, "metadata": {"origin": "USER_DEFINED"},
        });
        if let Some(wan) = wan {
            record["ipv6Configuration"] = serde_json::json!({
                "interfaceType": "PREFIX_DELEGATION",
                "prefixDelegationWanInterfaceId": wan,
                "clientAddressAssignment": {"slaacEnabled": true},
            });
        }
        let details: crate::integration_types::NetworkDetailsResponse =
            serde_json::from_value(record).expect("network");
        Arc::new(Network::from(details))
    }

    #[test]
    fn ipv6_comes_from_the_wan_the_site_already_delegates_from() {
        let second = "6b1e5f3c-2b4d-4e6f-9a0b-1c2d3e4f5a66";
        let networks = [network("Guest", None), network("Home", Some(second))];
        assert_eq!(super::wan_in_use(&networks), second.parse().ok());
        assert_eq!(super::wan_in_use(&networks[..1]), None);
    }
}
