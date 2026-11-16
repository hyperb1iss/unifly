use std::collections::HashMap;
use std::time::{Duration, Instant};

use color_eyre::eyre::Result;
use tokio::task::AbortHandle;

use super::App;
use crate::tui::action::{Action, StatsData, StatsPeriod};
use crate::tui::screen::ScreenId;

#[cfg(test)]
mod fetch_tests;

/// How long a DPI snapshot stands in for a fresh fetch. The counters are
/// totals that do not depend on the period, so stepping through periods
/// reuses one snapshot.
const DPI_REUSE: Duration = Duration::from_mins(2);

/// Application and category totals, with the reason either is missing.
#[derive(Debug, Clone, Default)]
pub(super) struct DpiSnapshot {
    apps: Vec<(String, u64)>,
    categories: Vec<(String, u64)>,
    error: Option<String>,
}

impl App {
    /// Whether a statistics fetch could only fail right now.
    pub(super) fn stats_blocked(&self) -> bool {
        !self.controller_reachable() || !self.access.session
    }

    /// Load `period` for the Stats screen. Nothing is fetched while the
    /// controller is unreachable or has no session access: the screen says
    /// why inline, and a request that can only fail would toast on a timer.
    pub(super) fn request_stats(&mut self, period: StatsPeriod) -> Result<()> {
        self.stats_period = period;
        self.last_stats_fetch = Some(Instant::now());
        // A newer request makes the one in flight pointless.
        if let Some(task) = self.monitor.stats_task.take() {
            task.abort();
        }
        if self.stats_blocked() {
            return Ok(());
        }
        self.forward_to_screen(ScreenId::Stats, &Action::StatsLoading(period))?;
        self.monitor.stats_task = self.fetch_stats(period);
        Ok(())
    }

    /// Keep the DPI totals a stats fetch read, for the next one. Totals
    /// that were themselves reused are not stamped again, or a screen
    /// that reloads inside the reuse window would never read new ones.
    pub(super) fn remember_dpi(&mut self, data: &StatsData) {
        if !data.dpi_fetched {
            return;
        }
        self.monitor.dpi = Some((
            Instant::now(),
            DpiSnapshot {
                apps: data.dpi_apps.clone(),
                categories: data.dpi_categories.clone(),
                error: data.dpi_error.clone(),
            },
        ));
    }

    /// Fetch historical stats from the controller and send `StatsUpdated`,
    /// or `StatsFailed` with the reason.
    ///
    /// Uses a generation counter so stale responses from a previous period
    /// switch are silently dropped.
    pub(super) fn fetch_stats(&self, period: StatsPeriod) -> Option<AbortHandle> {
        use std::sync::atomic::Ordering;

        let controller = self.controller.clone()?;

        let tx = self.action_tx.clone();
        let fenced = self.fenced();
        let sanitizer = self.sanitizer.clone();
        let interval = period.api_interval();
        #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
        let bucket_duration_secs = period.bucket_duration_secs() as f64;

        let generation = self.stats_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let generation_ref = self.stats_generation.clone();

        let now_ms = chrono::Utc::now().timestamp_millis();
        let start = Some(now_ms - period.duration_secs() * 1000);
        let end = Some(now_ms);
        let cached_dpi = self
            .monitor
            .dpi
            .as_ref()
            .filter(|(at, snapshot)| at.elapsed() < DPI_REUSE && snapshot.error.is_none())
            .map(|(_, snapshot)| snapshot.clone());

        let task = tokio::spawn(async move {
            let shown = |text: String| match &sanitizer {
                Some(sanitizer) => sanitizer.sanitize_text(&text),
                None => text,
            };
            let (gateway_res, site_res) = tokio::join!(
                controller.get_gateway_stats(interval, start, end, None),
                controller.get_site_stats(interval, start, end, None),
            );
            let history = gateway_res
                .and_then(|gateway| site_res.map(|site| (gateway, site)))
                .map_err(|error| error.to_string())
                .and_then(|(gateway, site)| {
                    historical_stats_data(&gateway, &site, bucket_duration_secs)
                });
            let mut data = match history {
                Ok(data) => data,
                Err(message) => {
                    if generation_ref.load(Ordering::Relaxed) == generation {
                        let _ = tx.send(fenced(Action::StatsFailed {
                            period,
                            message: shown(message),
                        }));
                    }
                    return;
                }
            };
            data.period = period;

            data.dpi_fetched = cached_dpi.is_none();
            let dpi = match cached_dpi {
                Some(dpi) => dpi,
                None => fetch_dpi(&controller).await,
            };
            data.dpi_apps = dpi.apps;
            data.dpi_categories = dpi.categories;
            data.dpi_error = dpi.error.map(shown);

            if generation_ref.load(Ordering::Relaxed) == generation {
                let _ = tx.send(fenced(Action::StatsUpdated(data)));
            }
        });
        Some(task.abort_handle())
    }
}

/// Fetch application and category traffic totals. The Integration catalog
/// carries byte counts on some controllers; where it does not, the session
/// counters do, and the catalog still names their applications.
async fn fetch_dpi(controller: &unifly_api::Controller) -> DpiSnapshot {
    let (apps_res, categories_res) = tokio::join!(
        controller.list_dpi_applications(),
        controller.list_dpi_categories(),
    );
    let mut snapshot = DpiSnapshot::default();
    let mut errors = Vec::new();

    let catalog = apps_res.unwrap_or_default();
    let mut apps: Vec<(String, u64)> = catalog
        .iter()
        .map(|app| (app.name.clone(), app.tx_bytes + app.rx_bytes))
        .filter(|(_, bytes)| *bytes > 0)
        .collect();
    if apps.is_empty() {
        let names: HashMap<u64, &str> = catalog
            .iter()
            .map(|app| (u64::from(app.id), app.name.as_str()))
            .collect();
        match controller.get_dpi_stats("by_app", None).await {
            Ok(raw) => apps = parse_session_dpi_apps(&raw, &names),
            Err(error) => errors.push(format!("applications: {error}")),
        }
    }
    apps.sort_by_key(|item| std::cmp::Reverse(item.1));
    apps.truncate(10);
    snapshot.apps = apps;

    let mut categories: Vec<(String, u64)> = categories_res
        .unwrap_or_default()
        .into_iter()
        .map(|category| (category.name, category.tx_bytes + category.rx_bytes))
        .filter(|(_, bytes)| *bytes > 0)
        .collect();
    if categories.is_empty() {
        match controller.get_dpi_stats("by_cat", None).await {
            Ok(raw) => categories = parse_session_dpi_categories(&raw),
            Err(error) => errors.push(format!("categories: {error}")),
        }
    }
    categories.sort_by_key(|item| std::cmp::Reverse(item.1));
    snapshot.categories = categories;

    if !errors.is_empty() {
        snapshot.error = Some(errors.join("; "));
    }
    snapshot
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

/// Parse Session `stat/stadpi` `by_app` response into `(name, total_bytes)`
/// tuples. `names` is the Integration catalog by application id; an
/// application it does not name is labelled by its category and number,
/// number first so a clipped label still tells two apart.
fn parse_session_dpi_apps(
    raw: &[serde_json::Value],
    names: &HashMap<u64, &str>,
) -> Vec<(String, u64)> {
    let mut apps: Vec<(String, u64)> = Vec::new();
    for entry in raw {
        let Some(by_app) = entry.get("by_app").and_then(|value| value.as_array()) else {
            continue;
        };
        for item in by_app {
            let field = |name: &str| item.get(name).and_then(serde_json::Value::as_u64);
            let category = field("cat").unwrap_or(255);
            let app_id = field("app").unwrap_or(0);
            let total = field("tx_bytes").unwrap_or(0) + field("rx_bytes").unwrap_or(0);
            if total == 0 {
                continue;
            }
            // The catalog keys an application by its category and its
            // number within it; the counters report the two apart.
            let number = app_id & 0xFFFF;
            let name = names
                .get(&((category << 16) | number))
                .or_else(|| names.get(&app_id))
                .map_or_else(
                    || format!("#{number} {}", dpi_category_name(category)),
                    |name| (*name).to_owned(),
                );
            apps.push((name, total));
        }
    }
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
    categories
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{historical_stats_data, parse_session_dpi_apps};
    use serde_json::json;

    #[test]
    fn session_dpi_apps_take_catalog_names_and_stay_distinct_without_them() {
        let raw = [json!({"by_app": [
            {"cat": 3, "app": 21, "tx_bytes": 10, "rx_bytes": 90},
            {"cat": 3, "app": 40, "tx_bytes": 5, "rx_bytes": 20},
            {"cat": 3, "app": 41, "tx_bytes": 1, "rx_bytes": 2},
            {"cat": 24, "app": 7, "tx_bytes": 0, "rx_bytes": 0},
        ]})];
        let names = HashMap::from([(0x0003_0015, "Netflix")]);

        let apps = parse_session_dpi_apps(&raw, &names);

        assert_eq!(
            apps,
            [
                ("Netflix".to_owned(), 100),
                ("#40 Streaming Media".to_owned(), 25),
                ("#41 Streaming Media".to_owned(), 3),
            ]
        );
        // Cut to the old 14-column label, the unnamed two still differ.
        let clipped: Vec<String> = apps[1..]
            .iter()
            .map(|(name, _)| name.chars().take(14).collect())
            .collect();
        assert_ne!(clipped[0], clipped[1]);
    }

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
