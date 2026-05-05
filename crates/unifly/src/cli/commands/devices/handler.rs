use std::sync::Arc;

use unifly_api::{
    ApplyPortEntry, ApplyPortsRequest, Command as CoreCommand, Controller, Device, MacAddress,
    PoeMode, PortMode, PortProfileUpdate, PortSpeedSetting,
};

use crate::cli::args::{DevicesArgs, DevicesCommand, GlobalOpts, PoeArg, PortModeArg, SpeedArg};
use crate::cli::commands::util;
use crate::cli::error::CliError;
use crate::cli::output;

use super::render::{
    build_last_seen_markers, detail, device_row, device_tag_identity, device_tag_row,
    enrich_with_connections, enriched_port_row, inject_last_seen_markers, pending_device_identity,
    pending_device_row, port_row, stats_detail,
};

fn find_device(controller: &Controller, needle: &str) -> Option<Arc<Device>> {
    controller
        .devices_snapshot()
        .iter()
        .find(|candidate| candidate.id.to_string() == needle || candidate.mac.to_string() == needle)
        .cloned()
}

#[allow(clippy::too_many_lines)]
pub(super) async fn handle(
    controller: &Controller,
    args: DevicesArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    let painter = output::Painter::new(global);

    match args.command {
        DevicesCommand::List(list) => {
            let all = controller.devices_snapshot();
            let snapshot = util::apply_list_args(all.iter().cloned(), &list, |device, filter| {
                util::matches_json_filter(device, filter)
            });
            let out = output::render_list(
                &global.output,
                &snapshot,
                |device| device_row(device, &painter),
                |device| device.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        DevicesCommand::Get { device } => {
            match find_device(controller, &device) {
                Some(device) => {
                    let out = output::render_single(&global.output, &device, detail, |device| {
                        device.id.to_string()
                    });
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "device".into(),
                        identifier: device,
                        list_command: "devices list".into(),
                    });
                }
            }
            Ok(())
        }

        DevicesCommand::Adopt { mac, ignore_limit } => {
            controller
                .execute(CoreCommand::AdoptDevice {
                    mac: MacAddress::new(&mac),
                    ignore_device_limit: ignore_limit,
                })
                .await?;
            if !global.quiet {
                eprintln!("Device adoption initiated");
            }
            Ok(())
        }

        DevicesCommand::Remove { device } => {
            let id = util::resolve_device_id(controller, &device)?;
            if !util::confirm(&format!("Remove device {device}?"), global.yes)? {
                return Ok(());
            }
            controller.execute(CoreCommand::RemoveDevice { id }).await?;
            if !global.quiet {
                eprintln!("Device removed");
            }
            Ok(())
        }

        DevicesCommand::Restart { device } => {
            let id = util::resolve_device_id(controller, &device)?;
            controller
                .execute(CoreCommand::RestartDevice { id })
                .await?;
            if !global.quiet {
                eprintln!("Device restart initiated");
            }
            Ok(())
        }

        DevicesCommand::Locate { device, on } => {
            let mac = util::resolve_device_mac(controller, &device)?;
            controller
                .execute(CoreCommand::LocateDevice { mac, enable: on })
                .await?;
            if !global.quiet {
                let state = if on { "enabled" } else { "disabled" };
                eprintln!("Locate LED {state}");
            }
            Ok(())
        }

        DevicesCommand::PortCycle { device, port } => {
            let device_id = util::resolve_device_id(controller, &device)?;
            controller
                .execute(CoreCommand::PowerCyclePort {
                    device_id,
                    port_idx: port,
                })
                .await?;
            if !global.quiet {
                eprintln!("Port {port} power-cycled");
            }
            Ok(())
        }

        DevicesCommand::Stats { device } => {
            match find_device(controller, &device) {
                Some(device) => {
                    let out =
                        output::render_single(&global.output, &device, stats_detail, |device| {
                            device.id.to_string()
                        });
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "device".into(),
                        identifier: device,
                        list_command: "devices list".into(),
                    });
                }
            }
            Ok(())
        }

        DevicesCommand::Pending(list) => {
            let pending = util::apply_list_args(
                controller.list_pending_devices().await?,
                &list,
                util::matches_json_filter,
            );
            let out = output::render_list(
                &global.output,
                &pending,
                |value| pending_device_row(value, &painter),
                pending_device_identity,
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        DevicesCommand::Upgrade { device, url } => {
            let mac = util::resolve_device_mac(controller, &device)?;
            controller
                .execute(CoreCommand::UpgradeDevice {
                    mac,
                    firmware_url: url,
                })
                .await?;
            if !global.quiet {
                eprintln!("Firmware upgrade initiated");
            }
            Ok(())
        }

        DevicesCommand::Provision { device } => {
            let mac = util::resolve_device_mac(controller, &device)?;
            controller
                .execute(CoreCommand::ProvisionDevice { mac })
                .await?;
            if !global.quiet {
                eprintln!("Device re-provision initiated");
            }
            Ok(())
        }

        DevicesCommand::Speedtest => {
            controller.execute(CoreCommand::SpeedtestDevice).await?;
            if !global.quiet {
                eprintln!("Speed test initiated");
            }
            Ok(())
        }

        DevicesCommand::Tags(list) => {
            let tags = util::apply_list_args(
                controller.list_device_tags().await?,
                &list,
                util::matches_json_filter,
            );
            let out = output::render_list(
                &global.output,
                &tags,
                |value| device_tag_row(value, &painter),
                device_tag_identity,
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        DevicesCommand::Ports {
            device,
            with_clients,
        } => handle_ports(controller, global, &painter, &device, with_clients).await,

        DevicesCommand::PortsExport {
            device,
            all,
            with_clients,
        } => handle_ports_export(controller, &device, all, with_clients).await,

        DevicesCommand::PortSet {
            device,
            port,
            mode,
            native_vlan,
            tagged_vlans,
            name,
            poe,
            speed,
            from_file,
            reset,
        } => {
            dispatch_port_set(
                controller,
                global,
                device,
                port,
                mode,
                native_vlan,
                tagged_vlans,
                name,
                poe,
                speed,
                from_file,
                reset,
            )
            .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_port_set(
    controller: &Controller,
    global: &GlobalOpts,
    device: String,
    port: Option<u32>,
    mode: Option<PortModeArg>,
    native_vlan: Option<String>,
    tagged_vlans: Option<Vec<String>>,
    name: Option<String>,
    poe: Option<PoeArg>,
    speed: Option<SpeedArg>,
    from_file: Option<std::path::PathBuf>,
    reset: bool,
) -> Result<(), CliError> {
    if let Some(path) = from_file {
        handle_port_set_from_file(controller, global, &device, &path).await
    } else if reset {
        let port = port.ok_or_else(|| CliError::Validation {
            field: "port-set".into(),
            reason: "PORT_IDX is required for --reset".into(),
        })?;
        handle_port_reset(controller, global, &device, port).await
    } else {
        let port = port.ok_or_else(|| CliError::Validation {
            field: "port-set".into(),
            reason: "PORT_IDX is required (or pass --from-file)".into(),
        })?;
        handle_port_set(
            controller,
            global,
            &device,
            port,
            mode,
            native_vlan,
            tagged_vlans,
            name,
            poe,
            speed,
        )
        .await
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_port_set(
    controller: &Controller,
    global: &GlobalOpts,
    device: &str,
    port: u32,
    mode: Option<PortModeArg>,
    native_vlan: Option<String>,
    tagged_vlans: Option<Vec<String>>,
    name: Option<String>,
    poe: Option<PoeArg>,
    speed: Option<SpeedArg>,
) -> Result<(), CliError> {
    util::ensure_session_access(controller, "devices port-set").await?;
    let mac = util::resolve_device_mac(controller, device)?;

    if mode.is_none()
        && native_vlan.is_none()
        && tagged_vlans.is_none()
        && name.is_none()
        && poe.is_none()
        && speed.is_none()
    {
        return Err(CliError::Validation {
            field: "port-set".into(),
            reason:
                "at least one of --mode / --native-vlan / --tagged-vlans / --name / --poe / --speed is required"
                    .into(),
        });
    }

    if matches!(mode, Some(PortModeArg::Access))
        && tagged_vlans.as_ref().is_some_and(|v| !v.is_empty())
    {
        return Err(CliError::Validation {
            field: "tagged-vlans".into(),
            reason: "access mode cannot carry tagged VLANs; use --mode trunk with --tagged-vlans"
                .into(),
        });
    }

    let native_network_id = match native_vlan {
        Some(name) => Some(controller.resolve_network_session_id(&name).await?.0),
        None => None,
    };
    let tagged_network_ids = resolve_tagged_networks(controller, tagged_vlans).await?;

    let update = PortProfileUpdate {
        name,
        mode: mode.map(map_port_mode),
        native_network_id,
        tagged_network_ids,
        poe_mode: poe.map(map_poe),
        speed_setting: speed.map(map_speed),
    };

    controller.update_device_port(&mac, port, &update).await?;
    if !global.quiet {
        eprintln!("Port {port} updated on device {device}");
    }
    Ok(())
}

async fn handle_ports(
    controller: &Controller,
    global: &GlobalOpts,
    painter: &output::Painter,
    device: &str,
    with_clients: bool,
) -> Result<(), CliError> {
    util::ensure_session_access(controller, "devices ports").await?;
    let mac = util::resolve_device_mac(controller, device)?;
    let profiles = controller.list_device_ports(&mac).await?;
    if with_clients {
        let clients = controller.clients_snapshot();
        let devices = controller.devices_snapshot();
        let enriched = enrich_with_connections(&profiles, &clients, &devices, &mac);
        let out = output::render_list(
            &global.output,
            &enriched,
            |port| enriched_port_row(port, painter),
            |port| port.profile.index.to_string(),
        );
        output::print_output(&out, global.quiet);
    } else {
        let out = output::render_list(
            &global.output,
            &profiles,
            |profile| port_row(profile, painter),
            |profile| profile.index.to_string(),
        );
        output::print_output(&out, global.quiet);
    }
    Ok(())
}

async fn handle_ports_export(
    controller: &Controller,
    device: &str,
    all: bool,
    with_clients: bool,
) -> Result<(), CliError> {
    util::ensure_session_access(controller, "devices ports-export").await?;
    let mac = util::resolve_device_mac(controller, device)?;

    let request = controller.export_device_ports(&mac, all).await?;
    let json = serde_json::to_string_pretty(&request)?;
    let output = if with_clients {
        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%MZ").to_string();
        let clients = controller.clients_snapshot();
        let devices = controller.devices_snapshot();
        let markers = build_last_seen_markers(&clients, &devices, &mac, &timestamp);
        inject_last_seen_markers(&json, &markers)
    } else {
        let mut s = json;
        s.push('\n');
        s
    };
    print!("{output}");
    Ok(())
}

async fn handle_port_reset(
    controller: &Controller,
    global: &GlobalOpts,
    device: &str,
    port: u32,
) -> Result<(), CliError> {
    util::ensure_session_access(controller, "devices port-set --reset").await?;
    let mac = util::resolve_device_mac(controller, device)?;
    if !util::confirm(
        &format!("Reset port {port} on {device} to controller defaults?"),
        global.yes,
    )? {
        return Ok(());
    }
    let request = ApplyPortsRequest {
        ports: vec![ApplyPortEntry {
            index: port,
            reset: true,
            ..ApplyPortEntry::default()
        }],
    };
    let summary = controller.apply_device_ports(&mac, &request).await?;
    if !global.quiet {
        if summary.reset > 0 {
            eprintln!("Port {port} override removed on device {device}");
        } else {
            eprintln!("Port {port} had no override on device {device} (no change)");
        }
    }
    Ok(())
}

async fn handle_port_set_from_file(
    controller: &Controller,
    global: &GlobalOpts,
    device: &str,
    path: &std::path::Path,
) -> Result<(), CliError> {
    util::ensure_session_access(controller, "devices port-set --from-file").await?;
    let mac = util::resolve_device_mac(controller, device)?;

    let request: ApplyPortsRequest = serde_json::from_value(util::read_json_file(path)?)?;
    if request.ports.is_empty() {
        return Err(CliError::Validation {
            field: "ports".into(),
            reason: "ports array is empty; nothing to apply".into(),
        });
    }

    // Mirror the confirmation guard the single-port `--reset` path uses
    // (see `handle_port_reset`). A from-file batch can wipe overrides on
    // many ports at once via `"reset": true` entries; without this
    // prompt a misapplied JSONC silently rewrites a production switch.
    let n_ports = request.ports.len();
    let n_resets = request.ports.iter().filter(|p| p.reset).count();
    let summary_label = if n_resets > 0 {
        format!("{n_ports} port override(s), including {n_resets} reset(s)")
    } else {
        format!("{n_ports} port override(s)")
    };
    if !util::confirm(
        &format!("Apply {summary_label} to device {device}?"),
        global.yes,
    )? {
        return Ok(());
    }

    let summary = controller.apply_device_ports(&mac, &request).await?;
    if !global.quiet {
        let plural_applied = if summary.applied == 1 { "" } else { "s" };
        let plural_reset = if summary.reset == 1 { "" } else { "s" };
        eprintln!(
            "Applied {applied} port override{plural_applied}{maybe_reset} on device {device}",
            applied = summary.applied,
            maybe_reset = if summary.reset > 0 {
                format!(", reset {} port{plural_reset}", summary.reset)
            } else {
                String::new()
            },
        );
    }
    Ok(())
}

async fn resolve_tagged_networks(
    controller: &Controller,
    tagged_vlans: Option<Vec<String>>,
) -> Result<Option<Vec<String>>, CliError> {
    let Some(names) = tagged_vlans else {
        return Ok(None);
    };
    let mut ids = Vec::with_capacity(names.len());
    for n in names {
        let trimmed = n.trim();
        if trimmed.is_empty() {
            continue;
        }
        ids.push(controller.resolve_network_session_id(trimmed).await?.0);
    }
    Ok(Some(ids))
}

fn map_port_mode(mode: PortModeArg) -> PortMode {
    match mode {
        PortModeArg::Access => PortMode::Access,
        PortModeArg::Trunk => PortMode::Trunk,
        PortModeArg::Mirror => PortMode::Mirror,
    }
}

fn map_poe(arg: PoeArg) -> PoeMode {
    match arg {
        PoeArg::Auto => PoeMode::Auto,
        PoeArg::Off => PoeMode::Off,
        PoeArg::Pasv24 => PoeMode::Passive24V,
        PoeArg::Passthrough => PoeMode::Passthrough,
    }
}

fn map_speed(arg: SpeedArg) -> PortSpeedSetting {
    match arg {
        SpeedArg::Auto => PortSpeedSetting::Auto,
        SpeedArg::Mbps10 => PortSpeedSetting::Mbps10,
        SpeedArg::Mbps100 => PortSpeedSetting::Mbps100,
        SpeedArg::Mbps1000 => PortSpeedSetting::Mbps1000,
        SpeedArg::Mbps2500 => PortSpeedSetting::Mbps2500,
        SpeedArg::Mbps5000 => PortSpeedSetting::Mbps5000,
        SpeedArg::Mbps10000 => PortSpeedSetting::Mbps10000,
    }
}
