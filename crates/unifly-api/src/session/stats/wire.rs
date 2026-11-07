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
    let path = series_path(schema, window, site_id);
    decode(&client.raw_get(&path).await?)
}

fn series_path(schema: &str, window: Window, site_id: &str) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair("schema", schema);
    query.append_pair("bucket", window.bucket.as_str());
    query.append_pair("start", &window.start.to_string());
    query.append_pair("end", &window.end.to_string());
    query.append_pair("filter", &format!("siteId.eq('{site_id}')"));
    format!("v2/api/stats/series?{}", query.finish())
}
