//! VPN command handlers.

mod render;

use std::path::PathBuf;

use unifly_api::{Command as CoreCommand, Controller, EntityId};

use crate::cli::args::{
    GlobalOpts, MagicSiteToSiteVpnArgs, MagicSiteToSiteVpnCommand, OutputFormat,
    RemoteAccessVpnArgs, RemoteAccessVpnCommand, SiteToSiteVpnArgs, SiteToSiteVpnCommand, VpnArgs,
    VpnClientsArgs, VpnClientsCommand, VpnCommand, VpnConnectionsArgs, VpnConnectionsCommand,
    VpnPeersArgs, VpnPeersCommand, VpnServersArgs, VpnServersCommand, VpnSettingsArgs,
    VpnSettingsCommand, VpnTunnelsArgs, VpnTunnelsCommand,
};
use crate::cli::error::CliError;
use crate::cli::output;

use self::render::{
    ipsec_sa_identity, ipsec_sa_row, magic_site_to_site_vpn_config_detail,
    magic_site_to_site_vpn_config_row, openvpn_port_row, remote_access_vpn_server_detail,
    remote_access_vpn_server_row, server_detail, site_to_site_vpn_detail, site_to_site_vpn_row,
    tunnel_detail, vpn_client_connection_detail, vpn_client_connection_row,
    vpn_client_profile_detail, vpn_client_profile_row, vpn_health_detail, vpn_server_row,
    vpn_setting_detail, vpn_setting_key_name, vpn_setting_patch_body, vpn_setting_row,
    vpn_tunnel_row, wireguard_peer_detail, wireguard_peer_row, wireguard_peer_subnet_row,
};

use super::util;

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: VpnArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    let painter = output::Painter::new(global);

    match args.command {
        VpnCommand::Servers(args) => handle_servers(controller, args, global, &painter).await,
        VpnCommand::Tunnels(args) => handle_tunnels(controller, args, global, &painter).await,
        VpnCommand::Status => {
            util::ensure_session_access(controller, "vpn status").await?;
            let sas = controller.list_ipsec_sa().await?;
            if sas.is_empty() {
                if !global.quiet && matches!(global.output, OutputFormat::Table) {
                    eprintln!("No active IPsec security associations");
                }
                if matches!(global.output, OutputFormat::Table) {
                    return Ok(());
                }
            }
            let out = output::render_list(
                &global.output,
                &sas,
                |sa| ipsec_sa_row(sa, &painter),
                ipsec_sa_identity,
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
        VpnCommand::Health => {
            util::ensure_session_access(controller, "vpn health").await?;
            match controller.get_vpn_health() {
                Some(health) => {
                    let out = output::render_single(
                        &global.output,
                        &health,
                        |health| vpn_health_detail(health, &painter),
                        |health| health.subsystem.clone(),
                    );
                    output::print_output(&out, global.quiet);
                    Ok(())
                }
                None => Err(CliError::NotFound {
                    resource_type: "vpn health".into(),
                    identifier: "vpn".into(),
                    list_command: "system health".into(),
                }),
            }
        }
        VpnCommand::SiteToSite(site_to_site) => {
            handle_site_to_site(controller, site_to_site, global, &painter).await
        }
        VpnCommand::RemoteAccess(remote_access) => {
            handle_remote_access(controller, remote_access, global, &painter).await
        }
        VpnCommand::Clients(clients) => handle_clients(controller, clients, global, &painter).await,
        VpnCommand::Connections(connections) => {
            handle_connections(controller, connections, global, &painter).await
        }
        VpnCommand::Peers(peers) => handle_peers(controller, peers, global, &painter).await,
        VpnCommand::MagicSiteToSite(magic_site_to_site) => {
            handle_magic_site_to_site(controller, magic_site_to_site, global, &painter).await
        }
        VpnCommand::Settings(settings) => {
            handle_settings(controller, settings, global, &painter).await
        }
    }
}

async fn handle_servers(
    controller: &Controller,
    args: VpnServersArgs,
    global: &GlobalOpts,
    painter: &output::Painter,
) -> Result<(), CliError> {
    util::ensure_integration_access(controller, "vpn servers").await?;

    match args.command {
        Some(VpnServersCommand::Get { id }) => {
            let servers = controller.list_vpn_servers().await?;
            let target_id = EntityId::from(id.clone());
            let server = servers.iter().find(|server| server.id == target_id);
            match server {
                Some(server) => {
                    let out = output::render_single(
                        &global.output,
                        server,
                        |server| server_detail(server, painter),
                        |server| server.id.to_string(),
                    );
                    output::print_output(&out, global.quiet);
                    Ok(())
                }
                None => Err(CliError::NotFound {
                    resource_type: "vpn server".into(),
                    identifier: id,
                    list_command: "vpn servers".into(),
                }),
            }
        }
        None => {
            let servers = util::apply_list_args(
                controller.list_vpn_servers().await?,
                &args.list,
                util::matches_json_filter,
            );
            let out = output::render_list(
                &global.output,
                &servers,
                |server| vpn_server_row(server, painter),
                |server| server.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
    }
}

async fn handle_tunnels(
    controller: &Controller,
    args: VpnTunnelsArgs,
    global: &GlobalOpts,
    painter: &output::Painter,
) -> Result<(), CliError> {
    util::ensure_integration_access(controller, "vpn tunnels").await?;

    match args.command {
        Some(VpnTunnelsCommand::Get { id }) => {
            let tunnels = controller.list_vpn_tunnels().await?;
            let target_id = EntityId::from(id.clone());
            let tunnel = tunnels.iter().find(|tunnel| tunnel.id == target_id);
            match tunnel {
                Some(tunnel) => {
                    let out = output::render_single(
                        &global.output,
                        tunnel,
                        |tunnel| tunnel_detail(tunnel, painter),
                        |tunnel| tunnel.id.to_string(),
                    );
                    output::print_output(&out, global.quiet);
                    Ok(())
                }
                None => Err(CliError::NotFound {
                    resource_type: "vpn tunnel".into(),
                    identifier: id,
                    list_command: "vpn tunnels".into(),
                }),
            }
        }
        None => {
            let tunnels = util::apply_list_args(
                controller.list_vpn_tunnels().await?,
                &args.list,
                util::matches_json_filter,
            );
            let out = output::render_list(
                &global.output,
                &tunnels,
                |tunnel| vpn_tunnel_row(tunnel, painter),
                |tunnel| tunnel.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
    }
}

async fn handle_site_to_site(
    controller: &Controller,
    args: SiteToSiteVpnArgs,
    global: &GlobalOpts,
    painter: &output::Painter,
) -> Result<(), CliError> {
    util::ensure_session_access(controller, "site-to-site vpn").await?;

    match args.command {
        SiteToSiteVpnCommand::List(list) => {
            let vpns = util::apply_list_args(
                controller.list_site_to_site_vpns().await?,
                &list,
                util::matches_json_filter,
            );
            let out = output::render_list(
                &global.output,
                &vpns,
                |vpn| site_to_site_vpn_row(vpn, painter),
                |vpn| vpn.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
        SiteToSiteVpnCommand::Get { id } => {
            let vpn = controller.get_site_to_site_vpn(&id).await?;
            let out = output::render_single(&global.output, &vpn, site_to_site_vpn_detail, |vpn| {
                vpn.id.to_string()
            });
            output::print_output(&out, global.quiet);
            Ok(())
        }
        SiteToSiteVpnCommand::Create { from_file } => {
            let req = serde_json::from_value(util::read_json_file(&from_file)?)?;
            let result = controller
                .execute(CoreCommand::CreateSiteToSiteVpn(req))
                .await?;
            if let unifly_api::CommandResult::Json(record) = result {
                print_created_record(global, &record);
            }
            if !global.quiet {
                eprintln!("Site-to-site VPN created");
            }
            Ok(())
        }
        SiteToSiteVpnCommand::Update { id, from_file } => {
            let req = serde_json::from_value(util::read_json_file(&from_file)?)?;
            controller
                .execute(CoreCommand::UpdateSiteToSiteVpn {
                    id: EntityId::Legacy(id),
                    update: req,
                })
                .await?;
            if !global.quiet {
                eprintln!("Site-to-site VPN updated");
            }
            Ok(())
        }
        SiteToSiteVpnCommand::Delete { id } => {
            controller
                .execute(CoreCommand::DeleteSiteToSiteVpn {
                    id: EntityId::Legacy(id),
                })
                .await?;
            if !global.quiet {
                eprintln!("Site-to-site VPN deleted");
            }
            Ok(())
        }
    }
}

/// Render a session record returned by a VPN create on stdout. Sensitive
/// fields are already redacted at the library layer.
fn print_created_record(global: &GlobalOpts, value: &serde_json::Value) {
    let out = output::render_single(
        &global.output,
        value,
        |v| serde_json::to_string_pretty(v).unwrap_or_default(),
        |v| {
            v.get("_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned()
        },
    );
    output::print_output(&out, global.quiet);
}

async fn handle_remote_access(
    controller: &Controller,
    args: RemoteAccessVpnArgs,
    global: &GlobalOpts,
    painter: &output::Painter,
) -> Result<(), CliError> {
    util::ensure_session_access(controller, "remote-access vpn").await?;

    match args.command {
        RemoteAccessVpnCommand::List(list) => {
            let servers = util::apply_list_args(
                controller.list_remote_access_vpn_servers().await?,
                &list,
                util::matches_json_filter,
            );
            let out = output::render_list(
                &global.output,
                &servers,
                |server| remote_access_vpn_server_row(server, painter),
                |server| server.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
        RemoteAccessVpnCommand::Get { id } => {
            let server = controller.get_remote_access_vpn_server(&id).await?;
            let out = output::render_single(
                &global.output,
                &server,
                remote_access_vpn_server_detail,
                |server| server.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
        RemoteAccessVpnCommand::Create { from_file } => {
            let req = serde_json::from_value(util::read_json_file(&from_file)?)?;
            let result = controller
                .execute(CoreCommand::CreateRemoteAccessVpnServer(req))
                .await?;
            if let unifly_api::CommandResult::Json(record) = result {
                print_created_record(global, &record);
            }
            if !global.quiet {
                eprintln!("Remote-access VPN server created");
            }
            Ok(())
        }
        RemoteAccessVpnCommand::Update { id, from_file } => {
            let req = serde_json::from_value(util::read_json_file(&from_file)?)?;
            controller
                .execute(CoreCommand::UpdateRemoteAccessVpnServer {
                    id: EntityId::Legacy(id),
                    update: req,
                })
                .await?;
            if !global.quiet {
                eprintln!("Remote-access VPN server updated");
            }
            Ok(())
        }
        RemoteAccessVpnCommand::SuggestPort => {
            let ports = controller.list_openvpn_port_suggestions().await?;
            let out = output::render_list(
                &global.output,
                &ports,
                |port| openvpn_port_row(*port, painter),
                ToString::to_string,
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
        RemoteAccessVpnCommand::DownloadConfig { id, path } => {
            let bytes = controller.download_openvpn_configuration(&id).await?;
            let default_name = format!("{id}.ovpn");
            let mut target = path.unwrap_or_else(|| PathBuf::from(&default_name));
            if target.is_dir() {
                target = target.join(&default_name);
            }
            std::fs::write(&target, bytes)?;
            if !global.quiet {
                eprintln!("OpenVPN configuration downloaded to {}", target.display());
            }
            Ok(())
        }
        RemoteAccessVpnCommand::Delete { id } => {
            controller
                .execute(CoreCommand::DeleteRemoteAccessVpnServer {
                    id: EntityId::Legacy(id),
                })
                .await?;
            if !global.quiet {
                eprintln!("Remote-access VPN server deleted");
            }
            Ok(())
        }
    }
}

async fn handle_settings(
    controller: &Controller,
    args: VpnSettingsArgs,
    global: &GlobalOpts,
    painter: &output::Painter,
) -> Result<(), CliError> {
    util::ensure_session_access(controller, "vpn settings").await?;

    match args.command {
        VpnSettingsCommand::List(list) => {
            let settings = util::apply_list_args(
                controller.list_vpn_settings().await?,
                &list,
                util::matches_json_filter,
            );
            let out = output::render_list(
                &global.output,
                &settings,
                |setting| vpn_setting_row(setting, painter),
                |setting| setting.key.clone(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
        VpnSettingsCommand::Get { key } => {
            let setting = controller
                .get_vpn_setting(vpn_setting_key_name(key))
                .await?;
            let out =
                output::render_single(&global.output, &setting, vpn_setting_detail, |setting| {
                    setting.key.clone()
                });
            output::print_output(&out, global.quiet);
            Ok(())
        }
        VpnSettingsCommand::Set { key, enabled } => {
            controller
                .update_vpn_setting(
                    vpn_setting_key_name(key),
                    &serde_json::json!({ "enabled": enabled }),
                )
                .await?;
            if !global.quiet {
                eprintln!("VPN setting updated");
            }
            Ok(())
        }
        VpnSettingsCommand::Patch { key, from_file } => {
            let body = vpn_setting_patch_body(util::read_json_file(&from_file)?);
            controller
                .update_vpn_setting(vpn_setting_key_name(key), &body)
                .await?;
            if !global.quiet {
                eprintln!("VPN setting patched");
            }
            Ok(())
        }
    }
}

async fn handle_clients(
    controller: &Controller,
    args: VpnClientsArgs,
    global: &GlobalOpts,
    painter: &output::Painter,
) -> Result<(), CliError> {
    util::ensure_session_access(controller, "vpn clients").await?;

    match args.command {
        VpnClientsCommand::List(list) => {
            let clients = util::apply_list_args(
                controller.list_vpn_client_profiles().await?,
                &list,
                util::matches_json_filter,
            );
            let out = output::render_list(
                &global.output,
                &clients,
                |client| vpn_client_profile_row(client, painter),
                |client| client.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
        VpnClientsCommand::Get { id } => {
            let client = controller.get_vpn_client_profile(&id).await?;
            let out = output::render_single(
                &global.output,
                &client,
                vpn_client_profile_detail,
                |client| client.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
        VpnClientsCommand::Create { from_file } => {
            let req = serde_json::from_value(util::read_json_file(&from_file)?)?;
            let result = controller
                .execute(CoreCommand::CreateVpnClientProfile(req))
                .await?;
            if let unifly_api::CommandResult::Json(record) = result {
                print_created_record(global, &record);
            }
            if !global.quiet {
                eprintln!("VPN client created");
            }
            Ok(())
        }
        VpnClientsCommand::Update { id, from_file } => {
            let req = serde_json::from_value(util::read_json_file(&from_file)?)?;
            controller
                .execute(CoreCommand::UpdateVpnClientProfile {
                    id: EntityId::Legacy(id),
                    update: req,
                })
                .await?;
            if !global.quiet {
                eprintln!("VPN client updated");
            }
            Ok(())
        }
        VpnClientsCommand::Delete { id } => {
            controller
                .execute(CoreCommand::DeleteVpnClientProfile {
                    id: EntityId::Legacy(id),
                })
                .await?;
            if !global.quiet {
                eprintln!("VPN client deleted");
            }
            Ok(())
        }
    }
}

async fn handle_connections(
    controller: &Controller,
    args: VpnConnectionsArgs,
    global: &GlobalOpts,
    painter: &output::Painter,
) -> Result<(), CliError> {
    util::ensure_session_access(controller, "vpn connections").await?;

    match args.command {
        VpnConnectionsCommand::List(list) => {
            let connections = util::apply_list_args(
                controller.list_vpn_client_connections().await?,
                &list,
                util::matches_json_filter,
            );
            let out = output::render_list(
                &global.output,
                &connections,
                |connection| vpn_client_connection_row(connection, painter),
                |connection| connection.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
        VpnConnectionsCommand::Get { id } => {
            let connection = controller.get_vpn_client_connection(&id).await?;
            let out = output::render_single(
                &global.output,
                &connection,
                vpn_client_connection_detail,
                |connection| connection.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
        VpnConnectionsCommand::Restart { id } => {
            controller
                .execute(CoreCommand::RestartVpnClientConnection {
                    id: EntityId::Legacy(id),
                })
                .await?;
            if !global.quiet {
                eprintln!("VPN client connection restarted");
            }
            Ok(())
        }
    }
}

async fn handle_peers(
    controller: &Controller,
    args: VpnPeersArgs,
    global: &GlobalOpts,
    painter: &output::Painter,
) -> Result<(), CliError> {
    util::ensure_session_access(controller, "vpn peers").await?;

    match args.command {
        VpnPeersCommand::List { server_id, list } => {
            let peers = util::apply_list_args(
                controller
                    .list_wireguard_peers(server_id.as_deref())
                    .await?,
                &list,
                util::matches_json_filter,
            );
            let out = output::render_list(
                &global.output,
                &peers,
                |peer| wireguard_peer_row(peer, painter),
                |peer| peer.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
        VpnPeersCommand::Get { server_id, id } => {
            let peer = controller.get_wireguard_peer(&server_id, &id).await?;
            let out = output::render_single(&global.output, &peer, wireguard_peer_detail, |peer| {
                peer.id.to_string()
            });
            output::print_output(&out, global.quiet);
            Ok(())
        }
        VpnPeersCommand::Create {
            server_id,
            from_file,
        } => {
            let req = serde_json::from_value(util::read_json_file(&from_file)?)?;
            let result = controller
                .execute(CoreCommand::CreateWireGuardPeer {
                    server_id: EntityId::Legacy(server_id),
                    peer: req,
                })
                .await?;
            if let unifly_api::CommandResult::Json(record) = result {
                print_created_record(global, &record);
            }
            if !global.quiet {
                eprintln!("WireGuard peer created");
            }
            Ok(())
        }
        VpnPeersCommand::Update {
            server_id,
            id,
            from_file,
        } => {
            let req = serde_json::from_value(util::read_json_file(&from_file)?)?;
            controller
                .execute(CoreCommand::UpdateWireGuardPeer {
                    server_id: EntityId::Legacy(server_id),
                    peer_id: EntityId::Legacy(id),
                    update: req,
                })
                .await?;
            if !global.quiet {
                eprintln!("WireGuard peer updated");
            }
            Ok(())
        }
        VpnPeersCommand::Delete { server_id, id } => {
            controller
                .execute(CoreCommand::DeleteWireGuardPeer {
                    server_id: EntityId::Legacy(server_id),
                    peer_id: EntityId::Legacy(id),
                })
                .await?;
            if !global.quiet {
                eprintln!("WireGuard peer deleted");
            }
            Ok(())
        }
        VpnPeersCommand::Subnets => {
            let subnets = controller.list_wireguard_peer_existing_subnets().await?;
            let out = output::render_list(
                &global.output,
                &subnets,
                |subnet| wireguard_peer_subnet_row(subnet, painter),
                Clone::clone,
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
    }
}

async fn handle_magic_site_to_site(
    controller: &Controller,
    args: MagicSiteToSiteVpnArgs,
    global: &GlobalOpts,
    painter: &output::Painter,
) -> Result<(), CliError> {
    util::ensure_session_access(controller, "magic site-to-site vpn").await?;

    match args.command {
        MagicSiteToSiteVpnCommand::List(list) => {
            let configs = util::apply_list_args(
                controller.list_magic_site_to_site_vpn_configs().await?,
                &list,
                util::matches_json_filter,
            );
            let out = output::render_list(
                &global.output,
                &configs,
                |config| magic_site_to_site_vpn_config_row(config, painter),
                |config| config.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
        MagicSiteToSiteVpnCommand::Get { id } => {
            let config = controller.get_magic_site_to_site_vpn_config(&id).await?;
            let out = output::render_single(
                &global.output,
                &config,
                magic_site_to_site_vpn_config_detail,
                |config| config.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
    }
}
