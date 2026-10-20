use std::net::Ipv4Addr;

use unifly_api::{Client, Command as CoreCommand, Controller, EntityId};

use crate::cli::args::{ClientsArgs, ClientsCommand, GlobalOpts};
use crate::cli::commands::util;
use crate::cli::error::CliError;
use crate::cli::output;

use super::render::{
    Reservation, client_row, detail, reservation_row, roam_row, wifi_experience_detail,
};
use super::resolve::{resolve_network, resolve_network_by_name};

fn matches_find_query(client: &Client, query: &str) -> bool {
    let fields = [
        client.ip.map(|ip| ip.to_string()),
        client.name.clone(),
        client.hostname.clone(),
        Some(client.mac.to_string()),
    ];
    fields.iter().any(|field| {
        field
            .as_ref()
            .is_some_and(|value| value.to_lowercase().contains(query))
    })
}

#[allow(clippy::too_many_lines)]
pub(super) async fn handle(
    controller: &Controller,
    args: ClientsArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    let painter = output::Painter::new(global);

    match args.command {
        ClientsCommand::List(list) => {
            let all = controller.clients_snapshot();
            let snapshot = util::apply_list_args(util::order::clients(&all), &list);
            let out = output::render_list(
                &global.output,
                &snapshot,
                |client| client_row(client, &painter),
                |client| client.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        ClientsCommand::Find { query } => {
            let normalized_query = query.to_lowercase();
            let matches: Vec<_> = util::order::clients(&controller.clients_snapshot())
                .into_iter()
                .filter(|client| matches_find_query(client, &normalized_query))
                .collect();
            if matches.is_empty() {
                return Err(CliError::NotFound {
                    resource_type: "client".into(),
                    identifier: query,
                    list_command: "clients list".into(),
                });
            }
            let out = output::render_list(
                &global.output,
                &matches,
                |client| client_row(client, &painter),
                |client| client.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        ClientsCommand::Get { client } => {
            let client = util::resolve_client(controller, &client)?;
            let out = output::render_single(&global.output, &client, detail, |client| {
                client.id.to_string()
            });
            output::print_output(&out, global.quiet);
            Ok(())
        }

        ClientsCommand::Roams { client, limit } => {
            let mac = util::resolve_client_mac(controller, &client)?.to_string();
            let events = controller.get_client_roams(&mac, Some(limit)).await?;
            let out = output::render_list(
                &global.output,
                &events,
                |event| roam_row(event, &painter),
                |event| {
                    event
                        .get("timestamp")
                        .or_else(|| event.get("time"))
                        .and_then(serde_json::Value::as_i64)
                        .map_or_else(|| "-".into(), |timestamp| timestamp.to_string())
                },
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        ClientsCommand::Wifi { client } => {
            let ip = util::resolve_client_ip(controller, &client)?.to_string();
            let data = controller.get_client_wifi_experience(&ip).await?;
            let out = output::render_single(
                &global.output,
                &data,
                |value| wifi_experience_detail(value, &painter),
                |_| ip.clone(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        ClientsCommand::Authorize {
            client,
            minutes,
            data_limit_mb,
            rx_limit_kbps,
            tx_limit_kbps,
        } => {
            let client_id = EntityId::from(client);
            controller
                .execute(CoreCommand::AuthorizeGuest {
                    client_id,
                    time_limit_minutes: Some(minutes),
                    data_limit_mb,
                    rx_rate_kbps: rx_limit_kbps,
                    tx_rate_kbps: tx_limit_kbps,
                })
                .await?;
            if !global.quiet {
                eprintln!("Guest authorized for {minutes} minutes");
            }
            Ok(())
        }

        ClientsCommand::Unauthorize { client } => {
            let client_id = EntityId::from(client);
            controller
                .execute(CoreCommand::UnauthorizeGuest { client_id })
                .await?;
            if !global.quiet {
                eprintln!("Guest authorization revoked");
            }
            Ok(())
        }

        ClientsCommand::Block { mac } => {
            let mac = util::require_mac(
                &mac,
                "client actions take the client's MAC (find it with `unifly clients list`)",
            )?;
            controller.execute(CoreCommand::BlockClient { mac }).await?;
            if !global.quiet {
                eprintln!("Client blocked");
            }
            Ok(())
        }

        ClientsCommand::Unblock { mac } => {
            let mac = util::require_mac(
                &mac,
                "client actions take the client's MAC (find it with `unifly clients list`)",
            )?;
            controller
                .execute(CoreCommand::UnblockClient { mac })
                .await?;
            if !global.quiet {
                eprintln!("Client unblocked");
            }
            Ok(())
        }

        ClientsCommand::Kick { mac } => {
            let mac = util::require_mac(
                &mac,
                "client actions take the client's MAC (find it with `unifly clients list`)",
            )?;
            controller.execute(CoreCommand::KickClient { mac }).await?;
            if !global.quiet {
                eprintln!("Client disconnected");
            }
            Ok(())
        }

        ClientsCommand::Forget { mac } => {
            let mac_addr = util::require_mac(
                &mac,
                "client actions take the client's MAC (find it with `unifly clients list`)",
            )?;
            if !util::confirm(
                &format!("Forget client {mac}? This cannot be undone."),
                global.yes,
            )? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::ForgetClient { mac: mac_addr })
                .await?;
            if !global.quiet {
                eprintln!("Client forgotten");
            }
            Ok(())
        }

        ClientsCommand::Reservations(list) => {
            let users = controller.list_users().await?;
            let reservations: Vec<Reservation> = users
                .iter()
                .filter(|u| u.use_fixedip.unwrap_or(false))
                .map(Reservation::from)
                .collect();
            let snapshot = util::apply_list_args(reservations, &list);
            let out = output::render_list(
                &global.output,
                &snapshot,
                |res| reservation_row(res, &painter),
                |res| res.mac.clone(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        ClientsCommand::SetIp { mac, ip, network } => {
            let ip_addr: Ipv4Addr = ip.parse().map_err(|_| CliError::Validation {
                field: "ip".into(),
                reason: format!("'{ip}' is not a valid IPv4 address"),
            })?;

            let network_id = resolve_network(controller, network.as_deref(), ip_addr)?;
            let mac_addr = util::require_mac(
                &mac,
                "client actions take the client's MAC (find it with `unifly clients list`)",
            )?;

            controller
                .execute(CoreCommand::SetClientFixedIp {
                    mac: mac_addr,
                    ip: ip_addr,
                    network_id,
                })
                .await?;
            if !global.quiet {
                eprintln!("Fixed IP {ip} set for {mac}");
            }
            Ok(())
        }

        ClientsCommand::RemoveIp { mac, network } => {
            let network_id = network
                .as_deref()
                .map(|n| resolve_network_by_name(controller, n))
                .transpose()?;
            let mac_addr = util::require_mac(
                &mac,
                "client actions take the client's MAC (find it with `unifly clients list`)",
            )?;
            controller
                .execute(CoreCommand::RemoveClientFixedIp {
                    mac: mac_addr,
                    network_id,
                })
                .await?;
            if !global.quiet {
                if let Some(name) = &network {
                    eprintln!("Fixed IP removed for {mac} on network {name}");
                } else {
                    eprintln!("Fixed IP removed for {mac}");
                }
            }
            Ok(())
        }
    }
}
