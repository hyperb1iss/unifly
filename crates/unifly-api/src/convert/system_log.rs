use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

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
            id: entry.id.filter(|id| !id.is_empty()).map(EntityId::from),
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
            raw_key: entry.key,
            source: DataSource::SessionApi,
        })
    }
}

impl TryFrom<SystemLogEntry> for Alarm {
    type Error = Error;

    fn try_from(entry: SystemLogEntry) -> Result<Self, Self::Error> {
        let archived = entry.status.as_deref() == Some("READ");
        let event = Event::try_from(entry)?;
        Ok(Self {
            id: event
                .id
                .ok_or_else(|| invalid_history("critical alert has no ID"))?,
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
            output.push_str(value);
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

fn parameter_text(value: &Value) -> Option<&str> {
    value.as_str().or_else(|| {
        ["name", "hostname", "id"].iter().find_map(|key| {
            value
                .get(key)
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
        })
    })
}

pub(crate) fn invalid_history(message: &str) -> Error {
    Error::Deserialization {
        message: message.into(),
        body: String::new(),
    }
}

#[cfg(test)]
mod tests;
