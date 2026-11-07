use serde_json::json;

use super::*;

fn entry(extra: &Value) -> SystemLogEntry {
    let mut value = json!({
        "timestamp": 1_793_980_800_000_i64,
        "id": "opaque-event-id",
        "key": "CLIENT_CONNECTED_WIRELESS",
        "category": "CLIENT_DEVICES",
        "severity": "LOW"
    });
    value.as_object_mut().expect("object").extend(
        extra
            .as_object()
            .expect("object")
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    serde_json::from_value(value).expect("fixture")
}

#[test]
fn nested_parameters_render_once_with_literal_braces_and_unicode() {
    let event = Event::try_from(entry(&json!({
        "message_raw": "{CLIENT} joined {WLAN} via {DEVICE}: {UNKNOWN} {unfinished",
        "parameters": {
            "CLIENT": {"name": "Zoë {WLAN}", "hostname": "other"},
            "WLAN": {"hostname": "日本語"},
            "DEVICE": {"id": "00:11:22:33:44:55"}
        }
    })))
    .expect("event");
    assert_eq!(
        event.message,
        "Zoë {WLAN} joined 日本語 via 00:11:22:33:44:55: {UNKNOWN} {unfinished"
    );
    assert_eq!(
        event.device_mac.as_ref().map(MacAddress::as_str),
        Some("00:11:22:33:44:55")
    );
    assert_eq!(event.client_mac, None);
}

#[test]
fn message_precedes_templates_and_title_is_a_fallback() {
    let direct = Event::try_from(entry(&json!({
        "message": "already rendered {CLIENT}", "message_raw": "unused"
    })))
    .expect("event");
    assert_eq!(direct.message, "already rendered {CLIENT}");
    let title = Event::try_from(entry(&json!({
        "message": " ", "message_raw": "", "title_raw": "{DEVICE} restarted",
        "parameters": {"DEVICE": {"name": "", "hostname": "switch"}}
    })))
    .expect("event");
    assert_eq!(title.message, "switch restarted");
}

#[test]
fn event_type_is_not_identity_and_unknown_categories_survive() {
    let event = Event::try_from(entry(&json!({
        "id": null, "category": "FUTURE_CATEGORY", "event": "CONNECTED"
    })))
    .expect("event");
    assert_eq!(event.id, None);
    assert_eq!(event.raw_key.as_deref(), Some("CLIENT_CONNECTED_WIRELESS"));
    assert_eq!(event.event_type, "CONNECTED");
    assert_eq!(event.category, EventCategory::Unknown);
}

#[test]
fn category_and_severity_keep_ips_and_admin_information() {
    for (name, expected) in [
        ("AUDIT", EventCategory::Admin),
        ("SECURITY", EventCategory::Firewall),
        ("UNIFI_ETHERNET_PORTS", EventCategory::Device),
        ("INTERNET_AND_WAN", EventCategory::Network),
        ("VPN", EventCategory::Vpn),
    ] {
        let event = Event::try_from(entry(&json!({"category": name}))).expect("event");
        assert_eq!(event.category, expected);
    }
    for (name, expected) in [
        ("LOW", EventSeverity::Info),
        ("INFO", EventSeverity::Info),
        ("MEDIUM", EventSeverity::Warning),
        ("WARNING", EventSeverity::Warning),
        ("HIGH", EventSeverity::Error),
        ("VERY_HIGH", EventSeverity::Critical),
    ] {
        let event = Event::try_from(entry(&json!({"severity": name}))).expect("event");
        assert_eq!(event.severity, expected);
    }
}

#[test]
fn actor_macs_reject_addresses_and_non_actor_identifiers() {
    let event = Event::try_from(entry(&json!({"parameters": {
        "DEVICE": {"id": "100.100.100.100"},
        "CLIENT": {"id": "aa:bb:cc:dd:ee:ff"},
        "DST_IP": {"id": "11:22:33:44:55:66"},
        "INITIATOR_ID": {"id": "22:33:44:55:66:77"}
    }})))
    .expect("event");
    assert_eq!(event.device_mac, None);
    assert_eq!(
        event.client_mac.as_ref().map(MacAddress::as_str),
        Some("aa:bb:cc:dd:ee:ff")
    );
}

#[test]
fn bad_timestamps_fail_instead_of_becoming_current_time() {
    assert!(Event::try_from(entry(&json!({"timestamp": i64::MAX}))).is_err());
    let legacy =
        serde_json::from_value(json!({"_id": "legacy", "datetime": "garbage"})).expect("legacy");
    assert!(legacy_history_event(legacy).is_err());
    let legacy = serde_json::from_value(json!({"_id": "legacy", "time": 1_793_980_800_000_i64}))
        .expect("legacy");
    assert_eq!(
        legacy_history_event(legacy)
            .expect("event")
            .timestamp
            .timestamp_millis(),
        1_793_980_800_000_i64
    );
}

#[test]
fn alarms_require_ids_and_only_read_is_archived() {
    assert!(Alarm::try_from(entry(&json!({"id": null}))).is_err());
    assert!(Alarm::try_from(entry(&json!({"id": ""}))).is_err());
    for (status, archived) in [
        ("NEW", false),
        ("READ", true),
        ("STALED", false),
        ("HIDDEN", false),
    ] {
        let alarm = Alarm::try_from(entry(
            &json!({"status": status, "category": "SECURITY", "severity": "HIGH"}),
        ))
        .expect("alarm");
        assert_eq!(alarm.archived, archived);
        assert_eq!(alarm.id.to_string(), "opaque-event-id");
        assert_eq!(alarm.category, EventCategory::Firewall);
        assert_eq!(alarm.severity, EventSeverity::Error);
    }
}
