use chrono::{DateTime, Datelike, TimeZone, Utc};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use super::ReportQuery;
use crate::{Error, SessionClient};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Series {
    pub site_id: String,
    pub device_mac: Option<String>,
    pub client_id: Option<String>,
    pub unit_id: Option<Value>,
    #[serde(rename = "series")]
    pub points: Vec<Point>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Point {
    pub timestamp: i64,
    pub duration: u64,
    #[serde(flatten)]
    pub values: Map<String, Value>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Window {
    pub start: i64,
    pub end: i64,
    bucket: Bucket,
}

impl Window {
    pub fn from_query(query: &ReportQuery<'_>) -> Result<Self, Error> {
        let (hours, bucket) = match query.interval {
            "5minutes" => (12, Bucket::FiveMinutes),
            "hourly" => (7 * 24, Bucket::Hourly),
            "daily" => (30 * 24, Bucket::EightHours),
            "monthly" => (365 * 24, Bucket::Monthly),
            other => return Err(invalid(format!("Unknown statistics interval: {other}"))),
        };
        let end = query
            .end
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
        let start = query
            .start
            .map_or_else(|| end.checked_sub(hours * 3_600_000), Some)
            .ok_or_else(|| invalid("Statistics time range overflows Unix milliseconds"))?;
        if start > end || end.checked_sub(start).is_none() {
            return Err(invalid(
                "Statistics start must not exceed end or overflow the time range",
            ));
        }
        Ok(Self { start, end, bucket })
    }

    pub fn overlaps(self, point: &Point) -> Result<bool, Error> {
        let duration = i64::try_from(point.duration)
            .ok()
            .filter(|duration| *duration > 0)
            .ok_or_else(|| invalid("Statistics response contains an invalid bucket duration"))?;
        let end = point
            .timestamp
            .checked_add(duration)
            .ok_or_else(|| invalid("Statistics bucket end overflows Unix milliseconds"))?;
        Ok(point.timestamp <= self.end && end > self.start)
    }

    fn wire_bounds(self) -> Result<(i64, i64), Error> {
        if !matches!(self.bucket, Bucket::Monthly) {
            return Ok((self.start, self.end));
        }
        // The controller's epoch-anchored calendar rounds some month boundaries
        // inconsistently across time zones. January anchors avoid that clamp;
        // overlaps() still selects against the caller's original window.
        Ok((
            january_anchor(self.start, -1)?,
            january_anchor(self.end, 1)?,
        ))
    }
}

fn january_anchor(timestamp: i64, year_offset: i32) -> Result<i64, Error> {
    DateTime::from_timestamp_millis(timestamp)
        .and_then(|date| date.year().checked_add(year_offset))
        .and_then(|year| Utc.with_ymd_and_hms(year, 1, 2, 0, 0, 0).single())
        .map(|date| date.timestamp_millis())
        .ok_or_else(|| invalid("Monthly statistics bounds exceed the supported calendar range"))
}

#[derive(Debug, Clone, Copy)]
enum Bucket {
    FiveMinutes,
    Hourly,
    EightHours,
    Monthly,
}

impl Bucket {
    fn as_str(self) -> &'static str {
        match self {
            Self::FiveMinutes => "OF_5_MINUTES",
            Self::Hourly => "OF_1_HOUR",
            Self::EightHours => "OF_8_HOURS",
            Self::Monthly => "OF_1_MONTH",
        }
    }
}

pub(super) fn decode<T: DeserializeOwned>(value: &Value) -> Result<T, Error> {
    serde_json::from_value(value.clone()).map_err(|error| Error::Deserialization {
        message: error.to_string(),
        body: value.to_string(),
    })
}

pub(super) fn invalid(message: impl Into<String>) -> Error {
    Error::SessionApi {
        message: message.into(),
    }
}

pub(super) async fn fetch(
    client: &SessionClient,
    schema: &str,
    window: Window,
    site_id: &str,
) -> Result<Vec<Series>, Error> {
    let path = series_path(schema, window, site_id)?;
    decode(&client.raw_get(&path).await?)
}

fn series_path(schema: &str, window: Window, site_id: &str) -> Result<String, Error> {
    let (start, end) = window.wire_bounds()?;
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair("schema", schema);
    query.append_pair("bucket", window.bucket.as_str());
    query.append_pair("start", &start.to_string());
    query.append_pair("end", &end.to_string());
    query.append_pair("filter", &format!("siteId.eq('{site_id}')"));
    Ok(format!("v2/api/stats/series?{}", query.finish()))
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::{Bucket, Window};

    fn millis(value: &str) -> i64 {
        DateTime::parse_from_rfc3339(value)
            .expect("fixture date")
            .timestamp_millis()
    }

    #[test]
    fn monthly_wire_padding_handles_leap_days_and_year_boundaries() {
        for (start, end, padded_start, padded_end) in [
            (
                "2024-02-29T12:00:00Z",
                "2024-03-01T00:00:00Z",
                "2023-01-02T00:00:00Z",
                "2025-01-02T00:00:00Z",
            ),
            (
                "2025-12-31T23:00:00Z",
                "2026-01-01T01:00:00Z",
                "2024-01-02T00:00:00Z",
                "2027-01-02T00:00:00Z",
            ),
        ] {
            let window = Window {
                start: millis(start),
                end: millis(end),
                bucket: Bucket::Monthly,
            };
            assert_eq!(
                window.wire_bounds().expect("calendar bounds"),
                (millis(padded_start), millis(padded_end))
            );
            assert_eq!(window.start, millis(start));
            assert_eq!(window.end, millis(end));
        }
    }

    #[test]
    fn monthly_wire_padding_rejects_unrepresentable_calendar_bounds() {
        for timestamp in [
            i64::MIN,
            i64::MAX,
            DateTime::<Utc>::MIN_UTC.timestamp_millis(),
            DateTime::<Utc>::MAX_UTC.timestamp_millis(),
        ] {
            let window = Window {
                start: timestamp,
                end: timestamp,
                bucket: Bucket::Monthly,
            };
            assert!(window.wire_bounds().is_err());
        }
    }
}
