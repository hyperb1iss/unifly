use chrono::Utc;

use crate::integration_types;
use crate::model::common::{Bandwidth, DataSource};
use crate::model::device::{Device, DeviceState, DeviceStats, DeviceType};
use crate::model::entity_id::{EntityId, MacAddress};
use crate::session::models::SessionDevice;

use super::helpers::{epoch_to_datetime, parse_ip, parse_iso, parse_legacy_wan_ipv6};
use super::interface::{
    parse_integration_ports, parse_integration_radios, parse_session_ports, parse_session_radios,
};

// ── Session API ──────────────────────────────────────────────────

fn infer_device_type(device_type: &str, model: Option<&String>) -> DeviceType {
    match device_type {
        "uap" => DeviceType::AccessPoint,
        "usw" => DeviceType::Switch,
        "ugw" | "udm" => DeviceType::Gateway,
        _ => {
            if let Some(m) = model {
                let upper = m.to_uppercase();
                if upper.starts_with("UAP") || upper.starts_with("U6") || upper.starts_with("U7") {
                    DeviceType::AccessPoint
                } else if upper.starts_with("USW") || upper.starts_with("USL") {
                    DeviceType::Switch
                } else if upper.starts_with("UGW")
                    || upper.starts_with("UDM")
                    || upper.starts_with("UDR")
                    || upper.starts_with("UXG")
                    || upper.starts_with("UCG")
                    || upper.starts_with("UCK")
                {
                    DeviceType::Gateway
                } else {
                    DeviceType::Other
                }
            } else {
                DeviceType::Other
            }
        }
    }
}

fn map_device_state(code: i32) -> DeviceState {
    match code {
        0 => DeviceState::Offline,
        1 => DeviceState::Online,
        2 => DeviceState::PendingAdoption,
        4 => DeviceState::Updating,
        5 => DeviceState::GettingReady,
        _ => DeviceState::Unknown,
    }
}

fn parse_session_uplink(
    extra: &serde_json::Map<String, serde_json::Value>,
) -> (Option<MacAddress>, Option<u32>) {
    let Some(uplink) = extra.get("uplink").and_then(|v| v.as_object()) else {
        return (None, None);
    };
    let mac = uplink
        .get("uplink_mac")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(MacAddress::new);
    let port = uplink
        .get("uplink_remote_port")
        .and_then(serde_json::Value::as_u64)
        .and_then(|n| u32::try_from(n).ok());
    (mac, port)
}

impl From<SessionDevice> for Device {
    fn from(d: SessionDevice) -> Self {
        let device_type = infer_device_type(&d.device_type, d.model.as_ref());
        let state = map_device_state(d.state);
        let entity_id = if d.id.is_empty() {
            d.mac.clone()
        } else {
            d.id.clone()
        };
        let (uplink_device_mac, uplink_port_idx) = parse_session_uplink(&d.extra);

        let device_stats = {
            let mut s = DeviceStats {
                uptime_secs: d.uptime.and_then(|u| u.try_into().ok()),
                ..Default::default()
            };
            if let Some(ref sys) = d.sys_stats {
                s.load_average_1m = sys.load_1.as_deref().and_then(|v| v.parse().ok());
                s.load_average_5m = sys.load_5.as_deref().and_then(|v| v.parse().ok());
                s.load_average_15m = sys.load_15.as_deref().and_then(|v| v.parse().ok());
                s.cpu_utilization_pct = sys.cpu.as_deref().and_then(|v| v.parse().ok());
                s.memory_utilization_pct = match (sys.mem_used, sys.mem_total) {
                    (Some(used), Some(total)) if total > 0 =>
                    {
                        #[allow(clippy::as_conversions, clippy::cast_precision_loss)]
                        Some((used as f64 / total as f64) * 100.0)
                    }
                    _ => None,
                };
            }
            s
        };

        Device {
            id: EntityId::from(entity_id),
            mac: MacAddress::new(&d.mac),
            ip: parse_ip(d.ip.as_ref()),
            wan_ipv6: parse_legacy_wan_ipv6(&d.extra),
            name: d.name,
            model: d.model,
            device_type,
            state,
            firmware_version: d.version,
            firmware_updatable: d.upgradable.unwrap_or(false),
            adopted_at: None,
            provisioned_at: None,
            last_seen: epoch_to_datetime(d.last_seen),
            serial: d.serial,
            supported: true,
            ports: parse_session_ports(&d.extra),
            radios: parse_session_radios(&d.extra),
            uplink_device_id: None,
            uplink_device_mac,
            uplink_port_idx,
            has_switching: device_type == DeviceType::Switch || device_type == DeviceType::Gateway,
            has_access_point: device_type == DeviceType::AccessPoint,
            stats: device_stats,
            client_count: d.num_sta.and_then(|n| n.try_into().ok()),
            origin: None,
            source: DataSource::SessionApi,
            updated_at: Utc::now(),
        }
    }
}

// ── Integration API ──────────────────────────────────────────────

fn map_integration_device_state(state: &str) -> DeviceState {
    match state {
        "ONLINE" => DeviceState::Online,
        "OFFLINE" => DeviceState::Offline,
        "PENDING_ADOPTION" => DeviceState::PendingAdoption,
        "UPDATING" => DeviceState::Updating,
        "GETTING_READY" => DeviceState::GettingReady,
        "ADOPTING" => DeviceState::Adopting,
        "DELETING" => DeviceState::Deleting,
        "CONNECTION_INTERRUPTED" => DeviceState::ConnectionInterrupted,
        "ISOLATED" => DeviceState::Isolated,
        _ => DeviceState::Unknown,
    }
}

fn infer_device_type_integration(features: &[String], model: &str) -> DeviceType {
    let has = |f: &str| features.iter().any(|s| s == f);

    let upper = model.to_uppercase();
    let is_gateway_model = upper.starts_with("UGW")
        || upper.starts_with("UDM")
        || upper.starts_with("UDR")
        || upper.starts_with("UXG")
        || upper.starts_with("UCG")
        || upper.starts_with("UCK");

    if is_gateway_model || (has("switching") && has("routing")) || has("gateway") {
        DeviceType::Gateway
    } else if has("accessPoint") {
        DeviceType::AccessPoint
    } else if has("switching") {
        DeviceType::Switch
    } else {
        let model_owned = model.to_owned();
        infer_device_type("", Some(&model_owned))
    }
}

impl From<integration_types::DeviceResponse> for Device {
    fn from(d: integration_types::DeviceResponse) -> Self {
        let device_type = infer_device_type_integration(&d.features, &d.model);
        let state = map_integration_device_state(&d.state);

        Device {
            id: EntityId::Uuid(d.id),
            mac: MacAddress::new(&d.mac_address),
            ip: d.ip_address.as_deref().and_then(|s| s.parse().ok()),
            wan_ipv6: None,
            name: Some(d.name),
            model: Some(d.model),
            device_type,
            state,
            firmware_version: d.firmware_version,
            firmware_updatable: d.firmware_updatable,
            adopted_at: None,
            provisioned_at: None,
            last_seen: None,
            serial: None,
            supported: d.supported,
            ports: parse_integration_ports(&d.interfaces),
            radios: parse_integration_radios(&d.interfaces),
            uplink_device_id: None,
            uplink_device_mac: None,
            uplink_port_idx: None,
            has_switching: d.features.iter().any(|f| f == "switching"),
            has_access_point: d.features.iter().any(|f| f == "accessPoint"),
            stats: DeviceStats::default(),
            client_count: None,
            origin: None,
            source: DataSource::IntegrationApi,
            updated_at: Utc::now(),
        }
    }
}

pub(crate) fn device_stats_from_integration(
    resp: &integration_types::DeviceStatisticsResponse,
) -> DeviceStats {
    DeviceStats {
        uptime_secs: resp.uptime_sec.and_then(|u| u.try_into().ok()),
        cpu_utilization_pct: resp.cpu_utilization_pct,
        memory_utilization_pct: resp.memory_utilization_pct,
        load_average_1m: resp.load_average_1_min,
        load_average_5m: resp.load_average_5_min,
        load_average_15m: resp.load_average_15_min,
        last_heartbeat: resp.last_heartbeat_at.as_deref().and_then(parse_iso),
        next_heartbeat: resp.next_heartbeat_at.as_deref().and_then(parse_iso),
        uplink_bandwidth: resp.uplink.as_ref().and_then(|u| {
            let tx = u
                .get("txRateBps")
                .or_else(|| u.get("txBytesPerSecond"))
                .or_else(|| u.get("tx_bytes-r"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let rx = u
                .get("rxRateBps")
                .or_else(|| u.get("rxBytesPerSecond"))
                .or_else(|| u.get("rx_bytes-r"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            if tx == 0 && rx == 0 {
                None
            } else {
                Some(Bandwidth {
                    tx_bytes_per_sec: tx,
                    rx_bytes_per_sec: rx,
                })
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn device_type_from_legacy_type_field() {
        assert_eq!(infer_device_type("uap", None), DeviceType::AccessPoint);
        assert_eq!(infer_device_type("usw", None), DeviceType::Switch);
        assert_eq!(infer_device_type("ugw", None), DeviceType::Gateway);
        assert_eq!(infer_device_type("udm", None), DeviceType::Gateway);
    }

    #[test]
    fn device_type_from_model_fallback() {
        assert_eq!(
            infer_device_type("unknown", Some(&"UAP-AC-Pro".into())),
            DeviceType::AccessPoint
        );
        assert_eq!(
            infer_device_type("unknown", Some(&"U6-LR".into())),
            DeviceType::AccessPoint
        );
        assert_eq!(
            infer_device_type("unknown", Some(&"USW-24-PoE".into())),
            DeviceType::Switch
        );
        assert_eq!(
            infer_device_type("unknown", Some(&"UDM-Pro".into())),
            DeviceType::Gateway
        );
        assert_eq!(
            infer_device_type("unknown", Some(&"UCG-Max".into())),
            DeviceType::Gateway
        );
    }

    #[test]
    fn integration_device_type_gateway_by_model() {
        assert_eq!(
            infer_device_type_integration(&["switching".into()], "UCG-Max"),
            DeviceType::Gateway
        );
        assert_eq!(
            infer_device_type_integration(&["switching".into(), "routing".into()], "UDM-Pro"),
            DeviceType::Gateway
        );
    }

    #[test]
    fn device_state_mapping() {
        assert_eq!(map_device_state(0), DeviceState::Offline);
        assert_eq!(map_device_state(1), DeviceState::Online);
        assert_eq!(map_device_state(2), DeviceState::PendingAdoption);
        assert_eq!(map_device_state(4), DeviceState::Updating);
        assert_eq!(map_device_state(5), DeviceState::GettingReady);
        assert_eq!(map_device_state(99), DeviceState::Unknown);
    }

    #[test]
    fn legacy_device_falls_back_to_mac_when_id_missing() {
        let raw = json!({
            "mac": "dc:9f:db:00:00:01",
            "type": "ugw",
            "name": "USG 3P",
            "state": 2
        });

        let session_device: SessionDevice =
            serde_json::from_value(raw).expect("session device should deserialize");
        let device: Device = session_device.into();

        assert_eq!(device.id.to_string(), "dc:9f:db:00:00:01");
        assert_eq!(device.mac.to_string(), "dc:9f:db:00:00:01");
        assert_eq!(device.state, DeviceState::PendingAdoption);
    }

    #[test]
    fn session_device_carries_wired_uplink_mac_and_remote_port() {
        let raw = json!({
            "mac": "6c:63:f8:36:53:42",
            "type": "uap",
            "name": "U7-Pro-Wall",
            "state": 1,
            "uplink": {
                "type": "wire",
                "uplink_mac": "58:d6:1f:62:33:01",
                "uplink_remote_port": 4
            }
        });

        let session_device: SessionDevice = serde_json::from_value(raw).expect("deserialize");
        let device: Device = session_device.into();

        assert_eq!(
            device.uplink_device_mac.as_ref().map(ToString::to_string),
            Some("58:d6:1f:62:33:01".into())
        );
        assert_eq!(device.uplink_port_idx, Some(4));
    }

    #[test]
    fn session_device_with_no_uplink_block_has_none() {
        let raw = json!({
            "mac": "58:d6:1f:62:33:01",
            "type": "usw",
            "state": 1
        });

        let session_device: SessionDevice = serde_json::from_value(raw).expect("deserialize");
        let device: Device = session_device.into();

        assert!(device.uplink_device_mac.is_none());
        assert!(device.uplink_port_idx.is_none());
    }
}
