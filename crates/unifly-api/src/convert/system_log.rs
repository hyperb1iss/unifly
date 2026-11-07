use std::borrow::Cow;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::Error;
use crate::model::common::DataSource;
use crate::model::{Alarm, EntityId, Event, EventCategory, EventSeverity, MacAddress};
use crate::session::models::SessionEvent;
use crate::session::system_log::SystemLogEntry;

use super::helpers::parse_datetime;

impl TryFrom<SystemLogEntry> for Event {
    type Error = Error;

    fn try_from(entry: SystemLogEntry) -> Result<Self, Self::Error> {
        let timestamp = DateTime::from_timestamp_millis(entry.timestamp).ok_or_else(|| {
            invalid_history("system-log timestamp is outside the supported range")
        })?;
        let id = match entry.id.as_ref().filter(|id| !id.is_empty()) {
            Some(id) => EntityId::from(id.clone()),
            None => EntityId::from(format!("syslog:{}", content_identity(&entry)?)),
        };
        let message = entry
            .message
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                let template = entry
                    .message_raw
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .or(entry.title_raw.as_deref())
                    .unwrap_or_default();
                render_message(template, &entry.parameters)
            });
        Ok(Self {
            id: Some(id),
            timestamp,
            category: category(entry.category.as_deref()),
            severity: severity(entry.severity.as_deref()),
            event_type: entry
                .event
                .or_else(|| entry.key.clone())
                .unwrap_or_default(),
            message,
            device_mac: actor_mac(&entry.parameters, &["DEVICE", "DEVICE_FROM", "DEVICE_TO"]),
            client_mac: actor_mac(&entry.parameters, &["CLIENT", "SRC_CLIENT"]),
            site_id: None,
            ssid: entry
                .parameters
                .get("WLAN")
                .and_then(parameter_text)
                .filter(|value| !value.trim().is_empty())
                .map(Cow::into_owned),
            raw_key: entry.key,
            source: DataSource::SessionApi,
        })
    }
}

impl TryFrom<SystemLogEntry> for Alarm {
    type Error = Error;

    fn try_from(entry: SystemLogEntry) -> Result<Self, Self::Error> {
        let id = entry
            .id
            .as_ref()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| invalid_history("critical alert has no ID"))?
            .clone();
        let archived = entry.status.as_deref() == Some("READ");
        let event = Event::try_from(entry)?;
        Ok(Self {
            id: EntityId::from(id),
            timestamp: event.timestamp,
            category: event.category,
            severity: event.severity,
            message: event.message,
            archived,
            device_mac: event.device_mac,
            site_id: event.site_id,
        })
    }
}

pub(crate) fn legacy_history_event(entry: SessionEvent) -> Result<Event, Error> {
    let timestamp = parse_datetime(entry.datetime.as_ref())
        .or_else(|| {
            entry
                .extra
                .get("time")
                .and_then(Value::as_i64)
                .and_then(DateTime::<Utc>::from_timestamp_millis)
        })
        .ok_or_else(|| invalid_history("legacy event has no valid timestamp"))?;
    let category = match entry.subsystem.as_deref() {
        Some("wlan" | "lan" | "wan") => EventCategory::Network,
        Some("device") => EventCategory::Device,
        Some("client") => EventCategory::Client,
        Some("system") => EventCategory::System,
        Some("admin") => EventCategory::Admin,
        Some("firewall") => EventCategory::Firewall,
        Some("vpn") => EventCategory::Vpn,
        _ => EventCategory::Unknown,
    };
    let ssid = super::event::event_ssid(&entry.extra);
    Ok(Event {
        id: (!entry.id.is_empty()).then(|| EntityId::from(entry.id)),
        timestamp,
        category,
        severity: EventSeverity::Info,
        event_type: entry.key.clone().unwrap_or_default(),
        message: render_message(entry.msg.as_deref().unwrap_or_default(), &entry.extra),
        device_mac: actor_mac(&entry.extra, &["mac", "sw", "ap"]),
        client_mac: actor_mac(&entry.extra, &["user", "sta"]),
        site_id: entry.site_id.map(EntityId::from),
        ssid,
        raw_key: entry.key,
        source: DataSource::SessionApi,
    })
}

fn category(value: Option<&str>) -> EventCategory {
    match value {
        Some("CLIENT_DEVICES") => EventCategory::Client,
        Some("UNIFI_DEVICES" | "UNIFI_ETHERNET_PORTS" | "POWER") => EventCategory::Device,
        Some("INTERNET_AND_WAN") => EventCategory::Network,
        Some("SOFTWARE_UPDATES") => EventCategory::System,
        Some("AUDIT") => EventCategory::Admin,
        Some("SECURITY") => EventCategory::Firewall,
        Some("VPN") => EventCategory::Vpn,
        _ => EventCategory::Unknown,
    }
}

fn severity(value: Option<&str>) -> EventSeverity {
    match value {
        Some("VERY_HIGH") => EventSeverity::Critical,
        Some("HIGH") => EventSeverity::Error,
        Some("MEDIUM" | "WARNING") => EventSeverity::Warning,
        _ => EventSeverity::Info,
    }
}

fn actor_mac(parameters: &Map<String, Value>, actors: &[&str]) -> Option<MacAddress> {
    actors.iter().find_map(|actor| {
        let value = parameters.get(*actor)?;
        value
            .get("mac")
            .or_else(|| value.get("id"))
            .unwrap_or(value)
            .as_str()
            .and_then(|value| MacAddress::try_new(value).ok())
    })
}

/// Replace only placeholders in the original template. Braces in names are data.
fn render_message(template: &str, parameters: &Map<String, Value>) -> String {
    let mut output = String::with_capacity(template.len());
    let mut remaining = template;
    while let Some((prefix, after_open)) = remaining.split_once('{') {
        output.push_str(prefix);
        let Some((name, suffix)) = after_open.split_once('}') else {
            output.push('{');
            output.push_str(after_open);
            return output;
        };
        if let Some(value) = parameters.get(name).and_then(parameter_text) {
            output.push_str(&value);
        } else {
            output.push('{');
            output.push_str(name);
            output.push('}');
        }
        remaining = suffix;
    }
    output.push_str(remaining);
    output
}

fn parameter_text(value: &Value) -> Option<Cow<'_, str>> {
    scalar_text(value).or_else(|| {
        ["name", "hostname", "id", "value"].iter().find_map(|key| {
            value
                .get(key)
                .and_then(scalar_text)
                .filter(|text| !text.is_empty())
        })
    })
}

fn scalar_text(value: &Value) -> Option<Cow<'_, str>> {
    match value {
        Value::String(text) => Some(Cow::Borrowed(text)),
        Value::Number(number) => Some(Cow::Owned(number.to_string())),
        Value::Bool(value) => Some(Cow::Borrowed(if *value { "true" } else { "false" })),
        _ => None,
    }
}

/// Hash canonical raw contents so ID-less records retain their identity after
/// conversion, without exposing unknown payload fields in a synthetic ID.
pub(crate) fn content_identity(entry: &impl Serialize) -> Result<String, Error> {
    let mut value = serde_json::to_value(entry)
        .map_err(|_| invalid_history("event identity could not be encoded"))?;
    value.sort_all_objects();
    let encoded = serde_json::to_vec(&value)
        .map_err(|_| invalid_history("event identity could not be encoded"))?;
    Ok(URL_SAFE_NO_PAD.encode(Sha256::digest(encoded)))
}

pub(crate) fn invalid_history(message: &str) -> Error {
    Error::Deserialization {
        message: message.into(),
        body: String::new(),
    }
}

#[cfg(test)]
mod tests;
