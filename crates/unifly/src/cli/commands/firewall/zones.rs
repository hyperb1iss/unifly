use std::sync::Arc;

use tabled::Tabled;
use unifly_api::model::FirewallZone;
use unifly_api::{
    Command as CoreCommand, Controller, CreateFirewallZoneRequest, EntityId,
    UpdateFirewallZoneRequest,
};

use crate::cli::args::{FirewallZonesCommand, GlobalOpts};
use crate::cli::error::CliError;
use crate::cli::output;

use super::util;

#[derive(Tabled)]
struct ZoneRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Networks")]
    network_count: String,
}

fn zone_row(zone: &Arc<FirewallZone>, painter: &output::Painter) -> ZoneRow {
    ZoneRow {
        id: painter.id(&zone.id.to_string()),
        name: painter.name(&zone.name),
        network_count: painter.number(&zone.network_ids.len().to_string()),
    }
}

fn zone_detail(zone: &Arc<FirewallZone>) -> String {
    let networks = zone
        .network_ids
        .iter()
        .map(|id| format!("  - {id}"))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "ID:       {}\nName:     {}\nNetworks:\n{}",
        zone.id, zone.name, networks
    )
}

#[allow(clippy::too_many_lines)]
pub(super) async fn handle(
    controller: &Controller,
    cmd: FirewallZonesCommand,
    global: &GlobalOpts,
    painter: &output::Painter,
) -> Result<(), CliError> {
    match cmd {
        FirewallZonesCommand::List(list) => {
            let all = controller.firewall_zones_snapshot();
            let snapshot = util::apply_list_args(all.iter().cloned(), &list, |zone, filter| {
                util::matches_json_filter(zone, filter)
            });
            let out = output::render_list(
                &global.output,
                &snapshot,
                |zone| zone_row(zone, painter),
                |zone| zone.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        FirewallZonesCommand::Get { id } => {
            let snapshot = controller.firewall_zones_snapshot();
            let found = snapshot.iter().find(|zone| zone.id.to_string() == id);
            match found {
                Some(zone) => {
                    let out = output::render_single(&global.output, zone, zone_detail, |zone| {
                        zone.id.to_string()
                    });
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "firewall zone".into(),
                        identifier: id,
                        list_command: "firewall zones list".into(),
                    });
                }
            }
            Ok(())
        }

        FirewallZonesCommand::Create {
            name,
            networks,
            from_file,
        } => {
            let req = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                let network_ids = networks
                    .unwrap_or_default()
                    .into_iter()
                    .map(EntityId::from)
                    .collect();
                CreateFirewallZoneRequest {
                    name: name.unwrap_or_default(),
                    description: None,
                    network_ids,
                }
            };
            let result = controller
                .execute(CoreCommand::CreateFirewallZone(req))
                .await?;
            if let unifly_api::CommandResult::FirewallZone(zone) = result {
                let zone = Arc::new(zone);
                let out =
                    crate::cli::output::render_single(&global.output, &zone, zone_detail, |z| {
                        z.id.to_string()
                    });
                crate::cli::output::print_output(&out, global.quiet);
            } else {
                crate::cli::output::warn_unrenderable_create(global.quiet);
            }
            if !global.quiet {
                eprintln!("Firewall zone created");
            }
            Ok(())
        }

        FirewallZonesCommand::Update {
            id,
            name,
            networks,
            from_file,
        } => {
            if from_file.is_none() && name.is_none() && networks.is_none() {
                return Err(CliError::Validation {
                    field: "update".into(),
                    reason: "at least one of --name, --networks, or --from-file is required".into(),
                });
            }

            let update = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                UpdateFirewallZoneRequest {
                    name,
                    description: None,
                    network_ids: networks
                        .map(|values| values.into_iter().map(EntityId::from).collect()),
                }
            };
            controller
                .execute(CoreCommand::UpdateFirewallZone {
                    id: EntityId::from(id),
                    update,
                })
                .await?;
            if !global.quiet {
                eprintln!("Firewall zone updated");
            }
            Ok(())
        }

        FirewallZonesCommand::Delete { id } => {
            if !util::confirm(&format!("Delete firewall zone {id}?"), global.yes)? {
                return Ok(());
            }

            controller
                .execute(CoreCommand::DeleteFirewallZone {
                    id: EntityId::from(id),
                })
                .await?;
            if !global.quiet {
                eprintln!("Firewall zone deleted");
            }
            Ok(())
        }
    }
}
