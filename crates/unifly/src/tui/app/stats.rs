use super::App;
use crate::tui::action::{Action, Notification, StatsData, StatsPeriod};

#[cfg(test)]
mod fetch_tests;

impl App {
    /// Fetch historical stats from the controller and send `StatsUpdated`.
    ///
    /// Uses a generation counter so stale responses from a previous period
    /// switch are silently dropped.
    pub(super) fn fetch_stats(&self, period: StatsPeriod) {
        use std::sync::atomic::Ordering;

        let Some(controller) = self.controller.clone() else {
            return;
        };

        let tx = self.action_tx.clone();
        let interval = period.api_interval();
        #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
        let bucket_duration_secs = period.bucket_duration_secs() as f64;

        let generation = self.stats_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let generation_ref = self.stats_generation.clone();

        let now_ms = chrono::Utc::now().timestamp_millis();
        let start = Some(now_ms - period.duration_secs() * 1000);
        let end = Some(now_ms);

        tokio::spawn(async move {
            let (gateway_res, site_res, dpi_apps_res, dpi_categories_res) = tokio::join!(
                controller.get_gateway_stats(interval, start, end, None),
                controller.get_site_stats(interval, start, end, None),
                controller.list_dpi_applications(),
                controller.list_dpi_categories(),
            );

            if generation_ref.load(Ordering::Relaxed) != generation {
                return;
            }

            let mut data =
                match gateway_res.and_then(|gateway| site_res.map(|site| (gateway, site))) {
                    Ok((gateway, site)) => {
                        match historical_stats_data(&gateway, &site, bucket_duration_secs) {
                            Ok(data) => data,
                            Err(message) => {
                                let _ = tx.send(Action::Notify(Notification::error(message)));
                                return;
                            }
                        }
                    }
                    Err(error) => {
                        let _ = tx.send(Action::Notify(Notification::error(format!(
                            "Could not load statistics: {error}"
                        ))));
                        return;
                    }
                };

            if let Ok(apps) = dpi_apps_res
                && !apps.is_empty()
            {
                let mut app_list: Vec<(String, u64)> = apps
                    .into_iter()
                    .map(|app| (app.name, app.tx_bytes + app.rx_bytes))
                    .filter(|(_, bytes)| *bytes > 0)
                    .collect();
                app_list.sort_by_key(|item| std::cmp::Reverse(item.1));
                app_list.truncate(10);
                data.dpi_apps = app_list;
            }
            if data.dpi_apps.is_empty() {
                match controller.get_dpi_stats("by_app", None).await {
                    Ok(raw) => data.dpi_apps = parse_session_dpi_apps(&raw),
                    Err(error) => {
                        if generation_ref.load(Ordering::Relaxed) == generation {
                            let _ = tx.send(Action::Notify(Notification::error(format!(
                                "Could not load application traffic: {error}"
                            ))));
                        }
                    }
                }
            }

            if let Ok(categories) = dpi_categories_res
                && !categories.is_empty()
            {
                let mut category_list: Vec<(String, u64)> = categories
                    .into_iter()
                    .map(|category| (category.name, category.tx_bytes + category.rx_bytes))
                    .filter(|(_, bytes)| *bytes > 0)
                    .collect();
                category_list.sort_by_key(|item| std::cmp::Reverse(item.1));
                data.dpi_categories = category_list;
            }
            if data.dpi_categories.is_empty() {
                match controller.get_dpi_stats("by_cat", None).await {
                    Ok(raw) => data.dpi_categories = parse_session_dpi_categories(&raw),
                    Err(error) => {
                        if generation_ref.load(Ordering::Relaxed) == generation {
                            let _ = tx.send(Action::Notify(Notification::error(format!(
                                "Could not load traffic categories: {error}"
                            ))));
                        }
                    }
                }
            }

            if generation_ref.load(Ordering::Relaxed) == generation {
                let _ = tx.send(Action::StatsUpdated(data));
            }
        });
    }
}

fn historical_stats_data(
    gateway: &[serde_json::Value],
    site: &[serde_json::Value],
    legacy_bucket_secs: f64,
) -> Result<StatsData, String> {
    let mut data = StatsData::default();
    for entry in gateway {
        let timestamp = report_time_secs(entry)?;
        let duration = report_duration_secs(entry, legacy_bucket_secs)?;
        if let Some(bytes) = entry
            .get("wan-tx_bytes")
            .or_else(|| entry.get("tx_bytes"))
            .and_then(serde_json::Value::as_f64)
        {
            data.bandwidth_tx.push((timestamp, bytes / duration));
        }
        if let Some(bytes) = entry
            .get("wan-rx_bytes")
            .or_else(|| entry.get("rx_bytes"))
            .and_then(serde_json::Value::as_f64)
        {
            data.bandwidth_rx.push((timestamp, bytes / duration));
        }
    }
    for entry in site {
        let timestamp = report_time_secs(entry)?;
        if let Some(count) = entry
            .get("num_sta")
            .or_else(|| entry.get("wlan-num_sta"))
            .and_then(serde_json::Value::as_f64)
        {
            data.client_counts.push((timestamp, count));
        }
    }
    Ok(data)
}

/// Both report backends expose epoch milliseconds; charts plot seconds.
fn report_time_secs(entry: &serde_json::Value) -> Result<f64, String> {
    entry
        .get("time")
        .and_then(serde_json::Value::as_f64)
        .map(|time| time / 1000.0)
        .ok_or_else(|| "Statistics response contains an invalid timestamp".into())
}

/// Modern buckets include their actual duration, including partial current buckets.
fn report_duration_secs(entry: &serde_json::Value, legacy_bucket_secs: f64) -> Result<f64, String> {
    let Some(duration) = entry.get("duration") else {
        return Ok(legacy_bucket_secs);
    };
    duration
        .as_f64()
        .filter(|duration| *duration > 0.0)
        .map(|duration| duration / 1000.0)
        .ok_or_else(|| "Statistics response contains an invalid bucket duration".into())
}

/// Well-known UniFi DPI category IDs → human-readable names.
fn dpi_category_name(id: u64) -> &'static str {
    match id {
        0 => "Instant Messaging",
        1 => "P2P",
        2 => "File Transfer",
        3 => "Streaming Media",
        4 => "Mail & Collab",
        5 => "VoIP",
        6 => "Database",
        7 => "Games",
        8 => "Network Mgmt",
        9 => "Remote Access",
        10 => "Proxies & VPN",
        11 => "Stock Market",
        13 => "Web",
        14 => "Security Update",
        18 => "Web IM",
        20 => "Business",
        23 => "Network Proto",
        24 => "Social Network",
        255 => "Unknown",
        _ => "Other",
    }
}

/// Parse Session `stat/stadpi` `by_app` response into `(name, total_bytes)` tuples.
fn parse_session_dpi_apps(raw: &[serde_json::Value]) -> Vec<(String, u64)> {
    let mut apps: Vec<(String, u64)> = Vec::new();
    for entry in raw {
        if let Some(by_app) = entry.get("by_app").and_then(|value| value.as_array()) {
            for item in by_app {
                let cat = item
                    .get("cat")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(255);
                let app_id = item
                    .get("app")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let tx = item
                    .get("tx_bytes")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let rx = item
                    .get("rx_bytes")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let total = tx + rx;
                if total > 0 {
                    let category_name = dpi_category_name(cat);
                    let sub_id = app_id & 0xFFFF;
                    apps.push((format!("{category_name} #{sub_id}"), total));
                }
            }
        }
    }
    apps.sort_by_key(|item| std::cmp::Reverse(item.1));
    apps.truncate(10);
    apps
}

/// Parse Session `stat/stadpi` `by_cat` response into `(name, total_bytes)` tuples.
fn parse_session_dpi_categories(raw: &[serde_json::Value]) -> Vec<(String, u64)> {
    let mut categories: Vec<(String, u64)> = Vec::new();
    for entry in raw {
        if let Some(by_cat) = entry.get("by_cat").and_then(|value| value.as_array()) {
            for item in by_cat {
                let category_id = item
                    .get("cat")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(255);
                let tx = item
                    .get("tx_bytes")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let rx = item
                    .get("rx_bytes")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let total = tx + rx;
                if total > 0 {
                    categories.push((dpi_category_name(category_id).to_owned(), total));
                }
            }
        }
    }
    categories.sort_by_key(|item| std::cmp::Reverse(item.1));
    categories
}

#[cfg(test)]
mod tests {
    use super::historical_stats_data;
    use serde_json::json;

    #[test]
    fn actual_bucket_duration_controls_rates_without_scaling_partial_metrics() {
        let gateway = [
            json!({"time": 1_700_000_000_000_i64, "duration": 150_000, "wan-tx_bytes": 300_000, "wan-rx_bytes": 600_000}),
        ];
        let data = historical_stats_data(&gateway, &[], 300.0).expect("partial bucket");
        assert_eq!(data.bandwidth_tx, vec![(1_700_000_000.0, 2000.0)]);
        assert_eq!(data.bandwidth_rx, vec![(1_700_000_000.0, 4000.0)]);
    }

    #[test]
    fn legacy_bucket_duration_and_missing_measurements_are_preserved() {
        let gateway = [
            json!({"time": 1000, "tx_bytes": 600}),
            json!({"time": 2000}),
        ];
        let site = [json!({"time": 1000, "num_sta": 0}), json!({"time": 2000})];
        let data = historical_stats_data(&gateway, &site, 300.0).expect("legacy report");
        assert_eq!(data.bandwidth_tx, vec![(1.0, 2.0)]);
        assert_eq!(data.bandwidth_rx, Vec::<(f64, f64)>::new());
        assert_eq!(data.client_counts, vec![(1.0, 0.0)]);
    }

    #[test]
    fn malformed_metadata_cannot_create_infinite_rates_or_epoch_zero_points() {
        for duration in [json!(0), json!(-1), json!(null), json!("300000")] {
            assert!(
                historical_stats_data(&[json!({"time": 1000, "duration": duration})], &[], 300.0)
                    .is_err()
            );
        }
        assert!(historical_stats_data(&[json!({"tx_bytes": 1000})], &[], 300.0).is_err());
    }
}
