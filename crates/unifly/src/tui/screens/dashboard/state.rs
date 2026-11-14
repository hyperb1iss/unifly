use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::tui::action::Action;
use crate::tui::widgets::hyperchart::axis;
use unifly_api::DeviceType;

use super::{
    BANDWIDTH_SCALE_PERCENTILE, BANDWIDTH_SCALE_WINDOW_SAMPLES, BANDWIDTH_TICK_COUNT,
    BandwidthSample, DashboardScreen, LIVE_CHART_SAMPLE_INTERVAL, LIVE_CHART_WINDOW_SAMPLES,
    MIN_BANDWIDTH_SCALE,
};

/// Arrivals closer together than this are one refresh reporting twice.
const SAME_REFRESH: Duration = Duration::from_millis(500);
/// How long a rate stays live before two refreshes have set the pace.
const STALE_DEFAULT: Duration = Duration::from_secs(45);
/// A rate is live for at least this long, however fast refreshes come.
const STALE_FLOOR: Duration = Duration::from_secs(15);
/// A rate is stale after this long, however slowly refreshes come.
const STALE_CEILING: Duration = Duration::from_mins(3);

impl DashboardScreen {
    /// Format the data age as a human-readable string for the title bar.
    pub(super) fn refresh_age_str(&self) -> String {
        match self.last_data_update {
            Some(t) => {
                let secs = t.elapsed().as_secs();
                if secs < 5 {
                    "just now".into()
                } else if secs < 60 {
                    format!("{secs}s ago")
                } else {
                    format!("{}m ago", secs / 60)
                }
            }
            None => "no data".into(),
        }
    }

    /// Whether the data-age label would read differently than it was last
    /// drawn, so a quiet dashboard still counts its age up.
    fn age_label_changed(&self) -> bool {
        *self.age_drawn.borrow() != self.refresh_age_str()
    }

    /// Record a bandwidth sample into the chart data ring buffer.
    #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
    pub(super) fn push_bandwidth_sample(&mut self, tx_bps: u64, rx_bps: u64) -> bool {
        self.sample_counter += 1.0;
        self.bandwidth_tx.push((self.sample_counter, tx_bps as f64));
        self.bandwidth_rx.push((self.sample_counter, rx_bps as f64));
        let peak_changed = self.update_peaks(tx_bps, rx_bps);

        if self.bandwidth_tx.len() > LIVE_CHART_WINDOW_SAMPLES {
            self.bandwidth_tx.remove(0);
            self.bandwidth_rx.remove(0);
        }

        let visible_tx = Self::bandwidth_scale_reference(&self.bandwidth_tx);
        let visible_rx = Self::bandwidth_scale_reference(&self.bandwidth_rx);
        self.chart_tx_y_max = axis::stable_upper_bound(
            self.chart_tx_y_max,
            visible_tx,
            BANDWIDTH_TICK_COUNT,
            MIN_BANDWIDTH_SCALE,
        );
        self.chart_rx_y_max = axis::stable_upper_bound(
            self.chart_rx_y_max,
            visible_rx,
            BANDWIDTH_TICK_COUNT,
            MIN_BANDWIDTH_SCALE,
        );
        peak_changed
    }

    fn update_peaks(&mut self, tx_bps: u64, rx_bps: u64) -> bool {
        let previous_peak = self.peak_tx.max(self.peak_rx);
        self.peak_tx = self.peak_tx.max(tx_bps);
        self.peak_rx = self.peak_rx.max(rx_bps);
        self.peak_tx.max(self.peak_rx) > previous_peak
    }

    pub(super) fn bandwidth_scale_reference(series: &[(f64, f64)]) -> f64 {
        let mut values: Vec<f64> = series
            .iter()
            .rev()
            .take(BANDWIDTH_SCALE_WINDOW_SAMPLES)
            .map(|&(_, value)| value)
            .filter(|value| *value > 0.0)
            .collect();

        if values.is_empty() {
            return 0.0;
        }

        values.sort_by(f64::total_cmp);
        let percentile_index =
            ((values.len().saturating_sub(1)) * BANDWIDTH_SCALE_PERCENTILE) / 100;
        let percentile_value = values[percentile_index];
        let current_value = series.last().map_or(0.0, |&(_, value)| value);

        percentile_value.max(current_value)
    }

    pub(super) fn current_bandwidth(&self) -> Option<(u64, u64)> {
        match (self.device_bandwidth, self.health_bandwidth) {
            (Some(device), Some(health)) => {
                if health.captured_at >= device.captured_at {
                    Some((health.tx_bps, health.rx_bps))
                } else {
                    Some((device.tx_bps, device.rx_bps))
                }
            }
            (Some(device), None) => Some((device.tx_bps, device.rx_bps)),
            (None, Some(health)) => Some((health.tx_bps, health.rx_bps)),
            (None, None) => None,
        }
    }

    /// Record that a WAN rate arrived at `now`. Two sources report in the
    /// same refresh, so arrivals a moment apart count as one.
    fn note_bandwidth(&mut self, now: Instant) {
        if let Some(seen) = self.bandwidth_seen_at {
            let gap = now.duration_since(seen);
            if gap >= SAME_REFRESH {
                self.bandwidth_gap = Some(gap);
            }
        }
        self.bandwidth_seen_at = Some(now);
    }

    /// How long ago the WAN rate last arrived, once that is long enough
    /// that it no longer describes the link: three refreshes at the pace
    /// they have been arriving, or at once when the controller is gone.
    /// One failed refresh is neither: the rate it would have brought is
    /// simply late, and the age limit catches it if more follow.
    pub(super) fn bandwidth_stale_for(&self, now: Instant) -> Option<Duration> {
        let age = now.duration_since(self.bandwidth_seen_at?);
        let limit = self.bandwidth_gap.map_or(STALE_DEFAULT, |gap| {
            (gap * 3).clamp(STALE_FLOOR, STALE_CEILING)
        });
        (!self.link.reachable() || age > limit).then_some(age)
    }

    /// What the chart's title says in place of a stale rate: that the
    /// controller is gone, or how long the rate has been missing.
    pub(super) fn stale_rate_text(&self, age: Duration) -> String {
        if let Some(waiting) = self.link.waiting() {
            return waiting.to_lowercase();
        }
        match age.as_secs() {
            secs if secs < 60 => format!("no rate for {secs}s"),
            secs => format!(
                "no rate for {}",
                crate::tui::widgets::bytes_fmt::fmt_uptime(secs)
            ),
        }
    }

    /// The WAN rate right now, or `None` when the last one is stale.
    pub(super) fn live_bandwidth(&self, now: Instant) -> Option<(u64, u64)> {
        if self.bandwidth_stale_for(now).is_some() {
            return None;
        }
        self.current_bandwidth()
    }

    #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
    pub(super) fn sample_bandwidth_if_due(&mut self, now: Instant) -> Option<bool> {
        if self.last_chart_sample_at.is_some_and(|last_sample_at| {
            now.duration_since(last_sample_at) < LIVE_CHART_SAMPLE_INTERVAL
        }) {
            return None;
        }

        // A stale rate is not sampled: repeating it would draw a flat
        // line that reads as a measurement.
        let Some((tx_bps, rx_bps)) = self.live_bandwidth(now) else {
            self.chart_stalled |= !self.bandwidth_tx.is_empty();
            return None;
        };
        // What the chart held is from before the gap, so it starts over
        // rather than joining the two with a line across the outage.
        if std::mem::take(&mut self.chart_stalled) {
            self.bandwidth_tx.clear();
            self.bandwidth_rx.clear();
        }
        let peak_changed = self.push_bandwidth_sample(tx_bps, rx_bps);
        self.last_chart_sample_at = Some(now);
        Some(peak_changed)
    }

    /// Right edge of the chart's x-range at `now`: the last sample index plus
    /// the fraction of the sample interval that has elapsed since it, so the
    /// plot scrolls continuously between samples instead of stepping.
    pub(super) fn chart_x_max(&self, now: Instant) -> f64 {
        let fraction = self.last_chart_sample_at.map_or(0.0, |last| {
            now.duration_since(last).as_secs_f64() / LIVE_CHART_SAMPLE_INTERVAL.as_secs_f64()
        });
        self.sample_counter.max(0.0) + fraction.clamp(0.0, 1.0)
    }

    /// Copy of a series with its last real value held flat out to `x_max`,
    /// so the line reaches "now" without inventing a measurement.
    pub(super) fn series_held_to(series: &[(f64, f64)], x_max: f64) -> Vec<(f64, f64)> {
        let mut held = Vec::with_capacity(series.len() + 1);
        held.extend_from_slice(series);
        if let Some(&(last_x, last_value)) = series.last()
            && x_max > last_x
        {
            held.push((x_max, last_value));
        }
        held
    }

    pub(super) fn apply_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::Tick => {
                // Sample regardless of focus so the history stays continuous;
                // redraw on every tick while focused so the chart scrolls
                // smoothly between the once-a-second samples.
                let now = Instant::now();
                let sampled = self.sample_bandwidth_if_due(now);
                if self.focused {
                    if sampled == Some(true) {
                        return Some(Action::ChartPeak);
                    }
                    let live = !self.bandwidth_tx.is_empty() && !self.chart_stalled;
                    if sampled.is_some() || live || self.age_label_changed() {
                        return Some(Action::Invalidate);
                    }
                }
            }
            Action::DevicesUpdated(devices) => {
                self.devices = Arc::clone(devices);
                let now = Instant::now();
                self.last_data_update = Some(now);
                self.device_bandwidth = self
                    .devices
                    .iter()
                    .find(|d| d.device_type == DeviceType::Gateway)
                    .and_then(|gw| {
                        gw.stats
                            .uplink_bandwidth
                            .as_ref()
                            .map(|bw| BandwidthSample {
                                tx_bps: bw.tx_bytes_per_sec,
                                rx_bps: bw.rx_bytes_per_sec,
                                captured_at: now,
                            })
                    });
                if self.device_bandwidth.is_some() {
                    self.note_bandwidth(now);
                }
                if let Some((tx_bps, rx_bps)) = self.current_bandwidth()
                    && self.update_peaks(tx_bps, rx_bps)
                    && self.focused
                {
                    return Some(Action::ChartPeak);
                }
            }
            Action::ClientsUpdated(clients) => {
                self.clients = Arc::clone(clients);
            }
            Action::NetworksUpdated(networks) => {
                self.networks = Arc::clone(networks);
            }
            Action::EventReceived(event) => {
                self.events.insert(Arc::clone(event));
            }
            Action::AccessUpdated(access) => self.access = Some(*access),
            Action::AlarmsLoaded(alarms) => {
                self.active_alarms = Some(alarms.iter().filter(|alarm| !alarm.archived).count());
            }
            Action::AlarmsFailed(_) => self.active_alarms = None,
            Action::Connecting
            | Action::Connected
            | Action::Reconnecting { .. }
            | Action::Disconnected(_) => self.link.follow(action),
            Action::ControllerChanged => {
                *self = Self {
                    focused: self.focused,
                    focus: self.focus,
                    column: self.column,
                    ..Self::new()
                };
            }
            Action::HealthUpdated(health) => {
                self.health = Arc::clone(health);
                let now = Instant::now();
                self.last_data_update = Some(now);
                self.health_bandwidth = self
                    .health
                    .iter()
                    .find(|health| health.subsystem == "wan")
                    .filter(|wan| wan.tx_bytes_r.is_some() || wan.rx_bytes_r.is_some())
                    .map(|wan| BandwidthSample {
                        tx_bps: wan.tx_bytes_r.unwrap_or(0),
                        rx_bps: wan.rx_bytes_r.unwrap_or(0),
                        captured_at: now,
                    });
                if self.health_bandwidth.is_some() {
                    self.note_bandwidth(now);
                }
                if let Some((tx_bps, rx_bps)) = self.current_bandwidth()
                    && self.update_peaks(tx_bps, rx_bps)
                    && self.focused
                {
                    return Some(Action::ChartPeak);
                }
            }
            _ => {}
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_replayed_backlog_leaves_one_copy_of_each_recent_event() {
        use crate::tui::screens::events::feed::tests::event;
        use crate::tui::table::render_lines;

        let mut screen = DashboardScreen::new();
        // Twice over, and newest first, as a reconnect and a refresh
        // batch deliver them.
        for _ in 0..2 {
            for minute in (0..4).rev() {
                screen.apply_action(&Action::EventReceived(event(minute)));
            }
        }

        assert_eq!(screen.events.len(), 4);
        let rows = render_lines(140, 60, |frame, area| screen.render_dashboard(frame, area));
        let shown: Vec<_> = rows
            .iter()
            .filter(|row| row.contains(" connected"))
            .collect();
        assert_eq!(shown.len(), 4, "{shown:?}");
        assert!(shown[0].contains("client-03"), "newest first: {shown:?}");
        assert!(shown[3].contains("client-00"), "{shown:?}");
    }

    #[test]
    fn switching_controllers_drops_everything_the_old_one_reported() {
        use crate::tui::screens::events::feed::tests::event;

        let mut screen = DashboardScreen::new();
        screen.focused = true;
        screen.apply_action(&Action::EventReceived(event(1)));
        screen.push_bandwidth_sample(10_000, 20_000);
        screen.health_bandwidth = Some(BandwidthSample {
            tx_bps: 1,
            rx_bps: 2,
            captured_at: Instant::now(),
        });

        screen.apply_action(&Action::ControllerChanged);

        assert!(screen.events.is_empty());
        assert!(screen.bandwidth_tx.is_empty() && screen.health_bandwidth.is_none());
        assert_eq!(screen.peak_rx, 0);
        assert!(screen.focused, "focus is the app's, not the controller's");
    }

    #[test]
    fn current_bandwidth_prefers_most_recent_sample() {
        let mut screen = DashboardScreen::new();
        let base = Instant::now();
        screen.device_bandwidth = Some(BandwidthSample {
            tx_bps: 10,
            rx_bps: 20,
            captured_at: base,
        });
        screen.health_bandwidth = Some(BandwidthSample {
            tx_bps: 30,
            rx_bps: 40,
            captured_at: base + Duration::from_secs(1),
        });

        assert_eq!(screen.current_bandwidth(), Some((30, 40)));
    }

    #[test]
    fn bandwidth_scaling_uses_recent_visible_samples() {
        let mut screen = DashboardScreen::new();
        for idx in 1..=4 {
            screen
                .bandwidth_tx
                .push((f64::from(idx), f64::from(idx * 1000)));
            screen
                .bandwidth_rx
                .push((f64::from(idx), f64::from(idx * 500)));
        }

        let reference = DashboardScreen::bandwidth_scale_reference(&screen.bandwidth_tx);
        assert!(reference >= 4_000.0);
    }

    #[test]
    fn bandwidth_scaling_keeps_tx_and_rx_independent() {
        let mut screen = DashboardScreen::new();

        screen.push_bandwidth_sample(20_000, 2_000_000);

        assert!(screen.chart_rx_y_max > screen.chart_tx_y_max);
        assert!(screen.chart_tx_y_max < 1_000_000.0);
    }

    #[test]
    fn focused_tick_pulses_on_new_peak_after_sampling() {
        let mut screen = DashboardScreen::new();
        screen.focused = true;
        let now = Instant::now();
        screen.device_bandwidth = Some(BandwidthSample {
            tx_bps: 10_000,
            rx_bps: 40_000,
            captured_at: now,
        });

        let action = screen.apply_action(&Action::Tick);

        assert!(matches!(action, Some(Action::ChartPeak)));
        assert_eq!(screen.bandwidth_tx.len(), 1);
        assert_eq!(screen.bandwidth_rx.len(), 1);
    }

    #[test]
    fn chart_x_max_advances_between_samples() {
        let mut screen = DashboardScreen::new();
        let now = Instant::now();
        screen.device_bandwidth = Some(BandwidthSample {
            tx_bps: 1,
            rx_bps: 1,
            captured_at: now,
        });
        assert_eq!(screen.sample_bandwidth_if_due(now), Some(true));
        assert!((screen.chart_x_max(now) - 1.0).abs() < f64::EPSILON);
        let half = now + LIVE_CHART_SAMPLE_INTERVAL / 2;
        assert!((screen.chart_x_max(half) - 1.5).abs() < 0.01);
        // never runs ahead of the next sample slot
        let late = now + LIVE_CHART_SAMPLE_INTERVAL * 3;
        assert!((screen.chart_x_max(late) - 2.0).abs() < f64::EPSILON);
        // and a second sample is not taken before the interval elapses
        assert_eq!(screen.sample_bandwidth_if_due(half), None);
    }

    #[test]
    fn series_held_to_extends_last_value_without_new_points() {
        let series = [(1.0, 10.0), (2.0, 30.0)];
        let held = DashboardScreen::series_held_to(&series, 2.6);
        assert_eq!(held, vec![(1.0, 10.0), (2.0, 30.0), (2.6, 30.0)]);
        assert_eq!(
            DashboardScreen::series_held_to(&series, 2.0),
            series.to_vec()
        );
        assert_eq!(DashboardScreen::series_held_to(&[], 5.0), Vec::new());
    }

    #[test]
    fn focused_tick_invalidates_without_new_peak() {
        let mut screen = DashboardScreen::new();
        screen.focused = true;
        let now = Instant::now();
        screen.peak_tx = 100_000;
        screen.peak_rx = 100_000;
        screen.device_bandwidth = Some(BandwidthSample {
            tx_bps: 10_000,
            rx_bps: 40_000,
            captured_at: now,
        });

        let action = screen.apply_action(&Action::Tick);

        assert!(matches!(action, Some(Action::Invalidate)));
    }
}
