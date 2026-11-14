use super::{BANDWIDTH_TICK_COUNT, CLIENT_TICK_COUNT, MIN_BANDWIDTH_SCALE, StatsScreen};

use crate::tui::action::{Action, StatsData, StatsPeriod};
use crate::tui::screens::link::Link;
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
            shown: None,
            loading: false,
            error: None,
            dpi_apps: Vec::new(),
            dpi_categories: Vec::new(),
            dpi_error: None,
            link: Link::default(),
            access: None,
        }
    }

    pub(super) fn period_index(&self) -> usize {
        StatsPeriod::ALL
            .iter()
            .position(|period| *period == self.period)
            .unwrap_or_default()
    }

    /// Why there is nothing to load: the controller is unreachable, or it
    /// has no session access to read statistics with.
    pub(super) fn blocked(&self) -> Option<&'static str> {
        if let Some(waiting) = self.link.waiting() {
            return Some(waiting);
        }
        match self.access {
            None => Some("Connecting\u{2026}"),
            Some(access) if !access.session => {
                Some("Statistics need a direct controller session; cloud profiles cannot read them")
            }
            Some(_) => None,
        }
    }

    pub(super) fn apply_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::StatsUpdated(data) => return self.apply_stats(data),
            Action::StatsLoading(period) if *period == self.period => {
                self.loading = true;
                self.error = None;
            }
            Action::StatsFailed { period, message } if *period == self.period => {
                self.loading = false;
                self.error = Some(message.clone());
            }
            Action::AccessUpdated(access) => self.access = Some(*access),
            Action::Connecting
            | Action::Connected
            | Action::Reconnecting { .. }
            | Action::Disconnected(_) => {
                self.link.follow(action);
                // Whatever was in flight died with the link.
                self.loading &= self.link.reachable();
            }
            Action::ControllerChanged => {
                *self = Self {
                    focused: self.focused,
                    period: self.period,
                    ..Self::new()
                };
            }
            _ => {}
        }
        None
    }

    /// Take a snapshot, unless it is for a period no longer on screen.
    fn apply_stats(&mut self, data: &StatsData) -> Option<Action> {
        if data.period != self.period {
            return None;
        }
        self.loading = false;
        self.error = None;
        // A different period shares neither scale nor peak with the one
        // that was on screen, so its first snapshot sizes the charts from
        // scratch instead of easing out of the old range or pulsing.
        if self.shown.replace(data.period) != Some(data.period) {
            self.bandwidth_tx_y_max = 0.0;
            self.bandwidth_rx_y_max = 0.0;
            self.bandwidth_peak = 0.0;
            self.client_y_max = 0.0;
        }

        self.bandwidth_tx.clone_from(&data.bandwidth_tx);
        self.bandwidth_rx.clone_from(&data.bandwidth_rx);
        self.client_counts.clone_from(&data.client_counts);
        self.dpi_apps.clone_from(&data.dpi_apps);
        self.dpi_categories.clone_from(&data.dpi_categories);
        self.dpi_error.clone_from(&data.dpi_error);

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
            ..crate::tui::action::StatsData::default()
        }
    }

    fn stats_for(
        period: StatsPeriod,
        bandwidth: f64,
        clients: f64,
    ) -> crate::tui::action::StatsData {
        crate::tui::action::StatsData {
            period,
            ..sample_stats_data(bandwidth, clients)
        }
    }

    fn connected() -> StatsScreen {
        let mut screen = StatsScreen::new();
        screen.apply_action(&Action::Connected);
        screen.apply_action(&Action::AccessUpdated(crate::tui::action::Access {
            integration: true,
            session: true,
            live_events: true,
        }));
        screen
    }

    fn draw(screen: &StatsScreen) -> String {
        crate::tui::table::render_lines(100, 30, |frame, area| screen.render_screen(frame, area))
            .join("\n")
    }

    #[test]
    fn one_failed_refresh_keeps_the_loaded_charts_on_screen() {
        let mut screen = connected();
        screen.apply_action(&Action::StatsUpdated(sample_stats_data(4_000_000.0, 12.0)));
        screen.apply_action(&Action::StatsLoading(StatsPeriod::default()));

        screen.apply_action(&Action::Reconnecting { attempt: 1 });

        let text = draw(&screen);
        assert!(text.contains("WAN Bandwidth"), "{text}");
        assert!(!text.contains("Connecting"), "{text}");
        assert!(screen.loading, "the fetch in flight is still expected");
        assert_eq!(screen.blocked(), None);

        // A lost controller is different: the charts give way to why.
        screen.apply_action(&Action::Disconnected("gone".into()));
        assert_eq!(screen.blocked(), Some("Not connected"));
        assert!(!screen.loading);
    }

    #[test]
    fn a_new_period_never_shows_the_old_periods_charts() {
        use crossterm::event::{KeyCode, KeyEvent};

        let mut screen = connected();
        screen.apply_action(&Action::StatsUpdated(sample_stats_data(4_000_000.0, 12.0)));
        assert!(draw(&screen).contains("WAN Bandwidth"));

        let request = screen.handle_key_input(KeyEvent::from(KeyCode::Char('h')));
        assert!(matches!(
            request,
            Some(Action::RequestStats(StatsPeriod::ThirtyDays))
        ));
        screen.apply_action(&Action::StatsLoading(StatsPeriod::ThirtyDays));
        let text = draw(&screen);
        assert!(text.contains("Loading 30d"), "{text}");
        assert!(!text.contains("WAN Bandwidth"), "stale charts: {text}");

        // A late answer for the period that was left is dropped.
        screen.apply_action(&Action::StatsUpdated(sample_stats_data(9_000_000.0, 99.0)));
        assert!(!draw(&screen).contains("WAN Bandwidth"));
        screen.apply_action(&Action::StatsFailed {
            period: StatsPeriod::OneHour,
            message: "too late to matter".into(),
        });
        assert!(!draw(&screen).contains("too late"));

        screen.apply_action(&Action::StatsUpdated(stats_for(
            StatsPeriod::ThirtyDays,
            1_000_000.0,
            40.0,
        )));
        let text = draw(&screen);
        assert!(
            text.contains("WAN Bandwidth") && !text.contains("Loading"),
            "{text}"
        );
    }

    #[test]
    fn a_failed_load_says_why_in_the_screen() {
        let mut screen = connected();
        screen.apply_action(&Action::StatsLoading(StatsPeriod::OneHour));
        screen.apply_action(&Action::StatsFailed {
            period: StatsPeriod::OneHour,
            message: "the controller timed out".into(),
        });

        let text = draw(&screen);
        assert!(
            text.contains("Could not load 1h: the controller timed out"),
            "{text}"
        );
        assert!(!text.contains("No bandwidth data yet"), "{text}");

        // With charts already up, a failed reload keeps them and says so.
        screen.apply_action(&Action::StatsUpdated(sample_stats_data(4_000_000.0, 12.0)));
        screen.apply_action(&Action::StatsLoading(StatsPeriod::OneHour));
        assert!(draw(&screen).contains("reloading"));
        screen.apply_action(&Action::StatsFailed {
            period: StatsPeriod::OneHour,
            message: "the controller timed out".into(),
        });
        let text = draw(&screen);
        assert!(text.contains("WAN Bandwidth"), "{text}");
        assert!(text.contains("reload failed, showing older data"), "{text}");
    }

    #[test]
    fn statistics_explain_a_missing_session_instead_of_loading() {
        let mut screen = StatsScreen::new();
        assert!(draw(&screen).contains("Connecting"));

        screen.apply_action(&Action::Connected);
        screen.apply_action(&Action::AccessUpdated(crate::tui::action::Access {
            integration: true,
            session: false,
            live_events: false,
        }));
        let text = draw(&screen);
        assert!(text.contains("need a direct controller session"), "{text}");
        assert!(!text.contains("Loading"), "{text}");

        screen.apply_action(&Action::Disconnected("gone".into()));
        assert!(draw(&screen).contains("Not connected"));
    }

    #[test]
    fn switching_controllers_drops_the_charts_but_keeps_the_period() {
        let mut screen = connected();
        screen.apply_action(&Action::StatsUpdated(sample_stats_data(4_000_000.0, 12.0)));

        screen.apply_action(&Action::ControllerChanged);

        assert!(screen.shown.is_none() && screen.bandwidth_tx.is_empty());
        assert!(screen.dpi_apps.is_empty() && screen.access.is_none());
    }

    #[test]
    fn application_names_are_never_cut_into_lookalikes() {
        let mut screen = connected();
        let mut data = sample_stats_data(4_000_000.0, 12.0);
        data.dpi_apps = vec![
            ("#40 Streaming Media".into(), 9_132_200_000_000),
            ("#41 Streaming Media".into(), 6_000_000_000),
        ];
        screen.apply_action(&Action::StatsUpdated(data));

        let text = draw(&screen);
        assert!(text.contains("#40 Streaming Media"), "{text}");
        assert!(text.contains("#41 Streaming Media"), "{text}");
        assert!(
            text.contains("9.1T"),
            "the largest value keeps its unit: {text}"
        );
        assert!(text.contains("all time"), "{text}");
    }

    #[test]
    fn an_unreadable_dpi_source_says_so_in_its_panel() {
        let mut screen = connected();
        let mut data = sample_stats_data(4_000_000.0, 12.0);
        data.dpi_apps.clear();
        data.dpi_error = Some("applications: forbidden".into());
        screen.apply_action(&Action::StatsUpdated(data));

        assert!(draw(&screen).contains("Unavailable: applications: forbidden"));
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
        screen.apply_action(&Action::StatsUpdated(sample_stats_data(100_000.0, 8.0)));

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
    fn a_new_period_starts_its_charts_on_a_fresh_scale() {
        use crossterm::event::{KeyCode, KeyEvent};

        let mut screen = StatsScreen::new();
        screen.focused = true;
        screen.apply_action(&Action::StatsUpdated(sample_stats_data(4_000_000.0, 650.0)));

        screen.handle_key_input(KeyEvent::from(KeyCode::Char('l')));
        let request = screen.handle_key_input(KeyEvent::from(KeyCode::Right));
        assert!(matches!(
            request,
            Some(Action::RequestStats(StatsPeriod::SevenDays))
        ));
        // The periods wrap in both directions.
        screen.handle_key_input(KeyEvent::from(KeyCode::Char('l')));
        let wrapped = screen.handle_key_input(KeyEvent::from(KeyCode::Char('l')));
        assert!(matches!(
            wrapped,
            Some(Action::RequestStats(StatsPeriod::OneHour))
        ));
        let back = screen.handle_key_input(KeyEvent::from(KeyCode::Char('h')));
        assert!(matches!(
            back,
            Some(Action::RequestStats(StatsPeriod::ThirtyDays))
        ));
        screen.handle_key_input(KeyEvent::from(KeyCode::Left));

        let carried = axis::stable_upper_bound(screen.client_y_max, 420.0, CLIENT_TICK_COUNT, 1.0);
        assert_eq!(screen.period, StatsPeriod::SevenDays);
        let pulse = screen.apply_action(&Action::StatsUpdated(stats_for(
            StatsPeriod::SevenDays,
            8_000_000.0,
            420.0,
        )));

        assert!(pulse.is_none(), "the first snapshot of a period is no peak");
        let fresh = axis::stable_upper_bound(0.0, 420.0, CLIENT_TICK_COUNT, 1.0);
        assert_ne!(fresh, carried, "the fixture must tell the two scales apart");
        assert_eq!(screen.client_y_max, fresh);
        assert_eq!(
            screen.bandwidth_tx_y_max,
            axis::stable_upper_bound(0.0, 8_000_000.0, BANDWIDTH_TICK_COUNT, MIN_BANDWIDTH_SCALE)
        );
    }

    #[test]
    fn refreshing_the_same_period_keeps_its_scale() {
        use crossterm::event::{KeyCode, KeyEvent};

        let mut screen = StatsScreen::new();
        screen.focused = true;
        screen.apply_action(&Action::StatsUpdated(sample_stats_data(4_000_000.0, 120.0)));
        let carried = axis::stable_upper_bound(screen.client_y_max, 100.0, CLIENT_TICK_COUNT, 1.0);

        screen.handle_key_input(KeyEvent::from(KeyCode::Char('r')));
        let pulse =
            screen.apply_action(&Action::StatsUpdated(sample_stats_data(8_000_000.0, 100.0)));

        assert!(matches!(pulse, Some(Action::ChartPeak)));
        assert_eq!(screen.client_y_max, carried);
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
