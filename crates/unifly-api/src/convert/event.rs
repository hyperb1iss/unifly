use chrono::Utc;

use crate::model::common::DataSource;
use crate::model::entity_id::{EntityId, MacAddress};
use crate::model::event::{Alarm, Event, EventCategory, EventSeverity};
use crate::session::models::{SessionAlarm, SessionEvent};
use crate::websocket::UnifiEvent;

use super::helpers::{parse_datetime, resolve_event_templates};

pub(super) fn event_ssid(fields: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    ["essid", "ssid"]
        .into_iter()
        .find_map(|key| {
            fields
                .get(key)
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
        })
        .map(str::to_owned)
}

fn map_event_category(subsystem: Option<&String>) -> EventCategory {
    match subsystem.map(String::as_str) {
        Some("wlan" | "lan" | "wan") => EventCategory::Network,
        Some("device") => EventCategory::Device,
        Some("client") => EventCategory::Client,
        Some("system") => EventCategory::System,
        Some("admin") => EventCategory::Admin,
        Some("firewall") => EventCategory::Firewall,
        Some("vpn") => EventCategory::Vpn,
        _ => EventCategory::Unknown,
    }
}

impl From<SessionEvent> for Event {
    fn from(e: SessionEvent) -> Self {
        let ssid = event_ssid(&e.extra);
        Event {
            id: Some(EntityId::from(e.id)),
            timestamp: parse_datetime(e.datetime.as_ref()).unwrap_or_else(Utc::now),
            category: map_event_category(e.subsystem.as_ref()),
            severity: EventSeverity::Info,
            event_type: e.key.clone().unwrap_or_default(),
            message: resolve_event_templates(
                &e.msg.unwrap_or_default(),
                &serde_json::Value::Object(e.extra),
            ),
            device_mac: None,
            client_mac: None,
            site_id: e.site_id.map(EntityId::from),
            ssid,
            raw_key: e.key,
            source: DataSource::SessionApi,
        }
    }
}

// ── Alarm → Event ────────────────────────────────────────────────

impl From<SessionAlarm> for Event {
    fn from(a: SessionAlarm) -> Self {
        let ssid = event_ssid(&a.extra);
        Event {
            id: Some(EntityId::from(a.id)),
            timestamp: parse_datetime(a.datetime.as_ref()).unwrap_or_else(Utc::now),
            category: EventCategory::System,
            severity: EventSeverity::Warning,
            event_type: a.key.clone().unwrap_or_default(),
            message: a.msg.unwrap_or_default(),
            device_mac: None,
            client_mac: None,
            site_id: None,
            ssid,
            raw_key: a.key,
            source: DataSource::SessionApi,
        }
    }
}

impl From<SessionAlarm> for Alarm {
    fn from(a: SessionAlarm) -> Self {
        Alarm {
            id: EntityId::from(a.id),
            timestamp: parse_datetime(a.datetime.as_ref()).unwrap_or_else(Utc::now),
            category: EventCategory::System,
            severity: EventSeverity::Warning,
            message: a.msg.unwrap_or_default(),
            archived: a.archived.unwrap_or(false),
            device_mac: None,
            site_id: None,
        }
    }
}

// ── WebSocket Event ──────────────────────────────────────────────

fn infer_ws_severity(key: &str) -> EventSeverity {
    let upper = key.to_uppercase();
    if upper.contains("ERROR") || upper.contains("FAIL") {
        EventSeverity::Error
    } else if upper.contains("DISCONNECT") || upper.contains("LOST") || upper.contains("DOWN") {
        EventSeverity::Warning
    } else {
        EventSeverity::Info
    }
}

impl From<UnifiEvent> for Event {
    fn from(e: UnifiEvent) -> Self {
        let category = map_event_category(Some(&e.subsystem));
        let severity = infer_ws_severity(&e.key);

        let device_mac = e
            .extra
            .get("mac")
            .or_else(|| e.extra.get("sw"))
            .or_else(|| e.extra.get("ap"))
            .and_then(|v| v.as_str())
            .map(MacAddress::new);

        let client_mac = e
            .extra
            .get("user")
            .or_else(|| e.extra.get("sta"))
            .and_then(|v| v.as_str())
            .map(MacAddress::new);

        let site_id = if e.site_id.is_empty() {
            None
        } else {
            Some(EntityId::Legacy(e.site_id))
        };

        Event {
            id: None,
            timestamp: parse_datetime(e.datetime.as_ref()).unwrap_or_else(Utc::now),
            category,
            severity,
            event_type: e.key.clone(),
            message: resolve_event_templates(&e.message.unwrap_or_default(), &e.extra),
            device_mac,
            client_mac,
            site_id,
            ssid: e.extra.as_object().and_then(event_ssid),
            raw_key: Some(e.key),
            source: DataSource::SessionApi,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alarm_event_preserves_ssid_for_message_sanitization() {
        let alarm: SessionAlarm = serde_json::from_value(serde_json::json!({
            "_id": "alarm-id", "msg": "OfficeWiFi is unavailable", "essid": "OfficeWiFi"
        }))
        .expect("alarm");
        let event = Event::from(alarm);
        assert_eq!(event.ssid.as_deref(), Some("OfficeWiFi"));
    }

    #[test]
    fn event_category_mapping() {
        assert_eq!(
            map_event_category(Some(&"wlan".into())),
            EventCategory::Network
        );
        assert_eq!(
            map_event_category(Some(&"device".into())),
            EventCategory::Device
        );
        assert_eq!(
            map_event_category(Some(&"admin".into())),
            EventCategory::Admin
        );
        assert_eq!(map_event_category(None), EventCategory::Unknown);
    }
}
