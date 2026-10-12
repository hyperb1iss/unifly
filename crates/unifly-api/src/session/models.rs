// Session API response types
//
// Models for the UniFi controller's session JSON API. All responses are wrapped
// in the `SessionResponse<T>` envelope. Fields use `#[serde(default)]` liberally
// because the API is inconsistent about field presence across firmware versions.

use serde::{Deserialize, Serialize};

// ── Response Envelope ────────────────────────────────────────────────

/// Standard UniFi session API response envelope.
///
/// Every session endpoint wraps its payload:
/// ```json
/// { "meta": { "rc": "ok", "msg": "optional" }, "data": [...] }
/// ```
#[derive(Debug, Deserialize)]
pub struct SessionResponse<T> {
    pub meta: Meta,
    pub data: Vec<T>,
}

/// Metadata from the session envelope. `rc` == `"ok"` means success.
#[derive(Debug, Deserialize)]
pub struct Meta {
    pub rc: String,
    #[serde(default)]
    pub msg: Option<String>,
}

// ── Device ───────────────────────────────────────────────────────────

/// Full device object from `stat/device`.
///
/// The session API can return 100+ fields per device. Only the fields unifly
/// reads are modelled; everything else is dropped at parse time so a refresh
/// does not materialise the whole payload. Callers that need the raw record
/// (e.g. `port_overrides` splicing) use `SessionClient::get_device_raw`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDevice {
    #[serde(default, rename = "_id")]
    pub id: String,
    pub mac: String,
    #[serde(rename = "type")]
    pub device_type: String,
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub adopted: bool,
    /// 0=offline, 1=online, 2=pending, 4=upgrading, 5=provisioning
    #[serde(default)]
    pub state: i32,
    #[serde(default)]
    pub sys_stats: Option<SysStats>,
    #[serde(default)]
    pub uptime: Option<i64>,
    #[serde(default)]
    pub num_sta: Option<i32>,
    #[serde(default)]
    pub serial: Option<String>,
    #[serde(default)]
    pub site_id: Option<String>,
    #[serde(default)]
    pub last_seen: Option<i64>,
    #[serde(default)]
    pub upgradable: Option<bool>,
    #[serde(default, rename = "user-num_sta")]
    pub user_num_sta: Option<i32>,
    #[serde(default, rename = "guest-num_sta")]
    pub guest_num_sta: Option<i32>,
    /// Uplink summary (`uplink.uplink_mac`, `uplink.uplink_remote_port`).
    #[serde(default)]
    pub uplink: Option<SessionUplink>,
    /// Primary WAN block; only the IPv6 list is retained.
    #[serde(default)]
    pub wan1: Option<SessionWan>,
    /// Device-level IPv6 address(es): a string or an array of strings.
    #[serde(default)]
    pub ipv6: Option<serde_json::Value>,
    /// Physical port table (switches and gateways).
    #[serde(default, deserialize_with = "null_as_empty")]
    pub port_table: Vec<SessionPortEntry>,
    /// Radio configuration rows (access points).
    #[serde(default, deserialize_with = "null_as_empty")]
    pub radio_table: Vec<SessionRadioEntry>,
    /// Per-radio runtime statistics, keyed by the `radio` band token.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub radio_table_stats: Vec<SessionRadioStats>,
}

/// Deserialize a list that the controller may send as `null` instead of
/// omitting it. `#[serde(default)]` alone only covers a missing key, and a
/// `null` table would otherwise fail the whole device record.
fn null_as_empty<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

/// Deserialize a string that UniFi sometimes sends as a bare number
/// (e.g. `"ht": 20` vs `"ht": "20"`). Non-scalar values become `None`.
fn lenient_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|v| match v {
        serde_json::Value::String(text) => Some(text),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }))
}

/// Deserialize a bool that may arrive as `true`/`false`, `0`/`1`, or a
/// `"true"`/`"false"` string. Anything else becomes `None`.
fn lenient_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|v| match v {
        serde_json::Value::Bool(b) => Some(b),
        serde_json::Value::Number(n) => n.as_i64().map(|i| i != 0),
        serde_json::Value::String(text) => match text.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }))
}

/// Deserialize a number that UniFi may send as a JSON number, a numeric
/// string, or a placeholder such as `"auto"`. Anything that is not a
/// non-negative integer becomes `None` instead of failing the whole record.
fn lenient_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value
        .as_ref()
        .and_then(lenient_u64_from_value)
        .and_then(|n| u32::try_from(n).ok()))
}

/// See [`lenient_u32`].
fn lenient_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.as_ref().and_then(lenient_u64_from_value))
}

/// See [`lenient_u32`]; accepts any finite number or numeric string.
fn lenient_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.as_ref().and_then(|v| match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }))
}

/// See [`lenient_u32`]; reads a `u64` from a number or numeric string.
fn lenient_u64_from_value(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::Number(n) => n.as_u64().or_else(|| {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::as_conversions
            )]
            n.as_f64()
                .filter(|f| *f >= 0.0 && f.fract() == 0.0)
                .map(|f| f as u64)
        }),
        serde_json::Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    }
}

/// Uplink summary nested inside `SessionDevice`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionUplink {
    /// Uplink medium: `"wire"` or `"wireless"`.
    #[serde(default, rename = "type", deserialize_with = "lenient_string")]
    pub uplink_type: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub uplink_mac: Option<String>,
    #[serde(default, deserialize_with = "lenient_u32")]
    pub uplink_remote_port: Option<u32>,
}

/// WAN block nested inside `SessionDevice`; only IPv6 is retained.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionWan {
    /// A string or an array of strings, possibly with a `/prefix` suffix.
    #[serde(default)]
    pub ipv6: Option<serde_json::Value>,
}

/// One row of a device's `port_table`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionPortEntry {
    #[serde(default, deserialize_with = "lenient_u32")]
    pub port_idx: Option<u32>,
    #[serde(default, deserialize_with = "lenient_bool")]
    pub up: Option<bool>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "lenient_u32")]
    pub speed: Option<u32>,
    /// `"GE"`, `"FE"`, `"SFP"`, `"SFP+"`.
    #[serde(default, deserialize_with = "lenient_string")]
    pub media: Option<String>,
    #[serde(default, deserialize_with = "lenient_bool")]
    pub port_poe: Option<bool>,
    #[serde(default, deserialize_with = "lenient_u64")]
    pub poe_caps: Option<u64>,
    #[serde(default, deserialize_with = "lenient_bool")]
    pub poe_enable: Option<bool>,
    #[serde(default, deserialize_with = "lenient_bool")]
    pub poe_good: Option<bool>,
}

/// One row of a device's `radio_table`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionRadioEntry {
    /// Band token: `"ng"`, `"na"`, `"6e"`.
    #[serde(default, deserialize_with = "lenient_string")]
    pub radio: Option<String>,
    #[serde(default, deserialize_with = "lenient_u32")]
    pub channel: Option<u32>,
    /// Channel width in MHz, sent as a string (`"20"`, `"40"`, `"80"`).
    #[serde(default, deserialize_with = "lenient_string")]
    pub ht: Option<String>,
}

/// One row of a device's `radio_table_stats`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionRadioStats {
    #[serde(default, deserialize_with = "lenient_string")]
    pub radio: Option<String>,
    #[serde(default, deserialize_with = "lenient_u32")]
    pub channel: Option<u32>,
    #[serde(default, deserialize_with = "lenient_f64")]
    pub cu_total: Option<f64>,
}

/// System statistics nested inside `SessionDevice`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SysStats {
    #[serde(default, rename = "loadavg_1")]
    pub load_1: Option<String>,
    #[serde(default, rename = "loadavg_5")]
    pub load_5: Option<String>,
    #[serde(default, rename = "loadavg_15")]
    pub load_15: Option<String>,
    #[serde(default)]
    pub mem_total: Option<i64>,
    #[serde(default)]
    pub mem_used: Option<i64>,
    #[serde(default)]
    pub cpu: Option<String>,
}

// ── Client (Station) ─────────────────────────────────────────────────

/// Connected client from `stat/sta`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionClientEntry {
    #[serde(rename = "_id")]
    pub id: String,
    pub mac: String,
    #[serde(default)]
    pub hostname: Option<String>,
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub oui: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub is_guest: Option<bool>,
    #[serde(default)]
    pub is_wired: Option<bool>,
    #[serde(default)]
    pub authorized: Option<bool>,
    #[serde(default)]
    pub blocked: Option<bool>,
    #[serde(default)]
    pub signal: Option<i32>,
    #[serde(default)]
    pub tx_bytes: Option<i64>,
    #[serde(default)]
    pub rx_bytes: Option<i64>,
    #[serde(default)]
    pub tx_rate: Option<i64>,
    #[serde(default)]
    pub rx_rate: Option<i64>,
    #[serde(default)]
    pub uptime: Option<i64>,
    #[serde(default)]
    pub first_seen: Option<i64>,
    #[serde(default)]
    pub last_seen: Option<i64>,
    #[serde(default)]
    pub site_id: Option<String>,
    #[serde(default)]
    pub essid: Option<String>,
    #[serde(default)]
    pub bssid: Option<String>,
    #[serde(default)]
    pub channel: Option<i32>,
    #[serde(default)]
    pub radio: Option<String>,
    #[serde(default)]
    pub rssi: Option<i32>,
    #[serde(default)]
    pub noise: Option<i32>,
    #[serde(default)]
    pub satisfaction: Option<i32>,
    #[serde(default)]
    pub ap_mac: Option<String>,
    #[serde(default)]
    pub network: Option<String>,
    #[serde(default)]
    pub network_id: Option<String>,
    #[serde(default)]
    pub sw_mac: Option<String>,
    #[serde(default)]
    pub sw_port: Option<i32>,
}

// ── User (known client / DHCP reservation) ──────────────────────────

/// User object from `rest/user`.
///
/// The "user" collection stores persistent client configuration such as
/// names, notes, and DHCP reservations. Unlike `stat/sta` (currently
/// connected stations), `rest/user` includes offline/historical clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionUserEntry {
    #[serde(rename = "_id")]
    pub id: String,
    pub mac: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub hostname: Option<String>,
    #[serde(default)]
    pub use_fixedip: Option<bool>,
    #[serde(default)]
    pub fixed_ip: Option<String>,
    #[serde(default)]
    pub network_id: Option<String>,
    #[serde(default)]
    pub site_id: Option<String>,
    #[serde(default)]
    pub noted: Option<bool>,
    #[serde(default)]
    pub note: Option<String>,
}

// ── Site ─────────────────────────────────────────────────────────────

/// Site object from `/api/self/sites`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSite {
    #[serde(rename = "_id")]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    /// Catch-all for undocumented fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── Event ────────────────────────────────────────────────────────────

/// Event object from `stat/event`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub msg: Option<String>,
    #[serde(default)]
    pub datetime: Option<String>,
    #[serde(default)]
    pub subsystem: Option<String>,
    #[serde(default)]
    pub site_id: Option<String>,
    /// Catch-all for undocumented fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── Alarm ────────────────────────────────────────────────────────────

/// Alarm object from `stat/alarm`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionAlarm {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub msg: Option<String>,
    #[serde(default)]
    pub datetime: Option<String>,
    #[serde(default)]
    pub archived: Option<bool>,
    /// Catch-all for undocumented fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── Wi-Fi Observability ─────────────────────────────────────────────

/// Neighboring / rogue access point from `stat/rogueap`.
///
/// Each entry represents a foreign AP detected by one of your APs.
/// Note: `stat/rogueap` uses Unix epoch **seconds** for query params,
/// unlike many other UniFi stats endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RogueAp {
    pub bssid: String,
    #[serde(default)]
    pub essid: Option<String>,
    #[serde(default)]
    pub channel: Option<i32>,
    #[serde(default)]
    pub freq: Option<i32>,
    #[serde(default)]
    pub signal: Option<i32>,
    #[serde(default)]
    pub rssi: Option<i32>,
    #[serde(default)]
    pub noise: Option<i32>,
    #[serde(default)]
    pub security: Option<String>,
    #[serde(default)]
    pub radio: Option<String>,
    #[serde(default)]
    pub age: Option<i64>,
    #[serde(default)]
    pub is_rogue: bool,
    /// MAC of your AP that observed this neighbor.
    #[serde(default)]
    pub ap_mac: Option<String>,
    /// Catch-all for undocumented fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Country-level regulatory channel data from `stat/current-channel`.
///
/// The UniFi API returns one record per country with per-band channel lists
/// (e.g. `channels_ng`, `channels_na`, `channels_6e`) rather than per-radio
/// rows. The typed fields cover the most common bands; the `extra` map
/// captures width-specific and AFC lists.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelAvailability {
    /// ISO 3166-1 numeric country code (e.g. `"840"` for the US).
    #[serde(default)]
    pub code: Option<String>,
    /// Two-letter country key (e.g. `"US"`).
    #[serde(default)]
    pub key: Option<String>,
    /// Human-readable country name.
    #[serde(default)]
    pub name: Option<String>,
    /// 2.4 GHz channels.
    #[serde(default)]
    pub channels_ng: Option<Vec<i32>>,
    /// 5 GHz channels.
    #[serde(default)]
    pub channels_na: Option<Vec<i32>>,
    /// 5 GHz DFS channels.
    #[serde(default)]
    pub channels_na_dfs: Option<Vec<i32>>,
    /// 6 GHz channels.
    #[serde(default)]
    pub channels_6e: Option<Vec<i32>>,
    /// Catch-all for width-specific lists, AFC data, etc.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_device_treats_null_tables_as_empty() {
        let json = serde_json::json!({
            "_id": "abc",
            "mac": "aa:bb:cc:dd:ee:ff",
            "type": "usw",
            "port_table": null,
            "radio_table": null,
            "radio_table_stats": null
        });
        let device: SessionDevice = serde_json::from_value(json).expect("null tables decode");
        assert_eq!(device.port_table.len(), 0);
        assert_eq!(device.radio_table.len(), 0);
        assert_eq!(device.radio_table_stats.len(), 0);
    }

    #[test]
    fn session_device_tolerates_auto_channel_and_string_numbers() {
        let json = serde_json::json!({
            "_id": "abc",
            "mac": "aa:bb:cc:dd:ee:ff",
            "type": "uap",
            "uplink": {"uplink_mac": "11:22:33:44:55:66", "uplink_remote_port": "7"},
            "port_table": [{"port_idx": 1, "speed": "auto", "poe_caps": 3.0, "up": 1, "media": "GE"}],
            "radio_table": [{"radio": "ng", "channel": "auto", "ht": 20}],
            "radio_table_stats": [{"radio": "ng", "channel": 6, "cu_total": "71"}]
        });
        let device: SessionDevice = serde_json::from_value(json).expect("lenient decode");
        assert_eq!(
            device.uplink.as_ref().and_then(|u| u.uplink_remote_port),
            Some(7)
        );
        assert_eq!(device.port_table[0].port_idx, Some(1));
        assert_eq!(device.port_table[0].speed, None);
        assert_eq!(device.port_table[0].poe_caps, Some(3));
        assert_eq!(device.radio_table[0].channel, None);
        assert_eq!(device.radio_table[0].ht.as_deref(), Some("20"));
        assert_eq!(device.port_table[0].up, Some(true));
        assert_eq!(device.port_table[0].media.as_deref(), Some("GE"));
        assert_eq!(device.radio_table_stats[0].channel, Some(6));
        assert_eq!(device.radio_table_stats[0].cu_total, Some(71.0));
    }
}
