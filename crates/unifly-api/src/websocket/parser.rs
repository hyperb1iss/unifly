use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{debug, warn};

/// A parsed event from the UniFi WebSocket stream.
///
/// Uses `#[serde(flatten)]` to capture all fields beyond the core set,
/// so nothing from the controller is silently dropped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiEvent {
    /// Event key, e.g. `"EVT_WU_Connected"`, `"EVT_SW_Disconnected"`.
    pub key: String,

    /// Subsystem that emitted the event: `"wlan"`, `"lan"`, `"sta"`, `"gw"`, etc.
    pub subsystem: String,

    /// Site ID this event belongs to.
    pub site_id: String,

    /// Human-readable event message, if present.
    /// The controller sends `"msg"` in most payloads; `"message"` is a rarer variant.
    #[serde(default, alias = "msg")]
    pub message: Option<String>,

    /// ISO-8601 timestamp from the controller.
    #[serde(default)]
    pub datetime: Option<String>,

    /// All remaining fields the controller sends.
    ///
    /// Populated for `EVT_*` events, where the message template needs
    /// arbitrary keys. For `device:sync` / `device:update` frames this is
    /// `Value::Null` and the stats live in [`device_sync`](Self::device_sync)
    /// instead, so the full device record is never retained.
    #[serde(flatten)]
    pub extra: serde_json::Value,

    /// Typed live stats extracted from `device:sync` / `device:update`.
    #[serde(skip)]
    pub device_sync: Option<Box<DeviceSync>>,
}

/// The subset of a `device:sync` frame that feeds live device stats.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DeviceSync {
    #[serde(default)]
    pub mac: Option<String>,
    #[serde(default)]
    pub sys_stats: Option<DeviceSyncSysStats>,
    #[serde(default)]
    pub uplink: Option<DeviceSyncUplink>,
    #[serde(default, rename = "tx_bytes-r")]
    pub tx_bytes_r: Option<NumOrStr>,
    #[serde(default, rename = "rx_bytes-r")]
    pub rx_bytes_r: Option<NumOrStr>,
    #[serde(default, rename = "_uptime")]
    pub underscore_uptime: Option<NumOrStr>,
    #[serde(default)]
    pub uptime: Option<NumOrStr>,
    #[serde(default)]
    pub num_sta: Option<NumOrStr>,
    #[serde(default)]
    pub wan1: Option<DeviceSyncWan>,
    #[serde(default)]
    pub ipv6: Option<serde_json::Value>,
}

/// `sys_stats` inside a `device:sync` frame. Numbers arrive as either JSON
/// numbers or strings depending on firmware.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DeviceSyncSysStats {
    #[serde(default)]
    pub cpu: Option<NumOrStr>,
    #[serde(default)]
    pub mem_used: Option<NumOrStr>,
    #[serde(default)]
    pub mem_total: Option<NumOrStr>,
    #[serde(default)]
    pub loadavg_1: Option<NumOrStr>,
    #[serde(default)]
    pub loadavg_5: Option<NumOrStr>,
    #[serde(default)]
    pub loadavg_15: Option<NumOrStr>,
}

/// `uplink` inside a `device:sync` frame; only the byte rates are retained.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DeviceSyncUplink {
    #[serde(default, rename = "tx_bytes-r", alias = "tx_bytes_r")]
    pub tx_bytes_r: Option<NumOrStr>,
    #[serde(default, rename = "rx_bytes-r", alias = "rx_bytes_r")]
    pub rx_bytes_r: Option<NumOrStr>,
}

/// `wan1` inside a `device:sync` frame; only IPv6 is retained.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DeviceSyncWan {
    #[serde(default)]
    pub ipv6: Option<serde_json::Value>,
}

/// Overwrite each field of `target` that `newer` carries, keeping the rest.
fn take_newer<T: Clone>(target: &mut Option<T>, newer: Option<&T>) {
    if let Some(value) = newer {
        *target = Some(value.clone());
    }
}

impl DeviceSync {
    /// Fold a later frame for the same device into this one. Frames can be
    /// partial, so a field the newer frame omits keeps its earlier value
    /// instead of being lost when several frames coalesce before a flush.
    pub fn merge_newer(&mut self, newer: &Self) {
        take_newer(&mut self.mac, newer.mac.as_ref());
        match (&mut self.sys_stats, &newer.sys_stats) {
            (Some(sys), Some(next)) => sys.merge_newer(next),
            (slot, next) => take_newer(slot, next.as_ref()),
        }
        match (&mut self.uplink, &newer.uplink) {
            (Some(uplink), Some(next)) => {
                take_newer(&mut uplink.tx_bytes_r, next.tx_bytes_r.as_ref());
                take_newer(&mut uplink.rx_bytes_r, next.rx_bytes_r.as_ref());
            }
            (slot, next) => take_newer(slot, next.as_ref()),
        }
        take_newer(&mut self.tx_bytes_r, newer.tx_bytes_r.as_ref());
        take_newer(&mut self.rx_bytes_r, newer.rx_bytes_r.as_ref());
        take_newer(
            &mut self.underscore_uptime,
            newer.underscore_uptime.as_ref(),
        );
        take_newer(&mut self.uptime, newer.uptime.as_ref());
        take_newer(&mut self.num_sta, newer.num_sta.as_ref());
        match (&mut self.wan1, &newer.wan1) {
            (Some(wan), Some(next)) => take_newer(&mut wan.ipv6, next.ipv6.as_ref()),
            (slot, next) => take_newer(slot, next.as_ref()),
        }
        take_newer(&mut self.ipv6, newer.ipv6.as_ref());
    }
}

impl DeviceSyncSysStats {
    fn merge_newer(&mut self, newer: &Self) {
        take_newer(&mut self.cpu, newer.cpu.as_ref());
        take_newer(&mut self.mem_used, newer.mem_used.as_ref());
        take_newer(&mut self.mem_total, newer.mem_total.as_ref());
        take_newer(&mut self.loadavg_1, newer.loadavg_1.as_ref());
        take_newer(&mut self.loadavg_5, newer.loadavg_5.as_ref());
        take_newer(&mut self.loadavg_15, newer.loadavg_15.as_ref());
    }
}

/// A JSON number that some firmware versions encode as a string.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum NumOrStr {
    Num(f64),
    Str(String),
}

impl NumOrStr {
    /// Numeric value, or `None` for an empty / unparseable string.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Num(n) => Some(*n),
            Self::Str(s) => s.trim().parse().ok(),
        }
    }

    /// Non-negative whole number, or `None` (negative, fractional, or text).
    pub fn as_u64(&self) -> Option<u64> {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::as_conversions
        )]
        self.as_f64()
            .filter(|f| f.is_finite() && *f >= 0.0 && f.fract() == 0.0)
            .map(|f| f as u64)
    }

    /// Whole number, or `None` (fractional or text).
    pub fn as_i64(&self) -> Option<i64> {
        #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
        self.as_f64()
            .filter(|f| f.is_finite() && f.fract() == 0.0)
            .map(|f| f as i64)
    }
}

#[derive(Debug, Deserialize)]
struct WsEnvelope {
    #[allow(dead_code)]
    meta: WsMeta,
    data: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct WsMeta {
    #[allow(dead_code)]
    rc: String,
    #[serde(default)]
    message: Option<String>,
}

pub(in crate::websocket) fn parse_and_broadcast(
    text: &str,
    event_tx: &broadcast::Sender<Arc<UnifiEvent>>,
) {
    let envelope: WsEnvelope = match serde_json::from_str(text) {
        Ok(envelope) => envelope,
        Err(error) => {
            tracing::debug!(error = %error, "Failed to parse WebSocket envelope");
            return;
        }
    };

    let msg_type = envelope.meta.message.as_deref().unwrap_or("");

    for data in envelope.data {
        let event = match msg_type {
            "events" => match UnifiEvent::deserialize(&data) {
                Ok(event) => event,
                Err(error) => {
                    tracing::debug!(
                        error = %error,
                        msg_type,
                        "Could not deserialize event, constructing from raw data"
                    );
                    event_from_raw(msg_type, data)
                }
            },
            _ => event_from_raw(msg_type, data),
        };

        let _ = event_tx.send(Arc::new(event));
    }
}

/// Build an event from an untyped frame.
///
/// `device:sync` / `device:update` frames carry the whole device record
/// (tens of KB each, several per second). Only the live-stats subset is
/// kept for those; every other frame keeps its full payload in `extra`.
fn event_from_raw(msg_type: &str, data: serde_json::Value) -> UnifiEvent {
    let key = data["key"].as_str().unwrap_or(msg_type).to_string();
    let subsystem = data["subsystem"].as_str().unwrap_or("unknown").to_string();
    let site_id = data["site_id"].as_str().unwrap_or("").to_string();
    let message = data["msg"]
        .as_str()
        .or_else(|| data["message"].as_str())
        .map(String::from);
    let datetime = data["datetime"].as_str().map(String::from);

    let (extra, device_sync) = if is_device_sync(&key) {
        let sync = match DeviceSync::deserialize(&data) {
            Ok(sync) => Some(Box::new(sync)),
            Err(error) => {
                // Loud once, quiet afterwards: this fires per frame and the
                // shape is the same for every frame from a given firmware.
                if SYNC_DECODE_WARNED.swap(true, std::sync::atomic::Ordering::AcqRel) {
                    debug!(%key, %error, "device sync frame did not decode");
                } else {
                    warn!(%key, %error, "device sync frame did not decode; live device stats disabled until this is fixed");
                }
                None
            }
        };
        (serde_json::Value::Null, sync)
    } else {
        (data, None)
    };

    UnifiEvent {
        key,
        subsystem,
        site_id,
        message,
        datetime,
        extra,
        device_sync,
    }
}

/// Set once the first undecodable device-sync frame has been reported, so a
/// broken payload warns loudly one time instead of on every frame.
static SYNC_DECODE_WARNED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Whether a WebSocket key is a live device-stats frame.
pub fn is_device_sync(key: &str) -> bool {
    key == "device:sync" || key == "device:update"
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_event_from_raw_json() {
        let data = serde_json::json!({
            "key": "EVT_WU_Connected",
            "subsystem": "wlan",
            "site_id": "abc123",
            "msg": "User[aa:bb:cc:dd:ee:ff] connected",
            "datetime": "2026-02-10T12:00:00Z",
            "user": "aa:bb:cc:dd:ee:ff",
            "ssid": "MyNetwork"
        });

        let event = event_from_raw("events", data);
        assert_eq!(event.key, "EVT_WU_Connected");
        assert_eq!(event.subsystem, "wlan");
        assert_eq!(event.site_id, "abc123");
        assert_eq!(
            event.message.as_deref(),
            Some("User[aa:bb:cc:dd:ee:ff] connected")
        );
        assert_eq!(event.datetime.as_deref(), Some("2026-02-10T12:00:00Z"));
    }

    #[test]
    fn parse_sync_event_from_raw_json() {
        let data = serde_json::json!({
            "mac": "aa:bb:cc:dd:ee:ff",
            "state": 1,
            "site_id": "site1"
        });

        let event = event_from_raw("device:sync", data);
        assert_eq!(event.key, "device:sync");
        assert_eq!(event.subsystem, "unknown");
        assert_eq!(event.site_id, "site1");
        assert!(
            event.extra.is_null(),
            "sync frames must not retain the payload"
        );
        let sync = event.device_sync.expect("typed sync stats");
        assert_eq!(sync.mac.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
    }

    #[test]
    fn device_sync_merge_keeps_fields_a_partial_frame_omits() {
        let first = event_from_raw(
            "device:sync",
            serde_json::json!({
                "mac": "aa:bb:cc:dd:ee:ff",
                "sys_stats": { "cpu": 40, "mem_used": 10, "mem_total": 100 },
                "num_sta": 3
            }),
        );
        let second = event_from_raw(
            "device:sync",
            serde_json::json!({
                "mac": "aa:bb:cc:dd:ee:ff",
                "sys_stats": { "mem_used": 20 },
                "num_sta": 5
            }),
        );
        let mut merged = *first.device_sync.expect("first frame decodes");
        merged.merge_newer(&second.device_sync.expect("second frame decodes"));

        let sys = merged.sys_stats.expect("sys_stats survives the merge");
        assert_eq!(sys.cpu.and_then(|v| v.as_f64()), Some(40.0));
        assert_eq!(sys.mem_used.and_then(|v| v.as_f64()), Some(20.0));
        assert_eq!(sys.mem_total.and_then(|v| v.as_f64()), Some(100.0));
        assert_eq!(merged.num_sta.as_ref().and_then(NumOrStr::as_u64), Some(5));
    }

    #[test]
    fn device_sync_extracts_stats_and_tolerates_string_numbers() {
        let data = serde_json::json!({
            "mac": "aa:bb:cc:dd:ee:ff",
            "sys_stats": { "cpu": "12.5", "mem_used": 50.0, "mem_total": "200", "loadavg_1": 0.5 },
            "uplink": { "tx_bytes-r": 100, "rx_bytes-r": 200 },
            "_uptime": 42,
            "num_sta": 3,
            "some_huge_field": [1, 2, 3]
        });
        let event = event_from_raw("device:sync", data);
        let sync = event.device_sync.expect("typed sync stats");
        let sys = sync.sys_stats.expect("sys_stats");
        assert_eq!(sys.cpu.and_then(|v| v.as_f64()), Some(12.5));
        assert_eq!(sys.loadavg_1.and_then(|v| v.as_f64()), Some(0.5));
        assert_eq!(
            sync.uplink
                .as_ref()
                .and_then(|u| u.tx_bytes_r.as_ref())
                .and_then(NumOrStr::as_u64),
            Some(100)
        );
        assert_eq!(
            sync.underscore_uptime.as_ref().and_then(NumOrStr::as_i64),
            Some(42)
        );
        assert_eq!(sync.num_sta.as_ref().and_then(NumOrStr::as_u64), Some(3));
    }

    #[test]
    fn deserialize_unifi_event() {
        let json = r#"{
            "key": "EVT_SW_Disconnected",
            "subsystem": "lan",
            "site_id": "default",
            "message": "Switch lost contact",
            "datetime": "2026-02-10T13:00:00Z",
            "sw": "aa:bb:cc:dd:ee:ff",
            "port": 4
        }"#;

        let event: UnifiEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.key, "EVT_SW_Disconnected");
        assert_eq!(event.subsystem, "lan");
        assert_eq!(event.site_id, "default");
        assert_eq!(event.message.as_deref(), Some("Switch lost contact"));
        assert_eq!(event.extra["sw"], "aa:bb:cc:dd:ee:ff");
        assert_eq!(event.extra["port"], 4);
    }

    #[test]
    fn deserialize_unifi_event_msg_alias() {
        let json = r#"{
            "key": "EVT_WU_Connected",
            "subsystem": "wlan",
            "site_id": "abc123",
            "msg": "User[aa:bb:cc:dd:ee:ff] connected",
            "datetime": "2026-02-10T12:00:00Z"
        }"#;

        let event: UnifiEvent = serde_json::from_str(json).unwrap();
        assert_eq!(
            event.message.as_deref(),
            Some("User[aa:bb:cc:dd:ee:ff] connected")
        );
    }

    #[test]
    fn parse_and_broadcast_events_message() {
        let (tx, mut rx) = broadcast::channel(16);

        let raw = serde_json::json!({
            "meta": { "rc": "ok", "message": "events" },
            "data": [{
                "key": "EVT_WU_Connected",
                "subsystem": "wlan",
                "site_id": "default",
                "msg": "Client connected",
                "user": "aa:bb:cc:dd:ee:ff"
            }]
        });

        parse_and_broadcast(&raw.to_string(), &tx);

        let event = rx.try_recv().unwrap();
        assert_eq!(event.key, "EVT_WU_Connected");
        assert_eq!(event.subsystem, "wlan");
    }

    #[test]
    fn parse_and_broadcast_sync_message() {
        let (tx, mut rx) = broadcast::channel(16);

        let raw = serde_json::json!({
            "meta": { "rc": "ok", "message": "device:sync" },
            "data": [{
                "mac": "aa:bb:cc:dd:ee:ff",
                "state": 1,
                "site_id": "site1"
            }]
        });

        parse_and_broadcast(&raw.to_string(), &tx);

        let event = rx.try_recv().unwrap();
        assert_eq!(event.key, "device:sync");
        assert_eq!(event.site_id, "site1");
    }

    #[test]
    fn parse_and_broadcast_malformed_json() {
        let (tx, mut rx) = broadcast::channel::<Arc<UnifiEvent>>(16);

        parse_and_broadcast("not json at all", &tx);

        assert!(rx.try_recv().is_err());
    }
}
