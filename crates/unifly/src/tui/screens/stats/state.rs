use super::{BANDWIDTH_TICK_COUNT, CLIENT_TICK_COUNT, MIN_BANDWIDTH_SCALE, StatsScreen};

use crate::tui::action::{Action, StatsPeriod};
use crate::tui::widgets::hyperchart::axis;

impl StatsScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            period: StatsPeriod::default(),
            hits: std::cell::RefCell::default(),
            bandwidth_tx: Vec::new(),
            bandwidth_rx: Vec::new(),
            bandwidth_tx_y_max: 0.0,
            bandwidth_rx_y_max: 0.0,
            client_counts: Vec::new(),
            client_y_max: 0.0,
            bandwidth_peak: 0.0,
            dpi_apps: Vec::new(),
            dpi_categories: Vec::new(),
        }
    }

    pub(super) fn period_index(&self) -> usize {
        StatsPeriod::ALL
            .iter()
            .position(|period| *period == self.period)
            .unwrap_or_default()
    }

    pub(super) fn apply_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::SetStatsPeriod(period) => {
                self.period = *period;
                self.bandwidth_tx_y_max = 0.0;
                self.bandwidth_rx_y_max = 0.0;
                self.bandwidth_peak = 0.0;
                self.client_y_max = 0.0;
            }
            Action::StatsUpdated(data) => {
                self.bandwidth_tx.clone_from(&data.bandwidth_tx);
                self.bandwidth_rx.clone_from(&data.bandwidth_rx);
                self.client_counts.clone_from(&data.client_counts);
                self.dpi_apps.clone_from(&data.dpi_apps);
                self.dpi_categories.clone_from(&data.dpi_categories);

                let tx_max = self
                    .bandwidth_tx
                    .iter()
                    .map(|&(_, value)| value)
                    .fold(0.0_f64, f64::max);
                let rx_max = self
                    .bandwidth_rx
                    .iter()
                    .map(|&(_, value)| value)
                    .fold(0.0_f64, f64::max);
                let previous_peak = self.bandwidth_peak;
                self.bandwidth_peak = tx_max.max(rx_max);
                self.bandwidth_tx_y_max = axis::stable_upper_bound(
                    self.bandwidth_tx_y_max,
                    tx_max,
                    BANDWIDTH_TICK_COUNT,
                    MIN_BANDWIDTH_SCALE,
                );
                self.bandwidth_rx_y_max = axis::stable_upper_bound(
                    self.bandwidth_rx_y_max,
                    rx_max,
                    BANDWIDTH_TICK_COUNT,
                    MIN_BANDWIDTH_SCALE,
                );

                let client_max = self
                    .client_counts
                    .iter()
                    .map(|&(_, value)| value)
                    .fold(0.0_f64, f64::max);
                self.client_y_max =
                    axis::stable_upper_bound(self.client_y_max, client_max, CLIENT_TICK_COUNT, 1.0);

                if self.focused && previous_peak > 0.0 && self.bandwidth_peak > previous_peak {
                    return Some(Action::ChartPeak);
                }
            }
            _ => {}
        }

        None
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn sample_stats_data(bandwidth: f64, clients: f64) -> crate::tui::action::StatsData {
        crate::tui::action::StatsData {
            bandwidth_tx: vec![(1.0, bandwidth)],
            bandwidth_rx: vec![(1.0, bandwidth / 2.0)],
            client_counts: vec![(1.0, clients)],
            dpi_apps: vec![("Video".into(), 1024)],
            dpi_categories: vec![("Streaming".into(), 2048)],
        }
    }

    #[test]
    fn set_stats_period_resets_axis_bounds() {
        let mut screen = StatsScreen::new();
        screen.bandwidth_tx_y_max = 42_000.0;
        screen.bandwidth_rx_y_max = 84_000.0;
        screen.bandwidth_peak = 84_000.0;
        screen.client_y_max = 99.0;

        screen.apply_action(&Action::SetStatsPeriod(StatsPeriod::SevenDays));

        assert_eq!(screen.period, StatsPeriod::SevenDays);
        assert_eq!(screen.bandwidth_tx_y_max, 0.0);
        assert_eq!(screen.bandwidth_rx_y_max, 0.0);
        assert_eq!(screen.bandwidth_peak, 0.0);
        assert_eq!(screen.client_y_max, 0.0);
    }

    #[test]
    fn stats_updates_use_stable_axis_bounds() {
        let mut screen = StatsScreen::new();

        screen.apply_action(&Action::StatsUpdated(sample_stats_data(120_000.0, 18.0)));
        let initial_upload_bound = screen.bandwidth_tx_y_max;
        let initial_download_ceiling = screen.bandwidth_rx_y_max;
        let first_client_max = screen.client_y_max;

        assert_eq!(
            initial_upload_bound,
            axis::stable_upper_bound(0.0, 120_000.0, BANDWIDTH_TICK_COUNT, MIN_BANDWIDTH_SCALE)
        );
        assert_eq!(
            initial_download_ceiling,
            axis::stable_upper_bound(0.0, 60_000.0, BANDWIDTH_TICK_COUNT, MIN_BANDWIDTH_SCALE)
        );
        assert_eq!(
            first_client_max,
            axis::stable_upper_bound(0.0, 18.0, CLIENT_TICK_COUNT, 1.0)
        );

        screen.apply_action(&Action::StatsUpdated(sample_stats_data(40_000.0, 8.0)));

        assert_eq!(
            screen.bandwidth_tx_y_max,
            axis::stable_upper_bound(
                initial_upload_bound,
                40_000.0,
                BANDWIDTH_TICK_COUNT,
                MIN_BANDWIDTH_SCALE
            )
        );
        assert_eq!(
            screen.bandwidth_rx_y_max,
            axis::stable_upper_bound(
                initial_download_ceiling,
                20_000.0,
                BANDWIDTH_TICK_COUNT,
                MIN_BANDWIDTH_SCALE
            )
        );
        assert_eq!(
            screen.client_y_max,
            axis::stable_upper_bound(first_client_max, 8.0, CLIENT_TICK_COUNT, 1.0)
        );
    }

    #[test]
    fn focused_stats_update_pulses_on_higher_peak() {
        let mut screen = StatsScreen::new();
        screen.focused = true;
        screen.bandwidth_peak = 100_000.0;

        let action = screen.apply_action(&Action::StatsUpdated(sample_stats_data(200_000.0, 8.0)));

        assert!(matches!(action, Some(Action::ChartPeak)));
    }

    #[test]
    fn focused_stats_update_skips_pulse_on_first_snapshot() {
        let mut screen = StatsScreen::new();
        screen.focused = true;

        let action = screen.apply_action(&Action::StatsUpdated(sample_stats_data(200_000.0, 8.0)));

        assert!(action.is_none());
    }

    #[test]
    fn stats_bandwidth_bounds_do_not_share_rx_spikes() {
        let mut screen = StatsScreen::new();
        let mut data = sample_stats_data(20_000.0, 4.0);
        data.bandwidth_rx = vec![(1.0, 2_000_000.0)];

        screen.apply_action(&Action::StatsUpdated(data));

        assert!(screen.bandwidth_rx_y_max > screen.bandwidth_tx_y_max);
        assert!(screen.bandwidth_tx_y_max < 1_000_000.0);
    }
}
