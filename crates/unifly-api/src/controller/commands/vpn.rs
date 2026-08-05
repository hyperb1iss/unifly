use crate::command::{Command, CommandResult};
use crate::core_error::CoreError;
use crate::model::EntityId;
use serde::Serialize;
use serde_json::json;

use super::{CommandContext, require_session};

fn require_legacy_id(id: &EntityId) -> Result<&str, CoreError> {
    match id {
        EntityId::Legacy(id) => Ok(id),
        EntityId::Uuid(_) => Err(CoreError::ValidationFailed {
            message: "VPN mutations require a legacy string id".into(),
        }),
    }
}

fn serialize_body<T: Serialize>(value: T, label: &str) -> Result<serde_json::Value, CoreError> {
    serde_json::to_value(value).map_err(|error| CoreError::ValidationFailed {
        message: format!("invalid {label} payload: {error}"),
    })
}

/// Wrap the first created session record (secrets redacted) as a result.
fn created_record_result(records: &[serde_json::Value]) -> CommandResult {
    records.first().map_or(CommandResult::Ok, |record| {
        CommandResult::Json(
            crate::controller::session_queries::common::redact_sensitive_value(record),
        )
    })
}

pub(super) async fn route(ctx: &CommandContext, cmd: Command) -> Result<CommandResult, CoreError> {
    let session = ctx.session.as_ref();

    match cmd {
        Command::CreateSiteToSiteVpn(req) => {
            let session = require_session(session)?;
            let body = serialize_body(req, "site-to-site VPN")?;
            let created = session.create_network_conf(&body).await?;
            Ok(created_record_result(&created))
        }
        Command::UpdateSiteToSiteVpn { id, update } => {
            let session = require_session(session)?;
            let body = serialize_body(update, "site-to-site VPN")?;
            session
                .update_network_conf(require_legacy_id(&id)?, &body)
                .await?;
            Ok(CommandResult::Ok)
        }
        Command::CreateRemoteAccessVpnServer(req) => {
            let session = require_session(session)?;
            let body = serialize_body(req, "remote-access VPN server")?;
            let created = session.create_network_conf(&body).await?;
            Ok(created_record_result(&created))
        }
        Command::UpdateRemoteAccessVpnServer { id, update } => {
            let session = require_session(session)?;
            let body = serialize_body(update, "remote-access VPN server")?;
            session
                .update_network_conf(require_legacy_id(&id)?, &body)
                .await?;
            Ok(CommandResult::Ok)
        }
        Command::CreateVpnClientProfile(req) => {
            let session = require_session(session)?;
            let body = serialize_body(req, "VPN client profile")?;
            let created = session.create_network_conf(&body).await?;
            Ok(created_record_result(&created))
        }
        Command::UpdateVpnClientProfile { id, update } => {
            let session = require_session(session)?;
            let body = serialize_body(update, "VPN client profile")?;
            session
                .update_network_conf(require_legacy_id(&id)?, &body)
                .await?;
            Ok(CommandResult::Ok)
        }
        Command::CreateWireGuardPeer { server_id, peer } => {
            let session = require_session(session)?;
            let body = serialize_body(vec![peer], "WireGuard peer")?;
            let created = session
                .create_wireguard_peers(require_legacy_id(&server_id)?, &body)
                .await?;
            // The batch endpoint answers with an array of created peers;
            // unwrap it so the caller gets the peer record, not the envelope.
            Ok(match created {
                serde_json::Value::Array(items) => created_record_result(&items),
                other => created_record_result(&[other]),
            })
        }
        Command::UpdateWireGuardPeer {
            server_id,
            peer_id,
            update,
        } => {
            let session = require_session(session)?;
            let mut body = serialize_body(update, "WireGuard peer")?;
            body.as_object_mut()
                .ok_or_else(|| CoreError::ValidationFailed {
                    message: "invalid WireGuard peer payload: expected an object".into(),
                })?
                .insert("_id".into(), json!(require_legacy_id(&peer_id)?));
            session
                .update_wireguard_peers(require_legacy_id(&server_id)?, &json!([body]))
                .await?;
            Ok(CommandResult::Ok)
        }
        Command::DeleteSiteToSiteVpn { id }
        | Command::DeleteRemoteAccessVpnServer { id }
        | Command::DeleteVpnClientProfile { id } => {
            let session = require_session(session)?;
            session.delete_network_conf(require_legacy_id(&id)?).await?;
            Ok(CommandResult::Ok)
        }
        Command::DeleteWireGuardPeer { server_id, peer_id } => {
            let session = require_session(session)?;
            session
                .delete_wireguard_peers(
                    require_legacy_id(&server_id)?,
                    &json!([require_legacy_id(&peer_id)?]),
                )
                .await?;
            Ok(CommandResult::Ok)
        }
        Command::RestartVpnClientConnection { id } => {
            let session = require_session(session)?;
            session
                .restart_vpn_client_connection(require_legacy_id(&id)?)
                .await?;
            Ok(CommandResult::Ok)
        }
        _ => unreachable!("vpn::route received non-VPN command"),
    }
}
