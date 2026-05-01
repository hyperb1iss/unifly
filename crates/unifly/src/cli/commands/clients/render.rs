use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use tabled::Tabled;
use unifly_api::Client;
use unifly_api::session_models::SessionUserEntry;

use crate::cli::output::Painter;

#[derive(Tabled)]
pub(super) struct ClientRow {
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "IP")]
    ip: String,
    #[tabled(rename = "Type")]
    ctype: String,
    #[tabled(rename = "Uplink")]
    uplink: String,
}

pub(super) fn client_row(client: &Arc<Client>, painter: &Painter) -> ClientRow {
    let name = client
        .name
        .clone()
        .or_else(|| client.hostname.clone())
        .unwrap_or_default();
    ClientRow {
        name: painter.name(&name),
        ip: painter.ip(&client.ip.map(|ip| ip.to_string()).unwrap_or_default()),
        ctype: painter.muted(&format!("{:?}", client.client_type)),
        uplink: painter.mac(
            &client
                .uplink_device_mac
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
        ),
    }
}

pub(super) fn detail(client: &Arc<Client>) -> String {
    let mut lines = vec![
        format!("ID:        {}", client.id),
        format!("Name:      {}", client.name.as_deref().unwrap_or("-")),
        format!("Hostname:  {}", client.hostname.as_deref().unwrap_or("-")),
        format!("MAC:       {}", client.mac),
        format!(
            "IP:        {}",
            client.ip.map_or_else(|| "-".into(), |ip| ip.to_string())
        ),
        format!("Type:      {:?}", client.client_type),
        format!("Guest:     {}", client.is_guest),
        format!("Blocked:   {}", client.blocked),
    ];
    if let Some(port) = client.switch_port {
        lines.push(format!("Switch Port: {port}"));
    }
    if client.use_fixedip {
        lines.push(format!(
            "Fixed IP:  {}",
            client.fixed_ip.map_or("-".into(), |ip| ip.to_string())
        ));
    }
    if let Some(wireless) = &client.wireless {
        lines.push(format!(
            "SSID:      {}",
            wireless.ssid.as_deref().unwrap_or("-")
        ));
        if let Some(signal) = wireless.signal_dbm {
            lines.push(format!("Signal:    {signal} dBm"));
        }
    }
    if let Some(os_name) = &client.os_name {
        lines.push(format!("OS:        {os_name}"));
    }
    lines.join("\n")
}

// ── DHCP Reservations ────────────────────────────────────────────

/// Clean serializable representation of a DHCP reservation.
#[derive(Debug, Serialize)]
pub(super) struct Reservation {
    pub mac: String,
    pub name: Option<String>,
    pub fixed_ip: Option<String>,
    pub network_id: Option<String>,
}

impl From<&SessionUserEntry> for Reservation {
    fn from(u: &SessionUserEntry) -> Self {
        Self {
            mac: u.mac.clone(),
            name: u.name.clone(),
            fixed_ip: u.fixed_ip.clone(),
            network_id: u.network_id.clone(),
        }
    }
}

#[derive(Tabled)]
pub(super) struct ReservationRow {
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "MAC")]
    mac: String,
    #[tabled(rename = "Fixed IP")]
    fixed_ip: String,
    #[tabled(rename = "Network")]
    network: String,
}

pub(super) fn reservation_row(reservation: &Reservation, painter: &Painter) -> ReservationRow {
    ReservationRow {
        name: painter.name(reservation.name.as_deref().unwrap_or("-")),
        mac: painter.mac(&reservation.mac),
        fixed_ip: painter.ip(reservation.fixed_ip.as_deref().unwrap_or("-")),
        network: painter.id(reservation.network_id.as_deref().unwrap_or("-")),
    }
}

// ── Client Roams ────────────────────────────────────────────────

#[derive(Tabled)]
pub(super) struct RoamRow {
    #[tabled(rename = "Time")]
    time: String,
    #[tabled(rename = "Event")]
    event: String,
    #[tabled(rename = "From")]
    from_ap: String,
    #[tabled(rename = "To")]
    to_ap: String,
    #[tabled(rename = "SSID")]
    ssid: String,
    #[tabled(rename = "Signal")]
    signal: String,
    #[tabled(rename = "Channel")]
    channel: String,
    #[tabled(rename = "Band")]
    band: String,
}

fn json_str<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
}

fn json_i64(value: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_i64))
}

fn format_event_timestamp(timestamp: i64) -> String {
    let formatted = if timestamp.abs() >= 1_000_000_000_000 {
        DateTime::<Utc>::from_timestamp_millis(timestamp)
    } else {
        DateTime::<Utc>::from_timestamp(timestamp, 0)
    };
    formatted.map_or_else(
        || timestamp.to_string(),
        |datetime| datetime.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
    )
}

fn append_neighbor_lines(lines: &mut Vec<String>, data: &Value, painter: &Painter) {
    let Some(neighbors) = data.get("nearest_neighbors").and_then(Value::as_array) else {
        return;
    };
    if neighbors.is_empty() {
        return;
    }

    lines.push(String::new());
    lines.push(format!("Nearest Neighbors ({}):", neighbors.len()));
    for neighbor in neighbors.iter().take(10) {
        let bssid = json_str(neighbor, &["bssid"]).unwrap_or("-");
        let channel =
            json_i64(neighbor, &["channel"]).map_or_else(|| "-".into(), |value| value.to_string());
        // wifiman may nest signal as [{"signal": N}] or return it flat as N
        let signal = neighbor
            .get("signal")
            .and_then(|v| {
                v.as_i64().or_else(|| {
                    v.as_array()
                        .and_then(|arr| arr.first())
                        .and_then(|entry| entry.get("signal"))
                        .and_then(Value::as_i64)
                })
            })
            .map_or_else(|| "-".into(), |value| format!("{value} dBm"));
        lines.push(format!(
            "  {} ch:{} signal:{}",
            painter.mac(bssid),
            painter.number(&channel),
            painter.number(&signal),
        ));
    }
}

fn append_uplink_lines(lines: &mut Vec<String>, data: &Value, painter: &Painter) {
    let Some(uplinks) = data.get("uplink_devices").and_then(Value::as_array) else {
        return;
    };
    if uplinks.is_empty() {
        return;
    }

    lines.push(String::new());
    lines.push(format!("Uplink Chain ({} hops):", uplinks.len()));
    for uplink in uplinks {
        let name = json_str(uplink, &["display_name", "device_name", "name"]).unwrap_or("-");
        let experience = json_i64(uplink, &["experience", "wifi_experience"])
            .map_or_else(|| "-".into(), |value| format!("{value}/100"));
        lines.push(format!(
            "  {} experience:{}",
            painter.name(name),
            painter.number(&experience),
        ));
    }
}

/// Extract the `name` field from a nested parameter object.
///
/// The v2 `system-log/client-connection` response nests all fields under
/// `parameters.PARAM_KEY.name` rather than flat top-level keys.
fn roam_param_name<'a>(event: &'a Value, param_key: &str) -> Option<&'a str> {
    event
        .get("parameters")
        .and_then(|p| p.get(param_key))
        .and_then(|p| p.get("name"))
        .and_then(Value::as_str)
}

/// Map UniFi radio band codes to human-readable labels.
///
/// Two code families exist: stat/sta uses "ng"/"na"/"6e", while
/// wifiman and system-log endpoints use "2.4g"/"5g"/"6g".
fn format_radio_band(code: &str) -> &str {
    match code {
        "ng" | "2.4g" => "2.4 GHz",
        "na" | "5g" => "5 GHz",
        "6e" | "6g" => "6 GHz",
        other => other,
    }
}

pub(super) fn roam_row(event: &Value, painter: &Painter) -> RoamRow {
    let time =
        json_i64(event, &["timestamp", "time"]).map_or_else(|| "-".into(), format_event_timestamp);

    let signal = roam_param_name(event, "SIGNAL_STRENGTH")
        .map_or_else(|| "-".into(), |s| format!("{s} dBm"));
    let channel = roam_param_name(event, "CHANNEL").unwrap_or("-");
    let band = roam_param_name(event, "RADIO_BAND").map_or("-", format_radio_band);
    let from_ap = roam_param_name(event, "DEVICE_FROM").unwrap_or("-");
    let to_ap = roam_param_name(event, "DEVICE_TO").unwrap_or("-");
    let ssid = roam_param_name(event, "WLAN").unwrap_or("-");

    RoamRow {
        time: painter.muted(&time),
        event: painter.name(json_str(event, &["key", "event_type", "eventType"]).unwrap_or("-")),
        from_ap: painter.muted(from_ap),
        to_ap: painter.muted(to_ap),
        ssid: painter.name(ssid),
        signal: painter.number(&signal),
        channel: painter.number(channel),
        band: painter.muted(band),
    }
}

// ── Wi-Fi Experience ────────────────────────────────────────────

pub(super) fn wifi_experience_detail(data: &Value, painter: &Painter) -> String {
    let mut lines = vec![
        format!(
            "Wi-Fi Experience: {}",
            painter.number(
                &json_i64(data, &["wifi_experience"])
                    .map_or_else(|| "-".into(), |score| format!("{score}/100")),
            )
        ),
        format!(
            "Signal:           {}",
            painter.number(
                &json_i64(data, &["signal"])
                    .map_or_else(|| "-".into(), |signal| format!("{signal} dBm")),
            )
        ),
        format!(
            "Noise:            {}",
            painter.number(
                &json_i64(data, &["noise"])
                    .map_or_else(|| "-".into(), |noise| format!("{noise} dBm")),
            )
        ),
        format!(
            "Channel:          {}",
            painter.number(
                &json_i64(data, &["channel"])
                    .map_or_else(|| "-".into(), |channel| channel.to_string()),
            )
        ),
        format!(
            "Band:             {}",
            painter.muted(
                json_str(data, &["wlan_band", "band"]).map_or("-", |code| match code {
                    "2.4g" => "2.4 GHz",
                    "5g" => "5 GHz",
                    "6g" => "6 GHz",
                    other => other,
                })
            )
        ),
        format!(
            "Channel Width:    {}",
            painter.number(
                &json_i64(data, &["channel_width"])
                    .map_or_else(|| "-".into(), |width| format!("{width} MHz")),
            )
        ),
        format!(
            "Protocol:         {}",
            painter.muted(json_str(data, &["radio_protocol", "protocol"]).unwrap_or("-"))
        ),
        format!(
            "Link Down:        {}",
            painter.number(
                &json_i64(data, &["link_download_rate_kbps", "download_rate_kbps"])
                    .map_or_else(|| "-".into(), |rate| format!("{rate} Kbps")),
            )
        ),
        format!(
            "Link Up:          {}",
            painter.number(
                &json_i64(data, &["link_upload_rate_kbps", "upload_rate_kbps"])
                    .map_or_else(|| "-".into(), |rate| format!("{rate} Kbps")),
            )
        ),
    ];

    if let Some(ssid) = json_str(data, &["ssid", "essid"]) {
        lines.insert(1, format!("SSID:             {}", painter.name(ssid)));
    }

    if let Some(access_point) = json_str(data, &["ap_name", "ap", "ap_mac"]) {
        lines.insert(
            2,
            format!("Access Point:     {}", painter.muted(access_point)),
        );
    }

    append_neighbor_lines(&mut lines, data, painter);
    append_uplink_lines(&mut lines, data, painter);

    lines.join("\n")
}
