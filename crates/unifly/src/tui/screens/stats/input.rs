use crossterm::event::{KeyCode, KeyEvent, MouseEvent};

use super::StatsScreen;
use crate::tui::action::{Action, StatsPeriod};

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

    /// A click on a period tab loads that period, like its key.
    pub(super) fn handle_mouse_input(&mut self, mouse: MouseEvent) -> Option<Action> {
        let index = self.period_targets.borrow().clicked(mouse)?;
        let period = *StatsPeriod::ALL.get(index)?;
        (period != self.period).then(|| self.set_period(period))
    }

    fn set_period(&mut self, period: StatsPeriod) -> Action {
        self.period = period;
        Action::RequestStats(period)
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::action::{Action, StatsPeriod};
    use crate::tui::component::Component;
    use crate::tui::screens::stats::StatsScreen;
    use crate::tui::table::render_lines;
    use crate::tui::widgets::tab_strip::click_label;

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
