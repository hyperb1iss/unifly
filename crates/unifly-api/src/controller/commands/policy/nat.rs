use crate::command::requests::UpdateNatPolicyRequest;
use serde_json::json;
use tracing::debug;

use crate::command::{Command, CommandResult};
use crate::core_error::CoreError;
use crate::model::EntityId;
use crate::session::SessionClient;

use super::super::{CommandContext, require_session};

pub(super) async fn route(ctx: &CommandContext, cmd: Command) -> Result<CommandResult, CoreError> {
    let session = ctx.session.as_ref();

    match cmd {
        Command::CreateNatPolicy(req) => {
            let session = require_session(session)?;

            let nat_type = match req.nat_type.to_lowercase().as_str() {
                "masquerade" => "MASQUERADE",
                "source" | "source_nat" | "snat" => "SNAT",
                _ => "DNAT",
            };

            let protocol = req
                .protocol
                .as_deref()
                .map(|p| match p.to_lowercase().as_str() {
                    "tcp" => "tcp",
                    "udp" => "udp",
                    "tcp_udp" | "tcp_and_udp" => "tcp_udp",
                    _ => "all",
                });

            // Determine the next available rule_index from existing rules.
            let rule_index = next_rule_index(session).await?;

            // Build v2 API body matching the controller's expected format
            let mut body = json!({
                "description": req.name,
                "enabled": req.enabled,
                "type": nat_type,
                "ip_version": "IPV4",
                "is_predefined": false,
                "rule_index": rule_index,
                "setting_preference": "manual",
                "logging": false,
                "exclude": false,
                "pppoe_use_base_interface": false,
            });

            if let Some(proto) = protocol {
                body["protocol"] = json!(proto);
            }

            // Translated address — DNAT/SNAT use ip_address, masquerade
            // uses the interface's own IP automatically.
            if let Some(addr) = &req.translated_address {
                body["ip_address"] = json!(addr);
            }

            // Translated port (top-level "port" in v2 schema)
            if let Some(port) = &req.translated_port {
                body["port"] = json!(port);
            }

            // DNAT matches traffic entering an interface (in_interface);
            // SNAT/masquerade matches traffic leaving one (out_interface).
            if let Some(iface) = &req.interface_id {
                let session_id = resolve_interface_id(session, iface).await?;
                let key = if nat_type == "DNAT" {
                    "in_interface"
                } else {
                    "out_interface"
                };
                body[key] = json!(session_id);
            }

            // Source filter
            body["source_filter"] =
                build_filter(req.src_address.as_deref(), req.src_port.as_deref());

            // Destination filter
            body["destination_filter"] =
                build_filter(req.dst_address.as_deref(), req.dst_port.as_deref());

            session.create_nat_rule(&body).await?;
            Ok(CommandResult::Ok)
        }
        Command::UpdateNatPolicy { id, update } => {
            let session = require_session(session)?;
            apply_nat_update(session, &id, update).await?;
            Ok(CommandResult::Ok)
        }
        Command::DeleteNatPolicy { id } => {
            let session = require_session(session)?;
            session.delete_nat_rule(&id.to_string()).await?;
            Ok(CommandResult::Ok)
        }
        _ => unreachable!("nat::route received non-NAT command"),
    }
}

/// Fetch the existing NAT rule, merge updated fields, and PUT it back.
async fn apply_nat_update(
    session: &SessionClient,
    id: &EntityId,
    update: UpdateNatPolicyRequest,
) -> Result<(), CoreError> {
    let rule_id = id.to_string();

    let rules = session.list_nat_rules().await.map_err(CoreError::from)?;
    let existing = rules
        .iter()
        .find(|r| r.get("_id").and_then(serde_json::Value::as_str) == Some(&rule_id))
        .ok_or_else(|| CoreError::NotFound {
            entity_type: "NAT rule".into(),
            identifier: rule_id.clone(),
        })?
        .clone();

    let mut body = existing;

    if let Some(name) = &update.name {
        body["description"] = json!(name);
    }
    if let Some(enabled) = update.enabled {
        body["enabled"] = json!(enabled);
    }
    if let Some(desc) = &update.description {
        body["description"] = json!(desc);
    }
    if let Some(ref nat_type) = update.nat_type {
        let lowered = nat_type.to_lowercase();
        let mapped = match lowered.as_str() {
            "masquerade" => "MASQUERADE",
            "source" | "source_nat" | "snat" => "SNAT",
            _ => "DNAT",
        };
        // When changing direction (DNAT <-> SNAT/MASQUERADE), clear the
        // stale interface key so the controller doesn't receive both
        // in_interface and out_interface simultaneously.
        let old_type = body["type"].as_str().unwrap_or("");
        let was_dnat = old_type == "DNAT";
        let is_dnat = mapped == "DNAT";
        if was_dnat != is_dnat {
            let stale_key = if was_dnat {
                "in_interface"
            } else {
                "out_interface"
            };
            if let Some(m) = body.as_object_mut() {
                m.remove(stale_key);
            }
        }
        body["type"] = json!(mapped);
    }
    if let Some(ref protocol) = update.protocol {
        let lowered = protocol.to_lowercase();
        let mapped = match lowered.as_str() {
            "tcp" => "tcp",
            "udp" => "udp",
            "tcp_udp" | "tcp_and_udp" => "tcp_udp",
            _ => "all",
        };
        body["protocol"] = json!(mapped);
    }
    if let Some(iface) = &update.interface_id {
        let session_id = resolve_interface_id(session, iface).await?;
        let nat_type_str = body["type"].as_str().unwrap_or("DNAT");
        let key = if nat_type_str == "DNAT" {
            "in_interface"
        } else {
            "out_interface"
        };
        body[key] = json!(session_id);
    }
    if let Some(addr) = &update.translated_address {
        body["ip_address"] = json!(addr);
    }
    if let Some(port) = &update.translated_port {
        body["port"] = json!(port);
    }

    if update.src_address.is_some() || update.src_port.is_some() {
        body["source_filter"] = merge_filter(
            body.get("source_filter"),
            update.src_address.as_deref(),
            update.src_port.as_deref(),
        );
    }
    if update.dst_address.is_some() || update.dst_port.is_some() {
        body["destination_filter"] = merge_filter(
            body.get("destination_filter"),
            update.dst_address.as_deref(),
            update.dst_port.as_deref(),
        );
    }

    session
        .update_nat_rule(&rule_id, &body)
        .await
        .map_err(CoreError::from)?;
    Ok(())
}

/// Merge new address/port values with an existing filter, preserving fields
/// that were not explicitly supplied in the update.
fn merge_filter(
    existing: Option<&serde_json::Value>,
    new_addr: Option<&str>,
    new_port: Option<&str>,
) -> serde_json::Value {
    let existing_addr = existing
        .and_then(|f| f.get("address"))
        .and_then(serde_json::Value::as_str);
    let existing_port = existing
        .and_then(|f| f.get("port"))
        .and_then(serde_json::Value::as_str);
    build_filter(new_addr.or(existing_addr), new_port.or(existing_port))
}

/// Build a v2 NAT filter object (source_filter or destination_filter).
///
/// The UniFi v2 API only accepts `NONE` and `ADDRESS_AND_PORT` as
/// `filter_type` values.  `ADDRESS` and `PORT` alone are rejected with a
/// deserialization error, so we always use `ADDRESS_AND_PORT` when either
/// field is present and include only the fields that were supplied.
fn build_filter(address: Option<&str>, port: Option<&str>) -> serde_json::Value {
    if address.is_none() && port.is_none() {
        return json!({
            "filter_type": "NONE",
            "firewall_group_ids": [],
            "invert_address": false,
            "invert_port": false,
        });
    }

    let mut filter = json!({
        "filter_type": "ADDRESS_AND_PORT",
        "firewall_group_ids": [],
        "invert_address": false,
        "invert_port": false,
    });

    if let Some(addr) = address {
        filter["address"] = json!(addr);
    }
    if let Some(p) = port {
        filter["port"] = json!(p);
    }

    filter
}

/// Determine the next available `rule_index` by querying existing NAT rules.
async fn next_rule_index(session: &SessionClient) -> Result<u64, CoreError> {
    let rules = session.list_nat_rules().await.map_err(CoreError::from)?;
    let max_idx = rules
        .iter()
        .filter_map(|r| r.get("rule_index").and_then(serde_json::Value::as_u64))
        .max()
        .unwrap_or(0);
    Ok(max_idx + 1)
}

/// Resolve an interface ID for the v2 NAT API.
///
/// The NAT v2 API expects Session API `_id` strings (hex) for
/// `in_interface` / `out_interface`, but users provide Integration API
/// UUIDs (from `networks list`).  This function queries Session
/// `rest/networkconf` and matches on the `external_id` field to find
/// the corresponding Session `_id`.  If the provided ID is already a
/// legacy (non-UUID) string, it is passed through as-is.
async fn resolve_interface_id(
    session: &SessionClient,
    iface: &EntityId,
) -> Result<String, CoreError> {
    match iface {
        // Already a Session-style hex ID — pass through.
        EntityId::Legacy(id) => Ok(id.clone()),
        // Integration UUID — look up the matching Session _id.
        EntityId::Uuid(uuid) => {
            let uuid_str = uuid.to_string();
            debug!(uuid = %uuid_str, "resolving Integration network UUID to Session _id");
            let records = session.list_network_conf().await.map_err(CoreError::from)?;
            for record in &records {
                if record
                    .get("external_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(&uuid_str)
                    && let Some(id) = record.get("_id").and_then(serde_json::Value::as_str)
                {
                    debug!(session_id = id, "resolved interface ID");
                    return Ok(id.to_owned());
                }
            }
            Err(CoreError::NotFound {
                entity_type: "network".into(),
                identifier: uuid_str,
            })
        }
    }
}
