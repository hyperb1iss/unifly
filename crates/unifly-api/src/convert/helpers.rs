use std::collections::HashMap;
use std::net::IpAddr;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::model::common::EntityOrigin;

pub(crate) fn parse_ip(raw: Option<&String>) -> Option<IpAddr> {
    raw.and_then(|s| s.parse().ok())
}

pub(crate) fn epoch_to_datetime(epoch: Option<i64>) -> Option<DateTime<Utc>> {
    epoch.and_then(|ts| DateTime::from_timestamp(ts, 0))
}

pub(crate) fn parse_datetime(raw: Option<&String>) -> Option<DateTime<Utc>> {
    raw.and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
}

pub(crate) fn parse_iso(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn parse_ipv6_text(raw: &str) -> Option<std::net::Ipv6Addr> {
    let candidate = raw.trim().split('/').next().unwrap_or(raw).trim();
    candidate.parse::<std::net::Ipv6Addr>().ok()
}

fn pick_ipv6_from_value(value: &Value) -> Option<String> {
    let mut first_link_local: Option<String> = None;

    let iter: Box<dyn Iterator<Item = &Value> + '_> = match value {
        Value::Array(items) => Box::new(items.iter()),
        _ => Box::new(std::iter::once(value)),
    };

    for item in iter {
        if let Some(ipv6) = item.as_str().and_then(parse_ipv6_text) {
            let ip_text = ipv6.to_string();
            if !ipv6.is_unicast_link_local() {
                return Some(ip_text);
            }
            if first_link_local.is_none() {
                first_link_local = Some(ip_text);
            }
        }
    }

    first_link_local
}

/// Pick the device's public IPv6 from `wan1.ipv6` first, then the top-level
/// `ipv6` field. Either may be a string or an array of strings.
pub(crate) fn parse_legacy_wan_ipv6(
    wan1_ipv6: Option<&Value>,
    ipv6: Option<&Value>,
) -> Option<String> {
    if let Some(v) = wan1_ipv6.and_then(pick_ipv6_from_value) {
        return Some(v);
    }
    ipv6.and_then(pick_ipv6_from_value)
}

pub(crate) fn extra_bool(extra: &HashMap<String, Value>, key: &str) -> bool {
    extra.get(key).and_then(Value::as_bool).unwrap_or(false)
}

pub(crate) fn extra_frequencies(extra: &HashMap<String, Value>, key: &str) -> Vec<f32> {
    extra
        .get(key)
        .and_then(Value::as_array)
        .map(|values| {
            #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
            values
                .iter()
                .filter_map(Value::as_f64)
                .map(|frequency| frequency as f32)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn map_origin(management: &str) -> Option<EntityOrigin> {
    match management {
        "USER_DEFINED" => Some(EntityOrigin::UserDefined),
        "SYSTEM_DEFINED" => Some(EntityOrigin::SystemDefined),
        "ORCHESTRATED" => Some(EntityOrigin::Orchestrated),
        _ => None,
    }
}

pub(crate) fn origin_from_metadata(metadata: &serde_json::Value) -> Option<EntityOrigin> {
    metadata
        .get("origin")
        .or_else(|| metadata.get("management"))
        .and_then(|v| v.as_str())
        .and_then(map_origin)
}

pub(crate) fn resolve_event_templates(msg: &str, extra: &serde_json::Value) -> String {
    if !msg.contains('{') {
        return msg.to_string();
    }

    let mut result = msg.to_string();
    while let Some(start) = result.find('{') {
        let Some(end) = result[start..].find('}') else {
            break;
        };
        let key = &result[start + 1..start + end];
        let replacement = extra
            .get(key)
            .and_then(|v| match v {
                serde_json::Value::String(s) => Some(s.as_str()),
                _ => None,
            })
            .unwrap_or(key);
        result = format!(
            "{}{replacement}{}",
            &result[..start],
            &result[start + end + 1..]
        );
    }
    result
}
