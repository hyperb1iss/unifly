use serde_json::Value;

use crate::model::device::{PoeInfo, Port, PortConnector, PortState, Radio};
use crate::session::models::{SessionPortEntry, SessionRadioEntry, SessionRadioStats};

fn parse_port_state(raw: &str) -> PortState {
    match raw {
        "UP" | "up" => PortState::Up,
        "DOWN" | "down" => PortState::Down,
        _ => PortState::Unknown,
    }
}

fn parse_port_connector(raw: &str) -> Option<PortConnector> {
    match raw {
        "RJ45" | "rj45" => Some(PortConnector::Rj45),
        "SFP" | "sfp" => Some(PortConnector::Sfp),
        "SFPPLUS" | "SFP+" | "sfp+" => Some(PortConnector::SfpPlus),
        "SFP28" | "sfp28" => Some(PortConnector::Sfp28),
        "QSFP28" | "qsfp28" => Some(PortConnector::Qsfp28),
        _ => None,
    }
}

pub(crate) fn parse_integration_ports(interfaces: &Value) -> Vec<Port> {
    let Some(ports) = interfaces.get("ports").and_then(Value::as_array) else {
        return Vec::new();
    };
    ports
        .iter()
        .filter_map(|p| {
            let idx = p.get("idx").and_then(Value::as_u64)?;
            #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
            Some(Port {
                index: idx as u32,
                name: p.get("name").and_then(Value::as_str).map(String::from),
                state: p
                    .get("state")
                    .and_then(Value::as_str)
                    .map_or(PortState::Unknown, parse_port_state),
                speed_mbps: p.get("speedMbps").and_then(Value::as_u64).map(|v| v as u32),
                max_speed_mbps: p
                    .get("maxSpeedMbps")
                    .and_then(Value::as_u64)
                    .map(|v| v as u32),
                connector: p
                    .get("connector")
                    .and_then(Value::as_str)
                    .and_then(parse_port_connector),
                poe: p.get("poe").map(|poe| PoeInfo {
                    standard: poe
                        .get("standard")
                        .and_then(Value::as_str)
                        .map(String::from),
                    enabled: poe.get("enabled").and_then(Value::as_bool).unwrap_or(false),
                    state: poe
                        .get("state")
                        .and_then(Value::as_str)
                        .map_or(PortState::Unknown, parse_port_state),
                }),
            })
        })
        .collect()
}

pub(crate) fn parse_integration_radios(interfaces: &Value) -> Vec<Radio> {
    let Some(radios) = interfaces.get("radios").and_then(Value::as_array) else {
        return Vec::new();
    };
    radios
        .iter()
        .filter_map(|r| {
            #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
            let freq = r.get("frequencyGHz").and_then(Value::as_f64)? as f32;
            #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
            Some(Radio {
                frequency_ghz: freq,
                channel: r.get("channel").and_then(Value::as_u64).map(|v| v as u32),
                channel_width_mhz: r
                    .get("channelWidthMHz")
                    .and_then(Value::as_u64)
                    .map(|v| v as u32),
                wlan_standard: r
                    .get("wlanStandard")
                    .and_then(Value::as_str)
                    .map(String::from),
                tx_retries_pct: r.get("txRetriesPct").and_then(Value::as_f64),
                channel_utilization_pct: None,
            })
        })
        .collect()
}

pub(crate) fn enrich_radios_from_stats(radios: &mut [Radio], stats_interfaces: &Value) {
    let Some(stats_radios) = stats_interfaces.get("radios").and_then(Value::as_array) else {
        return;
    };
    for sr in stats_radios {
        #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
        let Some(freq) = sr
            .get("frequencyGHz")
            .and_then(Value::as_f64)
            .map(|f| f as f32)
        else {
            continue;
        };
        let retries = sr.get("txRetriesPct").and_then(Value::as_f64);
        if let Some(radio) = radios
            .iter_mut()
            .find(|r| (r.frequency_ghz - freq).abs() < 0.1)
            .filter(|r| r.tx_retries_pct.is_none())
        {
            radio.tx_retries_pct = retries;
        }
    }
}

/// Convert session `port_table` entries into [`Port`] values, skipping any
/// entry without a `port_idx` (the controller's 1-based port number).
pub(crate) fn parse_session_ports(ports: &[SessionPortEntry]) -> Vec<Port> {
    ports
        .iter()
        .filter_map(|p| {
            let idx = p.port_idx?;
            let up = p.up.unwrap_or(false);
            Some(Port {
                index: idx,
                name: p.name.clone(),
                state: if up { PortState::Up } else { PortState::Down },
                speed_mbps: p.speed,
                max_speed_mbps: None,
                connector: p.media.as_deref().and_then(|m| match m {
                    "GE" | "FE" => Some(PortConnector::Rj45),
                    "SFP" => Some(PortConnector::Sfp),
                    "SFP+" => Some(PortConnector::SfpPlus),
                    _ => None,
                }),
                poe: if p.port_poe.unwrap_or(false) {
                    Some(PoeInfo {
                        standard: p.poe_caps.map(|caps| {
                            match caps {
                                7 => "802.3bt",
                                3 => "802.3at",
                                _ => "802.3af",
                            }
                            .to_owned()
                        }),
                        enabled: p.poe_enable.unwrap_or(false),
                        state: if p.poe_good.unwrap_or(false) {
                            PortState::Up
                        } else {
                            PortState::Down
                        },
                    })
                } else {
                    None
                },
            })
        })
        .collect()
}

fn session_radio_freq(band: &str) -> Option<f32> {
    match band {
        "ng" => Some(2.4),
        "na" => Some(5.0),
        "6e" => Some(6.0),
        _ => None,
    }
}

/// Merge session `radio_table` entries with their `radio_table_stats`
/// counterparts, matched by radio name. The Integration `interfaces` payload
/// is not the source for radios.
pub(crate) fn parse_session_radios(
    radios: &[SessionRadioEntry],
    stats: &[SessionRadioStats],
) -> Vec<Radio> {
    radios
        .iter()
        .filter_map(|r| {
            let band = r.radio.as_deref()?;
            let freq = session_radio_freq(band)?;
            let stat = stats.iter().find(|s| s.radio.as_deref() == Some(band));
            Some(Radio {
                frequency_ghz: freq,
                channel: r.channel.or_else(|| stat.and_then(|s| s.channel)),
                channel_width_mhz: r.ht.as_deref().and_then(|ht| ht.parse::<u32>().ok()),
                wlan_standard: None,
                tx_retries_pct: None,
                channel_utilization_pct: stat.and_then(|s| s.cu_total),
            })
        })
        .collect()
}
