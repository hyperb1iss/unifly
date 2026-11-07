//! Historical statistics, selected through the controller's statistics source.
//!
//! Legacy reports retain their requested resolution. Modern reports retain native
//! controller buckets and expose their actual `duration` in milliseconds. Modern
//! controllers support 5-minute, hourly, 8-hour, and monthly buckets; daily requests
//! use native 8-hour buckets without inventing daily aggregates. On modern
//! controllers, `interval` selects the default window when bounds are absent:
//! 5minutes:12h, hourly:7d, daily:30d, monthly:365d. Missing measurements stay absent.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Error, SessionClient};

mod dpi;
mod modern;
mod wire;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum StatsBackend {
    Legacy,
    Modern,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Report {
    Site,
    Device,
    Client,
    Gateway,
}

impl Report {
    fn suffix(self) -> &'static str {
        match self {
            Self::Site => "site",
            Self::Device => "device",
            Self::Client => "user",
            Self::Gateway => "gw",
        }
    }

    fn attributes(self) -> &'static [&'static str] {
        match self {
            Self::Site => &["bytes", "num_sta", "time", "wlan-num_sta", "lan-num_sta"],
            Self::Device => &["bytes", "num_sta", "time", "rx_bytes", "tx_bytes"],
            Self::Client => &["bytes", "time", "rx_bytes", "tx_bytes"],
            Self::Gateway => &[
                "bytes",
                "time",
                "wan-tx_bytes",
                "wan-rx_bytes",
                "lan-rx_bytes",
                "lan-tx_bytes",
            ],
        }
    }
}

struct ReportQuery<'a> {
    interval: &'a str,
    start: Option<i64>,
    end: Option<i64>,
    macs: Option<&'a [String]>,
    attrs: Option<&'a [String]>,
}

impl SessionClient {
    async fn stats_backend(&self) -> Result<StatsBackend, Error> {
        self.stats_backend
            .get_or_try_init(|| async {
                match self.raw_get("v2/api/stats/source").await {
                    Ok(value) => wire::decode(&value),
                    Err(error) if error.is_not_found() => Ok(StatsBackend::Legacy),
                    Err(error) => Err(error),
                }
            })
            .await
            .copied()
    }

    async fn stats_report(
        &self,
        report: Report,
        query: ReportQuery<'_>,
    ) -> Result<Vec<Value>, Error> {
        match self.stats_backend().await? {
            StatsBackend::Modern => modern::report(self, report, &query).await,
            StatsBackend::Legacy => {
                let mut body = json!({"attrs": query.attrs.map_or_else(|| json!(report.attributes()), |attrs| json!(attrs))});
                if let Some(start) = query.start {
                    body["start"] = json!(start);
                }
                if let Some(end) = query.end {
                    body["end"] = json!(end);
                }
                if let Some(macs) = query.macs {
                    body["macs"] = json!(macs);
                }
                self.post_read(
                    self.site_url(&format!(
                        "stat/report/{}.{}",
                        query.interval,
                        report.suffix()
                    )),
                    &body,
                )
                .await
            }
        }
    }

    /// Fetch site history with optional Unix millisecond bounds.
    /// Modern controllers return native buckets with an explicit millisecond `duration`.
    pub async fn get_site_stats(
        &self,
        interval: &str,
        start: Option<i64>,
        end: Option<i64>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<Value>, Error> {
        self.stats_report(
            Report::Site,
            ReportQuery {
                interval,
                start,
                end,
                macs: None,
                attrs,
            },
        )
        .await
    }

    /// Fetch per-device history using the default window for `interval`.
    pub async fn get_device_stats(
        &self,
        interval: &str,
        macs: Option<&[String]>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<Value>, Error> {
        self.get_device_stats_in_range(interval, None, None, macs, attrs)
            .await
    }

    /// Fetch device history with optional Unix millisecond bounds.
    /// Modern controllers return native buckets with an explicit millisecond `duration`.
    pub async fn get_device_stats_in_range(
        &self,
        interval: &str,
        start: Option<i64>,
        end: Option<i64>,
        macs: Option<&[String]>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<Value>, Error> {
        self.stats_report(
            Report::Device,
            ReportQuery {
                interval,
                start,
                end,
                macs,
                attrs,
            },
        )
        .await
    }

    /// Fetch per-client history using the default window for `interval`.
    pub async fn get_client_stats(
        &self,
        interval: &str,
        macs: Option<&[String]>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<Value>, Error> {
        self.get_client_stats_in_range(interval, None, None, macs, attrs)
            .await
    }

    /// Fetch client history with optional Unix millisecond bounds.
    /// Modern controllers return native buckets with an explicit millisecond `duration`.
    pub async fn get_client_stats_in_range(
        &self,
        interval: &str,
        start: Option<i64>,
        end: Option<i64>,
        macs: Option<&[String]>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<Value>, Error> {
        self.stats_report(
            Report::Client,
            ReportQuery {
                interval,
                start,
                end,
                macs,
                attrs,
            },
        )
        .await
    }

    /// Fetch gateway history with optional Unix millisecond bounds.
    /// Modern controllers return native buckets with an explicit millisecond `duration`.
    pub async fn get_gateway_stats(
        &self,
        interval: &str,
        start: Option<i64>,
        end: Option<i64>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<Value>, Error> {
        self.stats_report(
            Report::Gateway,
            ReportQuery {
                interval,
                start,
                end,
                macs: None,
                attrs,
            },
        )
        .await
    }
}
