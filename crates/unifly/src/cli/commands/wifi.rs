//! WiFi broadcast command handlers.

use std::sync::Arc;

use serde::Serialize;
use tabled::Tabled;
use unifly_api::model::{WifiBroadcast, WifiSecurityMode};
use unifly_api::session_models::{ChannelAvailability, RogueAp};
use unifly_api::{
    Command as CoreCommand, Controller, CreateWifiBroadcastRequest, EntityId,
    UpdateWifiBroadcastRequest,
};

use crate::cli::args::{GlobalOpts, WifiArgs, WifiBroadcastType, WifiCommand, WifiSecurity};
use crate::cli::error::CliError;
use crate::cli::output;

use super::util;

fn map_security(s: &WifiSecurity) -> WifiSecurityMode {
    match s {
        WifiSecurity::Open => WifiSecurityMode::Open,
        WifiSecurity::Wpa2Personal => WifiSecurityMode::Wpa2Personal,
        WifiSecurity::Wpa3Personal => WifiSecurityMode::Wpa3Personal,
        WifiSecurity::Wpa2Wpa3Personal => WifiSecurityMode::Wpa2Wpa3Personal,
        WifiSecurity::Wpa2Enterprise => WifiSecurityMode::Wpa2Enterprise,
        WifiSecurity::Wpa3Enterprise => WifiSecurityMode::Wpa3Enterprise,
        WifiSecurity::Wpa2Wpa3Enterprise => WifiSecurityMode::Wpa2Wpa3Enterprise,
    }
}

fn map_broadcast_type(t: &WifiBroadcastType) -> String {
    match t {
        WifiBroadcastType::Standard => "STANDARD".into(),
        WifiBroadcastType::IotOptimized => "IOT_OPTIMIZED".into(),
    }
}

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct WifiRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "SSID")]
    name: String,
    #[tabled(rename = "Type")]
    btype: String,
    #[tabled(rename = "Security")]
    security: String,
    #[tabled(rename = "Enabled")]
    enabled: String,
    #[tabled(rename = "Bands")]
    bands: String,
}

#[derive(Tabled)]
struct NeighborRow {
    #[tabled(rename = "BSSID")]
    bssid: String,
    #[tabled(rename = "SSID")]
    ssid: String,
    #[tabled(rename = "Ch")]
    channel: String,
    #[tabled(rename = "Signal")]
    signal: String,
    #[tabled(rename = "Radio")]
    radio: String,
    #[tabled(rename = "Security")]
    security: String,
    #[tabled(rename = "Observer")]
    observer: String,
}

#[derive(Tabled, Serialize)]
struct ChannelRow {
    #[tabled(rename = "Band")]
    band: String,
    #[tabled(rename = "Channels")]
    channels: String,
    #[tabled(rename = "Count")]
    count: String,
}

fn wifi_row(w: &Arc<WifiBroadcast>, p: &output::Painter) -> WifiRow {
    WifiRow {
        id: p.id(&w.id.to_string()),
        name: p.name(&w.name),
        btype: p.muted(&format!("{:?}", w.broadcast_type)),
        security: p.muted(&format!("{:?}", w.security)),
        enabled: p.enabled(w.enabled),
        bands: p.number(
            &w.frequencies_ghz
                .iter()
                .map(|f| format!("{f}GHz"))
                .collect::<Vec<_>>()
                .join(", "),
        ),
    }
}

fn neighbor_row(ap: &RogueAp, p: &output::Painter) -> NeighborRow {
    NeighborRow {
        bssid: p.mac(&ap.bssid),
        ssid: p.name(ap.essid.as_deref().unwrap_or("<hidden>")),
        channel: p.number(
            &ap.channel
                .map_or_else(|| "-".into(), |channel| channel.to_string()),
        ),
        signal: p.number(
            &ap.signal
                .map_or_else(|| "-".into(), |signal| format!("{signal} dBm")),
        ),
        radio: p.muted(ap.radio.as_deref().unwrap_or("-")),
        security: p.muted(ap.security.as_deref().unwrap_or("-")),
        observer: p.mac(ap.ap_mac.as_deref().unwrap_or("-")),
    }
}

/// Expand a single regulatory record into per-band rows for table display.
fn channel_rows(record: &ChannelAvailability, p: &output::Painter) -> Vec<ChannelRow> {
    let mut rows = Vec::new();
    let bands: &[(&str, &Option<Vec<i32>>)] = &[
        ("2.4 GHz", &record.channels_ng),
        ("5 GHz", &record.channels_na),
        ("5 GHz DFS", &record.channels_na_dfs),
        ("6 GHz", &record.channels_6e),
    ];
    for &(label, channels) in bands {
        if let Some(chs) = channels
            && !chs.is_empty()
        {
            rows.push(ChannelRow {
                band: p.name(label),
                channels: p.muted(
                    &chs.iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
                count: p.number(&chs.len().to_string()),
            });
        }
    }
    rows
}

fn detail(w: &Arc<WifiBroadcast>) -> String {
    [
        format!("ID:         {}", w.id),
        format!("SSID:       {}", w.name),
        format!("Enabled:    {}", w.enabled),
        format!("Type:       {:?}", w.broadcast_type),
        format!("Security:   {:?}", w.security),
        format!("Hidden:     {}", w.hidden),
        format!("Fast Roam:  {}", w.fast_roaming),
        format!("Band Steer: {}", w.band_steering),
        format!("MLO:        {}", w.mlo_enabled),
        format!("Hotspot:    {}", w.hotspot_enabled),
        format!(
            "Network:    {}",
            w.network_id
                .as_ref()
                .map_or_else(|| "-".into(), ToString::to_string)
        ),
    ]
    .join("\n")
}

// ── Handler ─────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
pub async fn handle(
    controller: &Controller,
    args: WifiArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    let p = output::Painter::new(global);

    match args.command {
        WifiCommand::List(list) => {
            util::ensure_integration_access(controller, "wifi").await?;

            let all = controller.wifi_broadcasts_snapshot();
            let snap = util::apply_list_args(all.iter().cloned(), &list, |w, filter| {
                util::matches_json_filter(w, filter)
            });
            let out = output::render_list(
                &global.output,
                &snap,
                |w| wifi_row(w, &p),
                |w| w.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        WifiCommand::Get { id } => {
            util::ensure_integration_access(controller, "wifi").await?;

            let entity_id = unifly_api::EntityId::from(id.clone());
            match controller.get_wifi_broadcast_detail(&entity_id).await {
                Ok(w) => {
                    let w = std::sync::Arc::new(w);
                    let out =
                        output::render_single(&global.output, &w, detail, |w| w.id.to_string());
                    output::print_output(&out, global.quiet);
                }
                Err(_) => {
                    return Err(CliError::NotFound {
                        resource_type: "wifi".into(),
                        identifier: id,
                        list_command: "wifi list".into(),
                    });
                }
            }
            Ok(())
        }

        WifiCommand::Neighbors { within, limit, all } => {
            let aps = controller.list_rogue_aps(within).await?;
            let max = if all { aps.len() } else { limit.unwrap_or(25) };
            let truncated = aps.len() > max;
            let display: Vec<_> = aps.into_iter().take(max).collect();
            let out = output::render_list(
                &global.output,
                &display,
                |ap| neighbor_row(ap, &p),
                |ap| ap.bssid.clone(),
            );
            output::print_output(&out, global.quiet);
            if truncated && !global.quiet {
                eprintln!("Showing {max} of more results. Use --all or --limit <n> to see more.");
            }
            Ok(())
        }

        WifiCommand::Channels => {
            let records = controller.list_channels().await?;
            // Print country header, then per-band channel rows.
            for record in &records {
                let country = record
                    .name
                    .as_deref()
                    .or(record.key.as_deref())
                    .unwrap_or("Unknown");
                let code = record.code.as_deref().unwrap_or("-");
                if !global.quiet && matches!(global.output, crate::cli::args::OutputFormat::Table) {
                    eprintln!("Country: {country} ({code})");
                }
                let rows: Vec<ChannelRow> = channel_rows(record, &p);
                let out = output::render_list(
                    &global.output,
                    &rows,
                    |row| ChannelRow {
                        band: row.band.clone(),
                        channels: row.channels.clone(),
                        count: row.count.clone(),
                    },
                    |row| row.band.clone(),
                );
                output::print_output(&out, global.quiet);
            }
            Ok(())
        }

        WifiCommand::Create {
            from_file,
            name,
            broadcast_type,
            network,
            security,
            passphrase,
            frequencies,
            hidden,
            band_steering,
            fast_roaming,
        } => {
            util::ensure_integration_access(controller, "wifi").await?;

            let req = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                CreateWifiBroadcastRequest {
                    name: name.clone().unwrap_or_default(),
                    ssid: name.unwrap_or_default(),
                    security_mode: map_security(&security),
                    passphrase,
                    enabled: true,
                    network_id: network.map(EntityId::from),
                    hide_ssid: hidden,
                    broadcast_type: Some(map_broadcast_type(&broadcast_type)),
                    frequencies_ghz: frequencies,
                    band_steering,
                    fast_roaming: if fast_roaming { Some(true) } else { None },
                }
            };

            let result = controller
                .execute(CoreCommand::CreateWifiBroadcast(req))
                .await?;
            if let unifly_api::CommandResult::WifiBroadcast(wifi) = result {
                let wifi = Arc::new(wifi);
                let out =
                    output::render_single(&global.output, &wifi, detail, |w| w.id.to_string());
                output::print_output(&out, global.quiet);
            }
            if !global.quiet {
                eprintln!("WiFi broadcast created");
            }
            Ok(())
        }

        WifiCommand::Update {
            id,
            from_file,
            name,
            passphrase,
            enabled,
        } => {
            util::ensure_integration_access(controller, "wifi").await?;

            if from_file.is_none() && name.is_none() && passphrase.is_none() && enabled.is_none() {
                return Err(CliError::Validation {
                    field: "update".into(),
                    reason: "at least one update flag or --from-file is required".into(),
                });
            }
            let update = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                UpdateWifiBroadcastRequest {
                    name,
                    ssid: None,
                    security_mode: None,
                    passphrase,
                    enabled,
                    hide_ssid: None,
                }
            };

            let eid = EntityId::from(id);
            controller
                .execute(CoreCommand::UpdateWifiBroadcast { id: eid, update })
                .await?;
            if !global.quiet {
                eprintln!("WiFi broadcast updated");
            }
            Ok(())
        }

        WifiCommand::Delete { id, force } => {
            util::ensure_integration_access(controller, "wifi").await?;

            let eid = EntityId::from(id.clone());
            if !util::confirm(&format!("Delete WiFi broadcast {id}?"), global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::DeleteWifiBroadcast { id: eid, force })
                .await?;
            if !global.quiet {
                eprintln!("WiFi broadcast deleted");
            }
            Ok(())
        }
    }
}
