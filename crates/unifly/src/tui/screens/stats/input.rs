use crossterm::event::{KeyCode, KeyEvent, MouseEvent};

use super::StatsScreen;
use crate::tui::action::{Action, StatsPeriod};
use crate::tui::widgets::click::Click;

impl StatsScreen {
    pub(super) fn handle_key_input(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Char('l') | KeyCode::Right => Some(self.step_period(1)),
            KeyCode::Char('h') | KeyCode::Left => Some(self.step_period(-1)),
            KeyCode::Char('r') => Some(Action::ReloadStats(self.period)),
            _ => None,
        }
    }

    /// A click on a period tab loads that period, like its key, and a key
    /// hint presses its key.
    pub(super) fn handle_mouse_input(&mut self, mouse: MouseEvent) -> Option<Action> {
        let target = self.hits.borrow().clicked(mouse)?;
        match target {
            // The active chip reloads, as its key does.
            Click::Tab(index) => Some(self.set_period(*StatsPeriod::ALL.get(index)?)),
            Click::Key(key) => Some(Action::PressKey(key)),
            _ => None,
        }
    }

    /// Load the period `step` places along the period bar, wrapping.
    fn step_period(&mut self, step: isize) -> Action {
        let all = StatsPeriod::ALL;
        let at = all.iter().position(|period| *period == self.period);
        let len = isize::try_from(all.len()).unwrap_or(1);
        let next = (isize::try_from(at.unwrap_or(0)).unwrap_or(0) + step).rem_euclid(len);
        self.set_period(all[usize::try_from(next).unwrap_or(0)])
    }

    /// Show `period` and ask for its data. The charts stay blank until
    /// that data arrives, and an error for the old period goes with it.
    /// Picking the period already on screen is a reload, like `r`.
    fn set_period(&mut self, period: StatsPeriod) -> Action {
        if period == self.period {
            return Action::ReloadStats(period);
        }
        self.period = period;
        self.loading = false;
        self.error = None;
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
        assert!(
            matches!(action, Some(Action::ReloadStats(StatsPeriod::OneHour))),
            "the active period reloads: {action:?}"
        );
        let action = screen.handle_key_input(crossterm::event::KeyCode::Char('r').into());
        assert!(
            matches!(action, Some(Action::ReloadStats(StatsPeriod::OneHour))),
            "{action:?}"
        );
    }
}
