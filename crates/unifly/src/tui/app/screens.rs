use color_eyre::eyre::Result;
use tracing::debug;

use super::App;
use crate::tui::action::{Action, StatsPeriod};
use crate::tui::component::Component;
use crate::tui::screen::ScreenId;

impl App {
    pub(super) fn install_screen<C>(&mut self, screen_id: ScreenId, mut screen: C) -> Result<()>
    where
        C: Component + 'static,
    {
        screen.init(self.action_tx.clone())?;
        self.screens.insert(screen_id, Box::new(screen));
        Ok(())
    }

    pub(super) fn switch_screen(&mut self, target: ScreenId) -> Result<()> {
        if target == self.active_screen {
            return Ok(());
        }

        self.previous_screen = Some(self.active_screen);
        self.set_active_screen(target);

        if self.effects_enabled {
            self.effects.start_screen_transition();
        }

        if target == ScreenId::Stats {
            self.action_tx
                .send(Action::RequestStats(StatsPeriod::default()))?;
        }
        if target == ScreenId::Wifi {
            self.action_tx.send(Action::RequestWifiChannels)?;
        }

        Ok(())
    }

    pub(super) fn set_active_screen(&mut self, target: ScreenId) {
        if target == self.active_screen {
            return;
        }

        debug!("switching screen: {} → {}", self.active_screen, target);
        self.set_screen_focus(self.active_screen, false);
        self.active_screen = target;
        self.set_screen_focus(target, true);
    }

    pub(super) fn forward_to_all_screens(&mut self, action: &Action) -> Result<()> {
        for screen in self.screens.values_mut() {
            if let Some(follow_up) = screen.update(action)? {
                self.action_tx.send(follow_up)?;
            }
        }

        Ok(())
    }

    pub(super) fn forward_to_screen(&mut self, screen_id: ScreenId, action: &Action) -> Result<()> {
        if let Some(screen) = self.screens.get_mut(&screen_id)
            && let Some(follow_up) = screen.update(action)?
        {
            self.action_tx.send(follow_up)?;
        }

        Ok(())
    }

    fn set_screen_focus(&mut self, screen_id: ScreenId, focused: bool) {
        if let Some(screen) = self.screens.get_mut(&screen_id) {
            screen.set_focused(focused);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_to_stats_requests_default_period() {
        let mut app = App::new(None, None, false, true);
        app.active_screen = ScreenId::Dashboard;

        app.switch_screen(ScreenId::Stats)
            .expect("screen switch should succeed");

        assert_eq!(app.previous_screen, Some(ScreenId::Dashboard));
        assert_eq!(app.active_screen, ScreenId::Stats);

        let queued = app
            .action_rx
            .try_recv()
            .expect("stats request should be queued");
        assert!(matches!(queued, Action::RequestStats(StatsPeriod::OneHour)));
    }

    #[test]
    fn set_active_screen_handles_missing_current_screen() {
        let mut app = App::new(None, None, false, true);
        app.screens.remove(&ScreenId::Setup);

        app.set_active_screen(ScreenId::Dashboard);

        assert_eq!(app.active_screen, ScreenId::Dashboard);
        assert!(
            app.screens
                .get(&ScreenId::Dashboard)
                .expect("dashboard screen should exist")
                .focused()
        );
    }
}
