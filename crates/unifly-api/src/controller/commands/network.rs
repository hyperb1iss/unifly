use std::collections::HashMap;

use crate::command::Command;
use crate::core_error::CoreError;
use crate::model::{NetworkManagement, NetworkPurpose};

use super::{
    CommandContext, build_create_wifi_broadcast_payload, build_update_wifi_broadcast_payload,
    parse_ipv4_cidr, require_integration, require_uuid,
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
            let crate::command::CreateNetworkRequest {
                name,
                vlan_id,
                subnet,
                management,
                purpose,
                dhcp_enabled,
                enabled,
                dhcp_range_start,
                dhcp_range_stop,
                dhcp_lease_time,
                dns_servers,
                firewall_zone_id,
                isolation_enabled,
                internet_access_enabled,
            } = req;

            let management = management.unwrap_or_else(|| {
                if matches!(purpose, Some(NetworkPurpose::VlanOnly)) {
                    NetworkManagement::Unmanaged
                } else if purpose.is_some() || subnet.is_some() || dhcp_enabled {
                    NetworkManagement::Gateway
                } else {
                    NetworkManagement::Unmanaged
                }
            });
            let mut extra = HashMap::new();

            if let Some(zone) = firewall_zone_id {
                extra.insert("zoneId".into(), serde_json::Value::String(zone));
            }

            if matches!(management, NetworkManagement::Gateway) {
                extra.insert(
                    "isolationEnabled".into(),
                    serde_json::Value::Bool(isolation_enabled),
                );
                extra.insert(
                    "internetAccessEnabled".into(),
                    serde_json::Value::Bool(internet_access_enabled),
                );
                extra.insert(
                    "mdnsForwardingEnabled".into(),
                    serde_json::Value::Bool(false),
                );
                extra.insert(
                    "cellularBackupEnabled".into(),
                    serde_json::Value::Bool(false),
                );

                if let Some(cidr) = subnet {
                    let (host_ip, prefix_len) = parse_ipv4_cidr(&cidr)?;
                    let lease_secs = dhcp_lease_time.unwrap_or(86400);
                    let mut dhcp_cfg = serde_json::Map::new();
                    dhcp_cfg.insert(
                        "mode".into(),
                        serde_json::Value::String(
                            if dhcp_enabled { "SERVER" } else { "NONE" }.into(),
                        ),
                    );
                    dhcp_cfg.insert(
                        "leaseTimeSeconds".into(),
                        serde_json::Value::Number(serde_json::Number::from(u64::from(lease_secs))),
                    );
                    dhcp_cfg.insert(
                        "pingConflictDetectionEnabled".into(),
                        serde_json::Value::Bool(false),
                    );

                    if let (Some(start), Some(stop)) = (dhcp_range_start, dhcp_range_stop) {
                        dhcp_cfg.insert(
                            "ipAddressRange".into(),
                            serde_json::json!({
                                "start": start,
                                "stop": stop
                            }),
                        );
                    }

                    if let Some(ref servers) = dns_servers {
                        dhcp_cfg.insert(
                            "dnsServerIpAddressesOverride".into(),
                            serde_json::json!(servers),
                        );
                    }

                    extra.insert(
                        "ipv4Configuration".into(),
                        serde_json::json!({
                            "hostIpAddress": host_ip.to_string(),
                            "prefixLength": u64::from(prefix_len),
                            "autoScaleEnabled": false,
                            "dhcpConfiguration": dhcp_cfg
                        }),
                    );
                }
            }

            let body = crate::integration_types::NetworkCreateUpdate {
                name,
                enabled,
                management: match management {
                    NetworkManagement::Gateway => "GATEWAY",
                    NetworkManagement::Switch => "SWITCH",
                    NetworkManagement::Unmanaged => "UNMANAGED",
                }
                .into(),
                vlan_id: vlan_id.map_or(1, i32::from),
                dhcp_guarding: None,
                extra,
            };
            let created = ic.create_network(&sid, &body).await?;
            Ok(crate::command::CommandResult::Network(created.into()))
        }
        Command::UpdateNetwork { id, update } => {
            let (ic, sid) = require_integration(integration, site_id, "UpdateNetwork")?;
            let uuid = require_uuid(&id)?;
            let existing = ic.get_network(&sid, &uuid).await?;
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
            if let Some(ref dhcp_update) = update.dhcp
                && let Some(ref servers) = dhcp_update.dns_servers
                && let Some(ipv4_cfg) = extra.get_mut("ipv4Configuration")
                && let Some(dhcp_cfg) = ipv4_cfg.get_mut("dhcpConfiguration")
            {
                dhcp_cfg["dnsServerIpAddressesOverride"] = serde_json::json!(servers);
            }
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
            let body = build_create_wifi_broadcast_payload(&req);
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
