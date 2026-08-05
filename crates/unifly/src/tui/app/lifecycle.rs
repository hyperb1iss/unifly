use color_eyre::eyre::Result;
use tokio_util::sync::CancellationToken;

use unifly_api::Controller;

use super::App;
use crate::tui::action::{Action, Notification};
use crate::tui::screen::ScreenId;

impl App {
    pub(super) fn handle_onboarding_complete(
        &mut self,
        config: &unifly_api::ControllerConfig,
    ) -> Result<()> {
        self.screens.remove(&ScreenId::Setup);

        let controller = Controller::new(config.clone());
        self.controller = Some(controller.clone());
        self.set_active_screen(ScreenId::Dashboard);

        self.spawn_data_bridge(controller);
        self.action_tx
            .send(Action::Notify(Notification::success("Connected!")))?;

        Ok(())
    }

    pub(super) fn open_settings(&mut self) -> Result<()> {
        if self.active_screen == ScreenId::Settings || self.active_screen == ScreenId::Setup {
            return Ok(());
        }

        self.previous_screen = Some(self.active_screen);
        self.install_screen(
            ScreenId::Settings,
            crate::tui::screens::settings::SettingsScreen::new(),
        )?;
        self.set_active_screen(ScreenId::Settings);

        Ok(())
    }

    pub(super) fn close_settings(&mut self) {
        self.screens.remove(&ScreenId::Settings);
        let target = self.previous_screen.take().unwrap_or(ScreenId::Dashboard);
        self.set_active_screen(target);
    }

    pub(super) fn apply_settings(&mut self, config: &unifly_api::ControllerConfig) -> Result<()> {
        self.reset_data_bridge();

        let controller = Controller::new(config.clone());
        self.controller = Some(controller.clone());
        self.spawn_data_bridge(controller);

        self.screens.remove(&ScreenId::Settings);
        self.set_active_screen(ScreenId::Dashboard);

        self.action_tx.send(Action::Notify(Notification::success(
            "Settings saved, reconnecting\u{2026}",
        )))?;

        Ok(())
    }

    fn spawn_data_bridge(&self, controller: Controller) {
        let cancel = self.data_cancel.clone();
        let tx = self.action_tx.clone();
        let sanitizer = self.sanitizer.clone();
        tokio::spawn(async move {
            crate::tui::data_bridge::spawn_data_bridge(controller, tx, cancel, sanitizer).await;
        });
    }

    fn reset_data_bridge(&mut self) {
        self.data_cancel.cancel();
        self.data_cancel = CancellationToken::new();
    }

    /// Immediately retry connecting after a failure, replacing the old
    /// data bridge (and its pending backoff sleep) with a fresh one.
    pub(super) fn retry_connect(&mut self) {
        if self.connection_status != super::ConnectionStatus::Disconnected {
            return;
        }
        let Some(controller) = self.controller.clone() else {
            return;
        };

        self.reset_data_bridge();
        self.spawn_data_bridge(controller);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_overlay_round_trips_focus_and_history() {
        let mut app = App::new(None, None, false);
        app.active_screen = ScreenId::Dashboard;
        app.screens
            .get_mut(&ScreenId::Dashboard)
            .expect("dashboard screen should exist")
            .set_focused(true);

        app.open_settings().expect("settings should open");

        assert_eq!(app.active_screen, ScreenId::Settings);
        assert_eq!(app.previous_screen, Some(ScreenId::Dashboard));
        assert!(app.screens.contains_key(&ScreenId::Settings));
        assert!(
            app.screens
                .get(&ScreenId::Settings)
                .expect("settings screen should exist")
                .focused()
        );
        assert!(
            !app.screens
                .get(&ScreenId::Dashboard)
                .expect("dashboard screen should exist")
                .focused()
        );

        app.close_settings();

        assert_eq!(app.active_screen, ScreenId::Dashboard);
        assert_eq!(app.previous_screen, None);
        assert!(!app.screens.contains_key(&ScreenId::Settings));
        assert!(
            app.screens
                .get(&ScreenId::Dashboard)
                .expect("dashboard screen should exist")
                .focused()
        );
    }
}
