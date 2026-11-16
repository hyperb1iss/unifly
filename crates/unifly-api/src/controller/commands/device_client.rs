use crate::command::{Command, CommandResult};
use crate::core_error::CoreError;
use crate::model::MacAddress;

use super::{
    CommandContext, client_mac, device_mac, fixed_ip, require_integration, require_session,
    require_uuid,
};

/// Run a client action through the Integration API, for a connection
/// with no Session API. The station manager is preferred where there is
/// one: it takes a MAC, so it also reaches a blocked client that has left
/// the connected list, and the Integration API documents only the guest
/// authorization actions.
async fn integration_client_action(
    ctx: &CommandContext,
    mac: &MacAddress,
    action: &str,
) -> Result<(), CoreError> {
    let (integration, site_id) =
        require_integration(ctx.integration.as_ref(), ctx.site_id, action)?;
    let client = ctx
        .store
        .client_by_mac(mac)
        .ok_or_else(|| CoreError::ClientNotFound {
            identifier: mac.to_string(),
        })?;
    let client_uuid = require_uuid(&client.id)?;
    integration
        .client_action(&site_id, &client_uuid, action)
        .await?;
    Ok(())
}

#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub(super) async fn route(ctx: &CommandContext, cmd: Command) -> Result<CommandResult, CoreError> {
    let store = ctx.store.as_ref();
    let integration = ctx.integration.as_ref();
    let session = ctx.session.as_ref();
    let site_id = ctx.site_id;

    match cmd {
        Command::AdoptDevice {
            mac,
            ignore_device_limit,
        } => {
            if let (Some(ic), Some(sid)) = (integration, site_id) {
                ic.adopt_device(&sid, mac.as_str(), ignore_device_limit)
                    .await?;
            } else {
                let session = require_session(session)?;
                session.adopt_device(mac.as_str()).await?;
            }
            Ok(CommandResult::Ok)
        }
        Command::RestartDevice { id } => {
            if let (Some(ic), Some(sid)) = (integration, site_id) {
                let device_uuid = require_uuid(&id)?;
                ic.device_action(&sid, &device_uuid, "RESTART").await?;
            } else {
                let session = require_session(session)?;
                let mac = device_mac(store, &id)?;
                session.restart_device(mac.as_str()).await?;
            }
            Ok(CommandResult::Ok)
        }
        Command::LocateDevice { mac, enable } => {
            if let (Some(ic), Some(sid)) = (integration, site_id) {
                let device =
                    store
                        .device_by_mac(&mac)
                        .ok_or_else(|| CoreError::DeviceNotFound {
                            identifier: mac.to_string(),
                        })?;
                let device_uuid = require_uuid(&device.id)?;
                let action = if enable { "LOCATE_ON" } else { "LOCATE_OFF" };
                ic.device_action(&sid, &device_uuid, action).await?;
            } else {
                let session = require_session(session)?;
                session.locate_device(mac.as_str(), enable).await?;
            }
            Ok(CommandResult::Ok)
        }
        Command::UpgradeDevice { mac, firmware_url } => {
            let session = require_session(session)?;
            session
                .upgrade_device(mac.as_str(), firmware_url.as_deref())
                .await?;
            Ok(CommandResult::Ok)
        }
        Command::RemoveDevice { id } => {
            let (ic, sid) = require_integration(integration, site_id, "RemoveDevice")?;
            let device_uuid = require_uuid(&id)?;
            ic.remove_device(&sid, &device_uuid).await?;
            Ok(CommandResult::Ok)
        }
        Command::ProvisionDevice { mac } => {
            let session = require_session(session)?;
            session.provision_device(mac.as_str()).await?;
            Ok(CommandResult::Ok)
        }
        Command::SpeedtestDevice => {
            let session = require_session(session)?;
            session.speedtest().await?;
            Ok(CommandResult::Ok)
        }
        Command::PowerCyclePort {
            device_id,
            port_idx,
        } => {
            let (ic, sid) = require_integration(integration, site_id, "PowerCyclePort")?;
            let device_uuid = require_uuid(&device_id)?;
            ic.port_action(&sid, &device_uuid, port_idx, "POWER_CYCLE")
                .await?;
            Ok(CommandResult::Ok)
        }
        Command::BlockClient { mac } => {
            if let Some(session) = session {
                session.block_client(mac.as_str()).await?;
            } else {
                integration_client_action(ctx, &mac, "BLOCK").await?;
            }
            Ok(CommandResult::Ok)
        }
        Command::UnblockClient { mac } => {
            if let Some(session) = session {
                session.unblock_client(mac.as_str()).await?;
            } else {
                integration_client_action(ctx, &mac, "UNBLOCK").await?;
            }
            Ok(CommandResult::Ok)
        }
        Command::KickClient { mac } => {
            if let Some(session) = session {
                session.kick_client(mac.as_str()).await?;
            } else {
                integration_client_action(ctx, &mac, "RECONNECT").await?;
            }
            Ok(CommandResult::Ok)
        }
        Command::ForgetClient { mac } => {
            let session = require_session(session)?;
            session.forget_client(mac.as_str()).await?;
            Ok(CommandResult::Ok)
        }
        Command::AuthorizeGuest {
            client_id,
            time_limit_minutes,
            data_limit_mb,
            rx_rate_kbps,
            tx_rate_kbps,
        } => {
            let session = require_session(session)?;
            let mac = client_mac(store, &client_id)?;
            let minutes = time_limit_minutes.unwrap_or(60);
            #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
            {
                session
                    .authorize_guest(
                        mac.as_str(),
                        minutes,
                        tx_rate_kbps.map(|r| r as u32),
                        rx_rate_kbps.map(|r| r as u32),
                        data_limit_mb.map(|m| m as u32),
                    )
                    .await?;
            }
            Ok(CommandResult::Ok)
        }
        Command::UnauthorizeGuest { client_id } => {
            let session = require_session(session)?;
            let mac = client_mac(store, &client_id)?;
            session.unauthorize_guest(mac.as_str()).await?;
            Ok(CommandResult::Ok)
        }
        Command::SetClientFixedIp {
            mac,
            ip,
            network_id,
        } => {
            let session = require_session(session)?;
            let records = session.list_network_conf().await?;
            let network = fixed_ip::reservation_network(&records, network_id.as_ref(), ip)?;
            session
                .set_client_fixed_ip(mac.as_str(), &ip.to_string(), &network)
                .await?;
            Ok(CommandResult::Ok)
        }
        Command::RemoveClientFixedIp { mac, network_id } => {
            let session = require_session(session)?;
            let network = match &network_id {
                Some(wanted) => {
                    let records = session.list_network_conf().await?;
                    Some(fixed_ip::named_network(&records, wanted)?)
                }
                None => None,
            };
            session
                .remove_client_fixed_ip(mac.as_str(), network.as_deref())
                .await?;
            Ok(CommandResult::Ok)
        }
        _ => unreachable!("device_client::route received non-device/client command"),
    }
}
