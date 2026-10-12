use color_eyre::eyre::Result;
use tokio_util::sync::CancellationToken;

use unifly_api::Controller;

use super::App;
use crate::tui::action::{Action, Notification};
use crate::tui::screen::ScreenId;

/// Shown instead of Settings while the demo fleet is on screen.
const DEMO_SETTINGS_NOTICE: &str = "Demo fleet: settings are disabled";

impl App {
    /// Whether this session is showing the synthetic demo fleet. Nothing in
    /// such a session may read real profiles or connect a real controller.
    pub(super) fn demo_fleet_active(&self) -> bool {
        self.controller.as_ref().is_some_and(Controller::is_demo)
    }

    pub(super) fn handle_onboarding_complete(
        &mut self,
        config: &unifly_api::ControllerConfig,
    ) -> Result<()> {
        if self.demo_fleet_active() {
            return self.refuse_in_demo_fleet();
        }
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
        // The Settings screen loads the real default profile as soon as it
        // is built, so it never opens over the demo fleet.
        if self.demo_fleet_active() {
            return self.refuse_in_demo_fleet();
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
        if self.demo_fleet_active() {
            self.screens.remove(&ScreenId::Settings);
            return self.refuse_in_demo_fleet();
        }
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

    fn refuse_in_demo_fleet(&self) -> Result<()> {
        self.action_tx
            .send(Action::Notify(Notification::info(DEMO_SETTINGS_NOTICE)))?;
        Ok(())
    }

    pub(super) fn spawn_data_bridge(&mut self, controller: Controller) {
        let cancel = self.data_cancel.clone();
        let tx = self.action_tx.clone();
        let sanitizer = self.sanitizer.clone();
        // Chain on the previous bridge so its teardown (including the
        // mid-connect disconnect cleanup) fully completes before the next
        // bridge touches shared controller state.
        let predecessor = self.bridge_handle.take();
        self.bridge_handle = Some(tokio::spawn(async move {
            if let Some(previous) = predecessor {
                let _ = previous.await;
            }
            crate::tui::data_bridge::spawn_data_bridge(controller, tx, cancel, sanitizer).await;
        }));
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
        let mut app = App::new(None, None, false, true);
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

    #[tokio::test]
    async fn settings_never_open_over_the_demo_fleet() {
        let demo = Controller::demo(unifly_api::DemoFleetOptions::default());
        let mut app = App::new(Some(demo), None, false, true);
        app.active_screen = ScreenId::Dashboard;

        app.open_settings().expect("open settings");
        assert_eq!(app.active_screen, ScreenId::Dashboard);
        assert!(!app.screens.contains_key(&ScreenId::Settings));
        let notice = app.action_rx.try_recv().expect("a notice is sent");
        assert!(matches!(notice, Action::Notify(_)));

        // Even a forged apply cannot swap a real controller in.
        app.apply_settings(&unifly_api::ControllerConfig::default())
            .expect("apply settings");
        assert!(app.demo_fleet_active(), "the demo controller must stay");
        app.handle_onboarding_complete(&unifly_api::ControllerConfig::default())
            .expect("onboarding complete");
        assert!(app.demo_fleet_active(), "the demo controller must stay");
    }
}
