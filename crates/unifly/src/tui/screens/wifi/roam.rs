//! Roam history rows, parsed from the controller's client connection log.

use std::cmp::Reverse;

use serde_json::Value;

/// One roam event as the Roaming table shows it.
#[derive(Debug, Clone)]
pub(super) struct ParsedRoamRow {
    /// When it happened, in epoch milliseconds.
    pub at: Option<i64>,
    pub time: String,
    pub event: String,
    pub from_ap: String,
    pub to_ap: String,
    pub signal: String,
    pub radio: String,
}

/// Parse a client's roam events, newest first. Events without a timestamp
/// follow the dated ones in the order the controller sent them.
pub(super) fn parse_roam_rows(events: &[Value]) -> Vec<ParsedRoamRow> {
    let mut rows: Vec<_> = events.iter().map(parse_roam_row).collect();
    rows.sort_by_key(|row| Reverse(row.at));
    rows
}

fn parse_roam_row(event: &Value) -> ParsedRoamRow {
    let at = ["timestamp", "time"]
        .iter()
        .find_map(|key| event.get(key).and_then(Value::as_i64))
        .map(epoch_millis);
    let parameters = event.get("parameters").unwrap_or(event);
    let dash = || "─".to_string();

    let band = parameter(parameters, &["RADIO_BAND", "band"]).map(|band| band_name(&band));
    let channel = parameter(parameters, &["CHANNEL", "channel"]);
    let radio = match (band, channel) {
        (Some(band), Some(channel)) => format!("{band} · ch {channel}"),
        (Some(band), None) => band,
        (None, Some(channel)) => format!("ch {channel}"),
        (None, None) => dash(),
    };

    ParsedRoamRow {
        at,
        time: at.and_then(local_time).unwrap_or_else(dash),
        event: json_string(event, &["key", "event_type", "eventType", "event"])
            .map_or_else(dash, |key| event_label(&key)),
        from_ap: parameter(parameters, &["DEVICE_FROM", "from_ap"]).unwrap_or_else(dash),
        to_ap: parameter(parameters, &["DEVICE_TO", "to_ap", "ap_mac"]).unwrap_or_else(dash),
        signal: parameter(parameters, &["SIGNAL_STRENGTH", "signal"])
            .map_or_else(dash, |value| format!("{value} dBm")),
        radio,
    }
}

/// The controller stamps some logs in seconds and some in milliseconds.
fn epoch_millis(timestamp: i64) -> i64 {
    if timestamp > 1_000_000_000_000 {
        timestamp
    } else {
        timestamp.saturating_mul(1000)
    }
}

/// A timestamp in the terminal's time zone, with its date: a history
/// spans days.
fn local_time(millis: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(millis).map(|utc| {
        utc.with_timezone(&chrono::Local)
            .format("%m-%d %H:%M:%S")
            .to_string()
    })
}

/// A band code as the rest of the app names bands.
pub(super) fn band_name(code: &str) -> String {
    match code {
        "ng" | "2.4g" => "2.4 GHz".to_owned(),
        "na" | "5g" => "5 GHz".to_owned(),
        "6e" | "6g" => "6 GHz".to_owned(),
        other => other.to_owned(),
    }
}

/// A log key such as `CLIENT_CONNECTED_WIRELESS` as a word or two. Keys in
/// any other shape are shown as they are.
fn event_label(key: &str) -> String {
    let shouting = key
        .chars()
        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_');
    if !shouting {
        return key.to_owned();
    }
    let core = key.strip_prefix("CLIENT_").unwrap_or(key);
    let core = core.strip_suffix("_WIRELESS").unwrap_or(core);
    let mut label = core.replace('_', " ").to_lowercase();
    if let Some(first) = label.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    label
}

/// A log parameter, which the controller nests as `{ "name": value }`.
/// An empty value counts as missing.
fn parameter(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        let found = value.get(key)?;
        let found = found.get("name").unwrap_or(found);
        let text = match found {
            Value::String(text) => text.clone(),
            Value::Number(number) => number.to_string(),
            _ => return None,
        };
        (!text.is_empty()).then_some(text)
    })
}

pub(super) fn json_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| match value.get(key)? {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) if number.is_i64() || number.is_u64() => Some(number.to_string()),
        _ => None,
    })
}

pub(super) fn json_i32(value: &Value, keys: &[&str]) -> Option<i32> {
    keys.iter().find_map(|key| {
        value
            .get(key)
            .and_then(Value::as_i64)
            .and_then(|found| i32::try_from(found).ok())
    })
}

pub(super) fn json_u64(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| value.get(key).and_then(Value::as_u64))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn roam(key: &str, timestamp: i64) -> Value {
        json!({
            "key": key,
            "event_type": "Label",
            "timestamp": timestamp,
            "parameters": {
                "DEVICE_FROM": { "name": "ap-atrium" },
                "DEVICE_TO": { "name": "ap-roof" },
                "SIGNAL_STRENGTH": { "name": "-61" },
                "RADIO_BAND": { "name": "5g" },
                "CHANNEL": { "name": "36" },
            }
        })
    }

    #[test]
    fn a_history_that_crosses_midnight_stays_in_time_order() {
        // 23:50 one day and 00:10 the next, in whatever zone the test runs:
        // ordering by the clock text would put the older one first.
        let late = 1_760_000_000_000_i64;
        let early_next_day = late + 20 * 60 * 1000;
        let rows = parse_roam_rows(&[
            roam("CLIENT_ROAMED", late),
            roam("CLIENT_ROAMED", early_next_day),
            roam("CLIENT_ROAMED", late - 24 * 60 * 60 * 1000),
        ]);

        let order: Vec<_> = rows.iter().map(|row| row.at).collect();
        assert_eq!(
            order,
            [
                Some(early_next_day),
                Some(late),
                Some(late - 24 * 60 * 60 * 1000)
            ]
        );
    }

    #[test]
    fn times_are_local_and_carry_the_date() {
        let at = 1_760_000_000_000_i64;
        let expected = chrono::DateTime::from_timestamp_millis(at)
            .expect("a valid instant")
            .with_timezone(&chrono::Local)
            .format("%m-%d %H:%M:%S")
            .to_string();

        let rows = parse_roam_rows(&[roam("CLIENT_ROAMED", at)]);
        assert_eq!(rows[0].time, expected);

        // The same instant in seconds reads the same.
        let rows = parse_roam_rows(&[roam("CLIENT_ROAMED", at / 1000)]);
        assert_eq!(rows[0].time, expected);
    }

    #[test]
    fn the_event_comes_from_the_log_key_and_the_band_reads_like_the_app() {
        let rows = parse_roam_rows(&[
            roam("CLIENT_CONNECTED_WIRELESS", 3),
            roam("CLIENT_ROAMED", 2),
            roam("EVT_WU_Roam", 1),
        ]);

        let events: Vec<_> = rows.iter().map(|row| row.event.as_str()).collect();
        assert_eq!(events, ["Connected", "Roamed", "EVT_WU_Roam"]);
        assert_eq!(rows[0].radio, "5 GHz · ch 36");
        assert_eq!(rows[0].signal, "-61 dBm");
    }

    #[test]
    fn an_event_without_a_key_falls_back_to_its_type_and_blank_aps_are_dashes() {
        let rows = parse_roam_rows(&[json!({
            "event_type": "Roamed",
            "parameters": { "DEVICE_FROM": { "name": "" }, "DEVICE_TO": { "name": "ap-roof" } }
        })]);

        assert_eq!(rows[0].event, "Roamed");
        assert_eq!(rows[0].from_ap, "─");
        assert_eq!(rows[0].to_ap, "ap-roof");
        assert_eq!(rows[0].time, "─");
        assert_eq!(rows[0].radio, "─");
    }
}
