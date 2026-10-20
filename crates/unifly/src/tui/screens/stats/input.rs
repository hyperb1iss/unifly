use crossterm::event::{KeyCode, KeyEvent, MouseEvent};

use super::StatsScreen;
use crate::tui::action::{Action, StatsPeriod};
use crate::tui::widgets::click::Click;

impl StatsScreen {
    pub(super) fn handle_key_input(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Char('h') => Some(self.set_period(StatsPeriod::OneHour)),
            KeyCode::Char('d') => Some(self.set_period(StatsPeriod::TwentyFourHours)),
            KeyCode::Char('w') => Some(self.set_period(StatsPeriod::SevenDays)),
            KeyCode::Char('m') => Some(self.set_period(StatsPeriod::ThirtyDays)),
            KeyCode::Char('r') => Some(Action::RequestStats(self.period)),
            _ => None,
        }
    }

    /// A click on a period tab loads that period, like its key, and a key
    /// hint presses its key.
    pub(super) fn handle_mouse_input(&mut self, mouse: MouseEvent) -> Option<Action> {
        let target = self.hits.borrow().clicked(mouse)?;
        match target {
            Click::Tab(index) => {
                let period = *StatsPeriod::ALL.get(index)?;
                (period != self.period).then(|| self.set_period(period))
            }
            Click::Key(key) => Some(Action::PressKey(key)),
            _ => None,
        }
    }

    /// Load `period`. A different period shares neither scale nor peak
    /// with the one on screen, so its first snapshot sizes the charts from
    /// scratch instead of easing out of the old range or pulsing as a peak.
    fn set_period(&mut self, period: StatsPeriod) -> Action {
        if period != self.period {
            self.period = period;
            self.fresh_scale = true;
        }
        Action::RequestStats(period)
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::action::{Action, StatsPeriod};
    use crate::tui::component::Component;
    use crate::tui::screens::stats::StatsScreen;
    use crate::tui::table::render_lines;
    use crate::tui::widgets::click::click_label;

    #[test]
    fn clicking_a_period_loads_it() {
        let mut screen = StatsScreen::new();
        for (label, period) in [
            ("24h", StatsPeriod::TwentyFourHours),
            ("7d", StatsPeriod::SevenDays),
            ("30d", StatsPeriod::ThirtyDays),
            ("1h", StatsPeriod::OneHour),
        ] {
            let rows = render_lines(100, 30, |frame, area| screen.render(frame, area));
            let action = screen.handle_mouse_input(click_label(&rows, "30d", label));
            assert!(
                matches!(action, Some(Action::RequestStats(clicked)) if clicked == period),
                "{label}: {action:?}"
            );
            assert_eq!(screen.period, period);
        }

        let rows = render_lines(100, 30, |frame, area| screen.render(frame, area));
        let action = screen.handle_mouse_input(click_label(&rows, "30d", "1h"));
        assert!(action.is_none(), "the active period is already loaded");
    }
}
