use std::sync::Arc;

use serde::Serialize;
use tabled::Tabled;
use unifly_api::{
    Client, Device, PoeMode, PortMode, PortProfile, PortSpeedSetting, PortState, StpState,
};

use crate::cli::output::Painter;

#[derive(Tabled)]
pub(super) struct DeviceRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Model")]
    model: String,
    #[tabled(rename = "Type")]
    dtype: String,
    #[tabled(rename = "State")]
    state: String,
    #[tabled(rename = "IP")]
    ip: String,
    #[tabled(rename = "MAC")]
    mac: String,
}

#[derive(Tabled)]
pub(super) struct PendingDeviceRow {
    #[tabled(rename = "IP")]
    ip: String,
    #[tabled(rename = "Model")]
    model: String,
    #[tabled(rename = "MAC")]
    mac: String,
    #[tabled(rename = "State")]
    state: String,
    #[tabled(rename = "Version")]
    firmware: String,
    #[tabled(rename = "Supported")]
    supported: String,
}

#[derive(Tabled)]
pub(super) struct DeviceTagRow {
    #[tabled(rename = "ID")]
    pub(super) id: String,
    #[tabled(rename = "Name")]
    pub(super) name: String,
}

pub(super) fn device_row(device: &Arc<Device>, painter: &Painter) -> DeviceRow {
    DeviceRow {
        id: painter.id(&device.id.to_string()),
        name: painter.name(&device.name.clone().unwrap_or_default()),
        model: painter.muted(&device.model.clone().unwrap_or_default()),
        dtype: painter.muted(&format!("{:?}", device.device_type)),
        state: painter.state(&format!("{:?}", device.state)),
        ip: painter.ip(&device.ip.map(|ip| ip.to_string()).unwrap_or_default()),
        mac: painter.mac(&device.mac.to_string()),
    }
}

pub(super) fn detail(device: &Arc<Device>) -> String {
    let mut lines = vec![
        format!("ID:       {}", device.id),
        format!("Name:     {}", device.name.as_deref().unwrap_or("-")),
        format!("MAC:      {}", device.mac),
        format!(
            "IP:       {}",
            device.ip.map_or_else(|| "-".into(), |ip| ip.to_string())
        ),
        format!("Model:    {}", device.model.as_deref().unwrap_or("-")),
        format!("Type:     {:?}", device.device_type),
        format!("State:    {:?}", device.state),
        format!(
            "Firmware: {}",
            device.firmware_version.as_deref().unwrap_or("-")
        ),
    ];
    if let Some(uptime) = device.stats.uptime_secs {
        lines.push(format!("Uptime:   {uptime}s"));
    }
    if let Some(cpu) = device.stats.cpu_utilization_pct {
        lines.push(format!("CPU:      {cpu:.1}%"));
    }
    if let Some(memory) = device.stats.memory_utilization_pct {
        lines.push(format!("Memory:   {memory:.1}%"));
    }
    lines.join("\n")
}

pub(super) fn stats_detail(device: &Arc<Device>) -> String {
    [
        format!("ID:          {}", device.id),
        format!("Name:        {}", device.name.as_deref().unwrap_or("-")),
        format!("MAC:         {}", device.mac),
        format!(
            "Uptime:      {}",
            device
                .stats
                .uptime_secs
                .map_or_else(|| "-".into(), |value| format!("{value}s"))
        ),
        format!(
            "CPU:         {}",
            device
                .stats
                .cpu_utilization_pct
                .map_or_else(|| "-".into(), |value| format!("{value:.1}%"))
        ),
        format!(
            "Memory:      {}",
            device
                .stats
                .memory_utilization_pct
                .map_or_else(|| "-".into(), |value| format!("{value:.1}%"))
        ),
        format!(
            "Load Avg 1m: {}",
            device
                .stats
                .load_average_1m
                .map_or_else(|| "-".into(), |value| format!("{value:.2}"))
        ),
        format!(
            "Load Avg 5m: {}",
            device
                .stats
                .load_average_5m
                .map_or_else(|| "-".into(), |value| format!("{value:.2}"))
        ),
        format!(
            "Load Avg15m: {}",
            device
                .stats
                .load_average_15m
                .map_or_else(|| "-".into(), |value| format!("{value:.2}"))
        ),
    ]
    .join("\n")
}

fn pending_string<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
}

pub(super) fn pending_device_row(value: &serde_json::Value, painter: &Painter) -> PendingDeviceRow {
    PendingDeviceRow {
        ip: painter.ip(pending_string(value, "ipAddress")),
        model: painter.muted(pending_string(value, "model")),
        mac: painter.mac(
            value
                .get("macAddress")
                .or_else(|| value.get("mac"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or(""),
        ),
        state: {
            let state = pending_string(value, "state");
            painter.state(if state.is_empty() { "PENDING" } else { state })
        },
        firmware: painter.muted(pending_string(value, "firmwareVersion")),
        supported: painter.enabled(
            value
                .get("supported")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        ),
    }
}

pub(super) fn pending_device_identity(value: &serde_json::Value) -> String {
    value
        .get("macAddress")
        .or_else(|| value.get("mac"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| value.get("ipAddress").and_then(serde_json::Value::as_str))
        .unwrap_or("")
        .to_owned()
}

pub(super) fn device_tag_row(value: &serde_json::Value, painter: &Painter) -> DeviceTagRow {
    DeviceTagRow {
        id: painter.id(value
            .get("id")
            .or_else(|| value.get("_id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")),
        name: painter.name(
            value
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(""),
        ),
    }
}

#[derive(Tabled)]
pub(super) struct PortRow {
    #[tabled(rename = "#")]
    pub(super) index: String,
    #[tabled(rename = "Name")]
    pub(super) name: String,
    #[tabled(rename = "Link")]
    pub(super) link: String,
    #[tabled(rename = "Mode")]
    pub(super) mode: String,
    #[tabled(rename = "Native VLAN")]
    pub(super) native: String,
    #[tabled(rename = "Tagged VLANs")]
    pub(super) tagged: String,
    #[tabled(rename = "PoE")]
    pub(super) poe: String,
    #[tabled(rename = "Speed")]
    pub(super) speed: String,
    #[tabled(rename = "STP")]
    pub(super) stp: String,
}

pub(super) fn port_row(profile: &PortProfile, painter: &Painter) -> PortRow {
    PortRow {
        index: painter.number(&profile.index.to_string()),
        name: painter.name(profile.name.as_deref().unwrap_or("")),
        link: painter.state(match profile.link_state {
            PortState::Up => "UP",
            PortState::Down => "DOWN",
            PortState::Unknown => "?",
        }),
        mode: painter.keyword(match profile.mode {
            PortMode::Access => "access",
            PortMode::Trunk => "trunk",
            PortMode::Mirror => "mirror",
            PortMode::Unknown => "-",
        }),
        native: painter.muted(&format_native(profile)),
        tagged: painter.muted(&format_tagged(profile)),
        poe: painter.muted(format_poe(profile.poe_mode)),
        speed: painter.muted(&format_speed(profile)),
        stp: painter.muted(match profile.stp_state {
            StpState::Disabled => "disabled",
            StpState::Blocking => "blocking",
            StpState::Listening => "listening",
            StpState::Learning => "learning",
            StpState::Forwarding => "forwarding",
            StpState::Broken => "broken",
            StpState::Unknown => "-",
        }),
    }
}

fn format_native(profile: &PortProfile) -> String {
    match (
        profile.native_network_name.as_deref(),
        profile.native_vlan_id,
    ) {
        (Some(name), Some(vlan)) => format!("{name} ({vlan})"),
        (Some(name), None) => name.to_owned(),
        (None, Some(vlan)) => format!("vlan {vlan}"),
        (None, None) => profile
            .native_network_id
            .clone()
            .unwrap_or_else(|| "-".into()),
    }
}

fn format_tagged(profile: &PortProfile) -> String {
    if profile.tagged_all {
        return "all".into();
    }
    if profile.tagged_network_names.is_empty() && profile.tagged_vlan_ids.is_empty() {
        if profile.tagged_network_ids.is_empty() {
            return "-".into();
        }
        return profile.tagged_network_ids.join(",");
    }
    if !profile.tagged_network_names.is_empty() {
        return profile.tagged_network_names.join(",");
    }
    profile
        .tagged_vlan_ids
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn format_poe(mode: Option<PoeMode>) -> &'static str {
    match mode {
        Some(PoeMode::Auto) => "auto",
        Some(PoeMode::Off) => "off",
        Some(PoeMode::Passive24V) => "pasv24",
        Some(PoeMode::Passthrough) => "passthrough",
        Some(PoeMode::Other) => "other",
        None => "-",
    }
}

fn format_speed(profile: &PortProfile) -> String {
    let cfg_label = profile.speed_setting.map(|s| match s {
        PortSpeedSetting::Auto => "auto".to_owned(),
        other => other
            .as_mbps()
            .map_or_else(|| "auto".to_owned(), |mbps| mbps.to_string()),
    });
    match (profile.speed_setting, cfg_label, profile.link_speed_mbps) {
        (Some(cfg), Some(label), Some(live))
            if cfg.as_mbps().is_some_and(|pinned| pinned == live) =>
        {
            label
        }
        (_, Some(label), Some(live)) => format!("{label} ({live})"),
        (_, Some(label), None) => label,
        (_, None, Some(live)) => live.to_string(),
        (_, None, None) => "-".into(),
    }
}

pub(super) fn device_tag_identity(value: &serde_json::Value) -> String {
    value
        .get("id")
        .or_else(|| value.get("_id"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_owned()
}

// ── Port enrichment with connected clients (`--with-clients`) ──────

/// One client currently observed on a switch port.
#[derive(Debug, Clone, Serialize)]
pub(super) struct ConnectedClientSummary {
    pub mac: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vlan_id: Option<u16>,
}

/// `PortProfile` plus the wired clients seen on it. `connected_clients`
/// flattens cleanly into the JSON shape expected by `devices ports
/// --with-clients` consumers.
#[derive(Debug, Clone, Serialize)]
pub(super) struct EnrichedPortProfile {
    #[serde(flatten)]
    pub profile: PortProfile,
    pub connected_clients: Vec<ConnectedClientSummary>,
}

#[derive(Tabled)]
pub(super) struct EnrichedPortRow {
    #[tabled(rename = "#")]
    index: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Link")]
    link: String,
    #[tabled(rename = "Mode")]
    mode: String,
    #[tabled(rename = "Native VLAN")]
    native: String,
    #[tabled(rename = "Tagged VLANs")]
    tagged: String,
    #[tabled(rename = "PoE")]
    poe: String,
    #[tabled(rename = "Speed")]
    speed: String,
    #[tabled(rename = "STP")]
    stp: String,
    #[tabled(rename = "Clients")]
    clients: String,
}

pub(super) fn enriched_port_row(
    enriched: &EnrichedPortProfile,
    painter: &Painter,
) -> EnrichedPortRow {
    let base = port_row(&enriched.profile, painter);
    EnrichedPortRow {
        index: base.index,
        name: base.name,
        link: base.link,
        mode: base.mode,
        native: base.native,
        tagged: base.tagged,
        poe: base.poe,
        speed: base.speed,
        stp: base.stp,
        clients: painter.muted(&format_client_count(&enriched.connected_clients)),
    }
}

fn format_client_count(clients: &[ConnectedClientSummary]) -> String {
    match clients.len() {
        0 => "-".into(),
        1 => format!(
            "1: {}",
            clients[0]
                .name
                .as_deref()
                .unwrap_or(clients[0].mac.as_str())
        ),
        n => {
            let head = clients[0]
                .name
                .as_deref()
                .unwrap_or(clients[0].mac.as_str());
            format!("{n}: {head}, …")
        }
    }
}

/// Pair each port profile with the wired clients currently seen on it.
/// `device_mac` filters the clients_snapshot to this device.
pub(super) fn enrich_with_clients(
    profiles: &[PortProfile],
    clients: &[Arc<Client>],
    device_mac: &unifly_api::MacAddress,
) -> Vec<EnrichedPortProfile> {
    use std::collections::HashMap;

    let mut by_port: HashMap<u32, Vec<ConnectedClientSummary>> = HashMap::new();
    for client in clients {
        let Some(uplink) = client.uplink_device_mac.as_ref() else {
            continue;
        };
        if uplink.as_str() != device_mac.as_str() {
            continue;
        }
        let Some(port) = client.switch_port else {
            continue;
        };
        by_port
            .entry(port)
            .or_default()
            .push(ConnectedClientSummary {
                mac: client.mac.to_string(),
                ip: client.ip.map(|ip| ip.to_string()),
                name: client.name.clone().or_else(|| client.hostname.clone()),
                vlan_id: client.vlan,
            });
    }

    for entries in by_port.values_mut() {
        entries.sort_by(|a, b| a.mac.cmp(&b.mac));
    }

    profiles
        .iter()
        .cloned()
        .map(|profile| {
            let connected_clients = by_port.remove(&profile.index).unwrap_or_default();
            EnrichedPortProfile {
                profile,
                connected_clients,
            }
        })
        .collect()
}

// ── ports-export --with-clients marker injection ───────────────────

/// Build a per-port-index map of `// last-seen ...` comment lines
/// suitable for injection into the JSONC output of `ports-export`.
///
/// `timestamp` is shared across all entries (one timestamp per export
/// run, for diff stability). Markers within a port are sorted by MAC.
pub(super) fn build_last_seen_markers(
    clients: &[Arc<Client>],
    device_mac: &unifly_api::MacAddress,
    timestamp: &str,
) -> std::collections::HashMap<u32, Vec<String>> {
    use std::collections::HashMap;

    let mut by_port: HashMap<u32, Vec<(String, Option<String>)>> = HashMap::new();
    for client in clients {
        let Some(uplink) = client.uplink_device_mac.as_ref() else {
            continue;
        };
        if uplink.as_str() != device_mac.as_str() {
            continue;
        }
        let Some(port) = client.switch_port else {
            continue;
        };
        let mac = client.mac.to_string();
        let name = client.name.clone().or_else(|| client.hostname.clone());
        by_port.entry(port).or_default().push((mac, name));
    }

    by_port
        .into_iter()
        .map(|(port, mut entries)| {
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let lines = entries
                .into_iter()
                .map(|(mac, name)| match name {
                    Some(n) => format!("// last-seen {timestamp}: {mac} ({n})"),
                    None => format!("// last-seen {timestamp}: {mac}"),
                })
                .collect();
            (port, lines)
        })
        .collect()
}

/// Inject `// last-seen ...` comment lines into `serde_json::to_string_pretty`
/// output. Looks for each port object's opening `{` (detected by an
/// immediately-following `"index": N,` field) and prepends the matching
/// markers at the same indent.
pub(super) fn inject_last_seen_markers(
    pretty_json: &str,
    markers: &std::collections::HashMap<u32, Vec<String>>,
) -> String {
    let lines: Vec<&str> = pretty_json.lines().collect();
    let mut out = String::with_capacity(pretty_json.len());

    for i in 0..lines.len() {
        let line = lines[i];
        if line.trim_end().ends_with('{')
            && let Some(next) = lines.get(i + 1)
            && let Some(idx) = parse_index_field(next.trim_start())
            && let Some(comments) = markers.get(&idx)
        {
            let indent_len = line.len() - line.trim_start().len();
            let indent = &line[..indent_len];
            for comment in comments {
                out.push_str(indent);
                out.push_str(comment);
                out.push('\n');
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn parse_index_field(s: &str) -> Option<u32> {
    s.strip_prefix("\"index\":")
        .and_then(|rest| rest.trim_start().trim_end_matches(',').trim().parse().ok())
}

#[cfg(test)]
mod tests {
    use super::{pending_device_identity, pending_device_row};
    use crate::cli::args::{ColorMode, GlobalOpts, OutputFormat};
    use crate::cli::output::Painter;

    fn plain_painter() -> Painter {
        Painter::new(&GlobalOpts {
            profile: None,
            controller: None,
            site: None,
            api_key: None,
            host_id: None,
            totp: None,
            no_cache: false,
            demo: false,
            output: OutputFormat::Plain,
            color: ColorMode::Never,
            theme: None,
            verbose: 0,
            quiet: false,
            yes: false,
            insecure: false,
            timeout: 30,
            no_effects: false,
        })
    }

    #[test]
    fn pending_device_row_uses_actual_api_fields() {
        let row = pending_device_row(
            &serde_json::json!({
                "macAddress": "aa:bb:cc:dd:ee:ff",
                "ipAddress": "10.0.0.20",
                "model": "U7-Pro",
                "state": "DISCOVERED",
                "firmwareVersion": "1.2.3",
                "supported": true
            }),
            &plain_painter(),
        );

        assert_eq!(row.ip, "10.0.0.20");
        assert_eq!(row.model, "U7-Pro");
        assert_eq!(row.mac, "aa:bb:cc:dd:ee:ff");
        assert_eq!(row.state, "DISCOVERED");
        assert_eq!(row.firmware, "1.2.3");
        assert_eq!(row.supported, "yes");
    }

    #[test]
    fn pending_device_identity_prefers_mac_address() {
        let identity = pending_device_identity(&serde_json::json!({
            "macAddress": "aa:bb:cc:dd:ee:ff",
            "ipAddress": "10.0.0.20"
        }));
        assert_eq!(identity, "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn inject_last_seen_prepends_markers_at_port_indent() {
        use std::collections::HashMap;

        use super::inject_last_seen_markers;

        let pretty = "{
  \"ports\": [
    {
      \"index\": 1,
      \"name\": \"uplink\"
    },
    {
      \"index\": 9,
      \"name\": \"mac-mini\"
    }
  ]
}";
        let mut markers: HashMap<u32, Vec<String>> = HashMap::new();
        markers.insert(
            9,
            vec!["// last-seen 2026-05-01T14:22Z: aa:bb:cc:dd:ee:ff (Mac Mini)".into()],
        );

        let out = inject_last_seen_markers(pretty, &markers);
        // Marker is inserted before the port-9 `{`, at the same indent.
        let expected = "{
  \"ports\": [
    {
      \"index\": 1,
      \"name\": \"uplink\"
    },
    // last-seen 2026-05-01T14:22Z: aa:bb:cc:dd:ee:ff (Mac Mini)
    {
      \"index\": 9,
      \"name\": \"mac-mini\"
    }
  ]
}
";
        assert_eq!(out, expected);
    }

    #[test]
    fn inject_last_seen_emits_one_line_per_client() {
        use std::collections::HashMap;

        use super::inject_last_seen_markers;

        let pretty = "{
  \"ports\": [
    {
      \"index\": 12,
      \"name\": \"trunk\"
    }
  ]
}";
        let mut markers: HashMap<u32, Vec<String>> = HashMap::new();
        markers.insert(
            12,
            vec![
                "// last-seen 2026-05-01T14:22Z: 11:22:33:44:55:66 (sonos)".into(),
                "// last-seen 2026-05-01T14:22Z: aa:bb:cc:dd:ee:ff".into(),
            ],
        );
        let out = inject_last_seen_markers(pretty, &markers);
        assert!(out.contains("// last-seen 2026-05-01T14:22Z: 11:22:33:44:55:66 (sonos)"));
        assert!(out.contains("// last-seen 2026-05-01T14:22Z: aa:bb:cc:dd:ee:ff"));
        // Both markers appear before the port-12 opening brace.
        let sonos_pos = out.find("11:22:33:44:55:66").expect("marker present");
        let port_pos = out.find("\"index\": 12").expect("port object present");
        assert!(sonos_pos < port_pos);
    }

    #[test]
    fn inject_last_seen_is_noop_when_port_has_no_markers() {
        use std::collections::HashMap;

        use super::inject_last_seen_markers;

        let pretty = "{
  \"ports\": [
    {
      \"index\": 1
    }
  ]
}";
        let markers: HashMap<u32, Vec<String>> = HashMap::new();
        let out = inject_last_seen_markers(pretty, &markers);
        assert_eq!(out, format!("{pretty}\n"));
    }
}
