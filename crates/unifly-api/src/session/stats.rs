// Session API statistics endpoints
//
// Historical reports (stat/report/) and DPI statistics (stat/sitedpi).
// These endpoints return loosely-typed JSON because the field set varies
// by report type, interval, and firmware version.

use serde_json::json;
use tracing::debug;

use crate::error::Error;
use crate::session::client::SessionClient;

fn attrs_or_default(attrs: Option<&[String]>, default: &[&str]) -> serde_json::Value {
    attrs.map_or_else(|| json!(default), |custom| json!(custom))
}

impl SessionClient {
    /// Fetch site-level historical statistics with optional Unix millisecond bounds.
    ///
    /// `POST /api/s/{site}/stat/report/{interval}.site`
    ///
    /// The `interval` parameter should be one of: `"5minutes"`, `"hourly"`, `"daily"`.
    /// Returns loosely-typed JSON because the field set varies by report type.
    pub async fn get_site_stats(
        &self,
        interval: &str,
        start: Option<i64>,
        end: Option<i64>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, Error> {
        let path = format!("stat/report/{interval}.site");
        let url = self.site_url(&path);
        debug!(interval, ?start, ?end, "fetching site stats");

        // The report endpoint requires a POST with attribute selection.
        // Requesting common attributes; the API ignores unknown ones.
        let mut body = json!({
            "attrs": attrs_or_default(
                attrs,
                &["bytes", "num_sta", "time", "wlan-num_sta", "lan-num_sta"],
            ),
        });
        if let Some(s) = start {
            body["start"] = json!(s);
        }
        if let Some(e) = end {
            body["end"] = json!(e);
        }

        self.post_read(url, &body).await
    }

    /// Fetch per-device historical statistics.
    ///
    /// `POST /api/s/{site}/stat/report/{interval}.device`
    ///
    /// If `macs` is provided, results are filtered to those devices.
    pub async fn get_device_stats(
        &self,
        interval: &str,
        macs: Option<&[String]>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, Error> {
        self.get_device_stats_in_range(interval, None, None, macs, attrs)
            .await
    }

    /// Fetch per-device historical statistics within optional Unix millisecond bounds.
    pub async fn get_device_stats_in_range(
        &self,
        interval: &str,
        start: Option<i64>,
        end: Option<i64>,
        macs: Option<&[String]>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, Error> {
        let path = format!("stat/report/{interval}.device");
        let url = self.site_url(&path);
        debug!(interval, "fetching device stats");

        let mut body = json!({
            "attrs": attrs_or_default(attrs, &["bytes", "num_sta", "time", "rx_bytes", "tx_bytes"]),
        });
        if let Some(m) = macs {
            body["macs"] = json!(m);
        }
        if let Some(start) = start {
            body["start"] = json!(start);
        }
        if let Some(end) = end {
            body["end"] = json!(end);
        }

        self.post_read(url, &body).await
    }

    /// Fetch per-client historical statistics.
    ///
    /// `POST /api/s/{site}/stat/report/{interval}.user`
    ///
    /// If `macs` is provided, results are filtered to those clients.
    pub async fn get_client_stats(
        &self,
        interval: &str,
        macs: Option<&[String]>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, Error> {
        self.get_client_stats_in_range(interval, None, None, macs, attrs)
            .await
    }

    /// Fetch per-client historical statistics within optional Unix millisecond bounds.
    pub async fn get_client_stats_in_range(
        &self,
        interval: &str,
        start: Option<i64>,
        end: Option<i64>,
        macs: Option<&[String]>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, Error> {
        let path = format!("stat/report/{interval}.user");
        let url = self.site_url(&path);
        debug!(interval, "fetching client stats");

        let mut body = json!({
            "attrs": attrs_or_default(attrs, &["bytes", "time", "rx_bytes", "tx_bytes"]),
        });
        if let Some(m) = macs {
            body["macs"] = json!(m);
        }
        if let Some(start) = start {
            body["start"] = json!(start);
        }
        if let Some(end) = end {
            body["end"] = json!(end);
        }

        self.post_read(url, &body).await
    }

    /// Fetch gateway historical statistics with optional Unix millisecond bounds.
    ///
    /// `POST /api/s/{site}/stat/report/{interval}.gw`
    pub async fn get_gateway_stats(
        &self,
        interval: &str,
        start: Option<i64>,
        end: Option<i64>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, Error> {
        let path = format!("stat/report/{interval}.gw");
        let url = self.site_url(&path);
        debug!(interval, ?start, ?end, "fetching gateway stats");

        let mut body = json!({
            "attrs": attrs_or_default(
                attrs,
                &[
                    "bytes",
                    "time",
                    "wan-tx_bytes",
                    "wan-rx_bytes",
                    "lan-rx_bytes",
                    "lan-tx_bytes",
                ],
            ),
        });
        if let Some(s) = start {
            body["start"] = json!(s);
        }
        if let Some(e) = end {
            body["end"] = json!(e);
        }

        self.post_read(url, &body).await
    }

    /// Fetch DPI (Deep Packet Inspection) statistics.
    ///
    /// Tries multiple session DPI endpoints for compatibility across firmware
    /// versions:
    /// 1. `stat/stadpi` with MAC filter — when `macs` is provided
    /// 2. `stat/sitedpi` with type filter — site-level aggregated stats
    /// 3. `stat/dpi` (unfiltered GET) — fallback for firmware that only
    ///    populates this endpoint
    ///
    /// The `group_by` parameter selects the DPI grouping: `"by_app"` or `"by_cat"`.
    /// Returns empty data if DPI tracking is not enabled on the controller.
    pub async fn get_dpi_stats(
        &self,
        group_by: &str,
        macs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, Error> {
        // Per-station endpoint when filtering by MAC addresses.
        if let Some(m) = macs {
            let url = self.site_url("stat/stadpi");
            debug!(group_by, "fetching station DPI stats (filtered)");
            let body = json!({"type": group_by, "macs": m});
            return self.post(url, &body).await;
        }

        // Try v2 flow statistics endpoint first (Network Application 9+).
        let v2_url = self.site_url_v2("traffic-flow-latest-statistics?period=DAY&top=30");
        debug!("fetching v2 traffic flow statistics");
        match self.get_raw(v2_url).await {
            Ok(v2_data) => {
                if v2_data
                    .get("top_all_traffic_by_application")
                    .and_then(|a| a.as_array())
                    .is_some_and(|a| !a.is_empty())
                {
                    debug!("v2 traffic flow stats received");
                    return Ok(vec![v2_data]);
                }
                debug!("v2 response had no DPI app data, trying session");
            }
            Err(e) => {
                debug!("v2 traffic flow stats unavailable, trying session: {e}");
            }
        }

        // Session: site-level filtered endpoint.
        let url = self.site_url("stat/sitedpi");
        debug!(group_by, "fetching site DPI stats");
        let result: Vec<serde_json::Value> = self.post(url, &json!({"type": group_by})).await?;
        if !result.is_empty() && result.iter().any(|v| v.get(group_by).is_some()) {
            return Ok(result);
        }

        // Session fallback: unfiltered DPI endpoint.
        let url = self.site_url("stat/dpi");
        debug!("falling back to unfiltered DPI stats");
        self.get(url).await
    }
}
