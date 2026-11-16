use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::model::address::Ipv6Scope;
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

/// The IPv6 addresses in each of `values`, where each is one address or an
/// array of them, possibly with a prefix length. Each address appears once,
/// widest reach first and otherwise in the order reported.
pub(crate) fn parse_ipv6_list<'a>(values: impl IntoIterator<Item = &'a Value>) -> Vec<Ipv6Addr> {
    let mut found: Vec<Ipv6Addr> = Vec::new();
    for value in values {
        let items = match value {
            Value::Array(items) => items.as_slice(),
            single => std::slice::from_ref(single),
        };
        for text in items.iter().filter_map(Value::as_str) {
            let address = text.split('/').next().unwrap_or(text).trim();
            if let Ok(address) = address.parse::<Ipv6Addr>()
                && !address.is_unspecified()
                && !found.contains(&address)
            {
                found.push(address);
            }
        }
    }
    found.sort_by_key(Ipv6Scope::of);
    found
}

/// The gateway's public IPv6 address on its primary WAN, from `wan1.ipv6`.
/// A WAN holding only a link-local address has none.
pub(crate) fn parse_wan_ipv6(wan1_ipv6: Option<&Value>) -> Option<String> {
    parse_ipv6_list(wan1_ipv6)
        .into_iter()
        .find(|address| Ipv6Scope::of(address) != Ipv6Scope::LinkLocal)
        .map(|address| address.to_string())
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
        "DERIVED" => Some(EntityOrigin::Derived),
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

#[cfg(test)]
mod ipv6_tests {
    use serde_json::json;

    use super::{parse_ipv6_list, parse_wan_ipv6};

    fn texts(value: &serde_json::Value) -> Vec<String> {
        parse_ipv6_list(Some(value))
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn a_list_puts_the_widest_reach_first_and_names_each_address_once() {
        let reported = json!([
            "fe80::1c2d:3eff:fe4f:5a6b",
            "fd00:c0de::9",
            "2001:db8:3fa::9/64",
            "2001:db8:3fa::a",
            "2001:DB8:3FA::9",
            "::",
            "not an address",
        ]);
        assert_eq!(
            texts(&reported),
            [
                "2001:db8:3fa::9",
                "2001:db8:3fa::a",
                "fd00:c0de::9",
                "fe80::1c2d:3eff:fe4f:5a6b"
            ]
        );
        assert_eq!(texts(&json!("2001:db8::1")), ["2001:db8::1"]);
        assert_eq!(texts(&json!(42)), Vec::<String>::new());
    }

    #[test]
    fn the_wan_address_is_never_a_link_local_one() {
        let wan = json!(["fe80::1", "2001:db8:4e48::1"]);
        assert_eq!(
            parse_wan_ipv6(Some(&wan)).as_deref(),
            Some("2001:db8:4e48::1")
        );
        assert_eq!(parse_wan_ipv6(Some(&json!(["fe80::1"]))), None);
        assert_eq!(parse_wan_ipv6(None), None);
    }
}
