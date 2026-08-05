//! Network command handlers.

use std::sync::Arc;

use tabled::Tabled;
use unifly_api::{
    Command as CoreCommand, Controller, CreateNetworkRequest, EntityId, Network,
    UpdateNetworkRequest,
};

use crate::cli::args::{
    GlobalOpts, NetworkManagement as CliNetworkManagement, NetworksArgs, NetworksCommand,
};
use crate::cli::error::CliError;
use crate::cli::output;

use super::util;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct NetworkRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "VLAN")]
    vlan: String,
    #[tabled(rename = "Management")]
    management: String,
    #[tabled(rename = "Enabled")]
    enabled: String,
    #[tabled(rename = "Subnet")]
    subnet: String,
}

fn network_row(n: &Arc<Network>, p: &output::Painter) -> NetworkRow {
    NetworkRow {
        id: p.id(&n.id.to_string()),
        name: p.name(&n.name),
        vlan: p.number(&n.vlan_id.map(|v| v.to_string()).unwrap_or_default()),
        management: p.muted(&n.management.map(|m| format!("{m:?}")).unwrap_or_default()),
        enabled: p.enabled(n.enabled),
        subnet: p.ip(&n.subnet.clone().unwrap_or_default()),
    }
}

fn detail(n: &Arc<Network>) -> String {
    let mut lines = vec![
        format!("ID:         {}", n.id),
        format!("Name:       {}", n.name),
        format!("Enabled:    {}", n.enabled),
        format!(
            "Management: {}",
            n.management.map_or_else(
                || "-".into(),
                |m: unifly_api::model::NetworkManagement| format!("{m:?}")
            )
        ),
        format!(
            "VLAN:       {}",
            n.vlan_id.map_or_else(|| "-".into(), |v: u16| v.to_string())
        ),
        format!("Subnet:     {}", n.subnet.as_deref().unwrap_or("-")),
        format!(
            "Gateway:    {}",
            n.gateway_ip
                .map_or_else(|| "-".into(), |ip: std::net::Ipv4Addr| ip.to_string())
        ),
        format!("Isolated:   {}", n.isolation_enabled),
        format!("Internet:   {}", n.internet_access_enabled),
    ];
    if let Some(ref dhcp) = n.dhcp {
        lines.push(format!("DHCP:       {}", dhcp.enabled));
        if let Some(start) = dhcp.range_start {
            lines.push(format!("DHCP Start: {start}"));
        }
        if let Some(stop) = dhcp.range_stop {
            lines.push(format!("DHCP Stop:  {stop}"));
        }
    }
    lines.join("\n")
}

fn map_network_management(value: &CliNetworkManagement) -> unifly_api::model::NetworkManagement {
    match value {
        CliNetworkManagement::Gateway => unifly_api::model::NetworkManagement::Gateway,
        CliNetworkManagement::Switch => unifly_api::model::NetworkManagement::Switch,
        CliNetworkManagement::Unmanaged => unifly_api::model::NetworkManagement::Unmanaged,
    }
}

// ── Handler ─────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
pub async fn handle(
    controller: &Controller,
    args: NetworksArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    util::ensure_integration_access(controller, "networks").await?;

    let p = output::Painter::new(global);

    match args.command {
        NetworksCommand::List(list) => {
            let all = controller.networks_snapshot();
            let snap = util::apply_list_args(all.iter().cloned(), &list, |n, filter| {
                util::matches_json_filter(n, filter)
            });
            let out = output::render_list(
                &global.output,
                &snap,
                |n| network_row(n, &p),
                |n| n.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        NetworksCommand::Get { id } => {
            let snap = controller.networks_snapshot();
            let found = snap.iter().find(|n| n.id.to_string() == id);
            match found {
                Some(n) => {
                    let out =
                        output::render_single(&global.output, n, detail, |n| n.id.to_string());
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "network".into(),
                        identifier: id,
                        list_command: "networks list".into(),
                    });
                }
            }
            Ok(())
        }

        NetworksCommand::Create {
            from_file,
            name,
            management,
            vlan,
            enabled,
            ipv4_host,
            dhcp,
            dhcp_start,
            dhcp_stop,
            dhcp_lease,
            dns_servers,
            zone,
            isolated,
            internet,
        } => {
            let req = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                CreateNetworkRequest {
                    name: name.unwrap_or_default(),
                    vlan_id: vlan,
                    subnet: ipv4_host,
                    management: management.as_ref().map(map_network_management),
                    purpose: None,
                    dhcp_enabled: dhcp,
                    enabled,
                    dhcp_range_start: dhcp_start,
                    dhcp_range_stop: dhcp_stop,
                    dhcp_lease_time: dhcp_lease,
                    dns_servers,
                    firewall_zone_id: zone,
                    isolation_enabled: isolated,
                    internet_access_enabled: internet,
                }
            };

            let result = controller.execute(CoreCommand::CreateNetwork(req)).await?;
            if let unifly_api::CommandResult::Network(network) = result {
                let network = Arc::new(network);
                let out =
                    output::render_single(&global.output, &network, detail, |n| n.id.to_string());
                output::print_output(&out, global.quiet);
            }
            if !global.quiet {
                eprintln!("Network created");
            }
            Ok(())
        }

        NetworksCommand::Update {
            id,
            from_file,
            name,
            enabled,
            vlan,
        } => {
            if from_file.is_none() && name.is_none() && enabled.is_none() && vlan.is_none() {
                return Err(CliError::Validation {
                    field: "update".into(),
                    reason: "at least one update flag or --from-file is required".into(),
                });
            }
            let update = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                UpdateNetworkRequest {
                    name,
                    vlan_id: vlan,
                    subnet: None,
                    dhcp_enabled: None,
                    enabled,
                    isolation_enabled: None,
                    internet_access_enabled: None,
                    mdns_forwarding_enabled: None,
                    ipv6_enabled: None,
                    dhcp: None,
                }
            };

            let eid = EntityId::from(id);
            controller
                .execute(CoreCommand::UpdateNetwork { id: eid, update })
                .await?;
            if !global.quiet {
                eprintln!("Network updated");
            }
            Ok(())
        }

        NetworksCommand::Delete { id, force } => {
            let eid = EntityId::from(id.clone());
            if !util::confirm(&format!("Delete network {id}?"), global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::DeleteNetwork { id: eid, force })
                .await?;
            if !global.quiet {
                eprintln!("Network deleted");
            }
            Ok(())
        }

        NetworksCommand::Refs { id } => {
            let refs = controller
                .get_network_references(&EntityId::from(id.clone()))
                .await?;
            let out = output::render_single(
                &global.output,
                &refs,
                |v| serde_json::to_string_pretty(v).unwrap_or_default(),
                |_| id.clone(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
    }
}
