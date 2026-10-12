use super::App;
use crate::tui::action::{Action, StatsData, StatsPeriod};

impl App {
    /// Fetch historical stats from the controller and send `StatsUpdated`.
    ///
    /// Uses a generation counter so stale responses from a previous period
    /// switch are silently dropped.
    #[allow(clippy::too_many_lines)]
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

        #[allow(
            clippy::cast_possible_wrap,
            clippy::cast_possible_truncation,
            clippy::as_conversions
        )]
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
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

            let mut data = StatsData::default();

            if let Ok(gateway_stats) = gateway_res {
                for entry in &gateway_stats {
                    let ts = report_time_secs(entry);
                    if let Some(tx_bytes) = entry
                        .get("wan-tx_bytes")
                        .or_else(|| entry.get("tx_bytes"))
                        .and_then(serde_json::Value::as_f64)
                    {
                        data.bandwidth_tx
                            .push((ts, tx_bytes / bucket_duration_secs));
                    }
                    if let Some(rx_bytes) = entry
                        .get("wan-rx_bytes")
                        .or_else(|| entry.get("rx_bytes"))
                        .and_then(serde_json::Value::as_f64)
                    {
                        data.bandwidth_rx
                            .push((ts, rx_bytes / bucket_duration_secs));
                    }
                }
            }

            if let Ok(site_stats) = site_res {
                for entry in &site_stats {
                    let ts = report_time_secs(entry);
                    if let Some(count) = entry
                        .get("num_sta")
                        .or_else(|| entry.get("wlan-num_sta"))
                        .and_then(serde_json::Value::as_f64)
                    {
                        data.client_counts.push((ts, count));
                    }
                }
            }

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
            if data.dpi_apps.is_empty()
                && let Ok(raw) = controller.get_dpi_stats("by_app", None).await
            {
                data.dpi_apps = parse_session_dpi_apps(&raw);
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
            if data.dpi_categories.is_empty()
                && let Ok(raw) = controller.get_dpi_stats("by_cat", None).await
            {
                data.dpi_categories = parse_session_dpi_categories(&raw);
            }

            let _ = tx.send(Action::StatsUpdated(data));
        });
    }
}

/// Bucket start of a `stat/report` row in epoch seconds. The controller
/// reports `time` in milliseconds; the charts plot seconds.
fn report_time_secs(entry: &serde_json::Value) -> f64 {
    let time = entry
        .get("time")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    if time > 1e11 { time / 1000.0 } else { time }
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
