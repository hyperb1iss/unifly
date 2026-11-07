use serde_json::json;
use tracing::debug;

use crate::{Error, SessionClient};

impl SessionClient {
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
