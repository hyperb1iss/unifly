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

    pub(super) fn open_settings(&mut self) -> Result<()> {
        self.open_settings_with(crate::tui::screens::settings::SettingsScreen::new)
    }

    pub(super) fn open_settings_with(
        &mut self,
        build: impl FnOnce() -> crate::tui::screens::settings::SettingsScreen,
    ) -> Result<()> {
        if self.active_screen == ScreenId::Settings || self.active_screen == ScreenId::Setup {
            return Ok(());
        }
        // The Settings screen loads the real default profile as soon as it
        // is built, so it never opens over the demo fleet.
        if self.demo_fleet_active() {
            return self.refuse_in_demo_fleet();
        }

        let mut screen = build();
        if self.sanitizer.is_some() {
            // The session is redacting what it shows, and Settings holds
            // the controller's real address, user, and site.
            screen.hide_values();
        } else if self.connection_status == super::ConnectionStatus::Connected
            && let Some(controller) = &self.controller
        {
            screen.set_known_sites(
                controller
                    .sites_snapshot()
                    .iter()
                    .map(|site| unifly_api::SiteHint {
                        internal_reference: site.internal_name.clone(),
                        display_name: site.name.clone(),
                    })
                    .collect(),
            );
        }

        // Toasts are drawn over Settings, and a standing one (the failed
        // connection that sent the user here) would sit on top of the form
        // it is about. It stays in the notification history.
        self.notices.dismiss_sticky();
        self.previous_screen = Some(self.active_screen);
        self.install_screen(ScreenId::Settings, screen)?;
        self.set_active_screen(ScreenId::Settings);

        Ok(())
    }

    /// Close Settings. When a warning or error toast is standing over it,
    /// that goes first, as Esc dismisses it on every other screen.
    pub(super) fn close_settings(&mut self) {
        if self.active_screen == ScreenId::Settings && self.notices.has_sticky() {
            self.notices.dismiss_sticky();
            return;
        }
        self.screens.remove(&ScreenId::Settings);
        let target = self.previous_screen.take().unwrap_or(ScreenId::Dashboard);
        self.set_active_screen(target);
    }

    /// Open the setup wizard for a new profile, from a running session.
    pub(super) fn open_setup_for(&mut self, profile_name: &str) -> Result<()> {
        if self.demo_fleet_active() {
            return self.refuse_in_demo_fleet();
        }
        self.screens.remove(&ScreenId::Settings);
        self.install_screen(
            ScreenId::Setup,
            crate::tui::screens::onboarding::OnboardingScreen::for_new_profile(profile_name),
        )?;
        self.set_active_screen(ScreenId::Setup);
        Ok(())
    }

    /// Leave a setup wizard that was opened from a running session.
    pub(super) fn close_setup(&mut self) {
        if self.controller.is_none() {
            // Nothing to go back to: the wizard is the session.
            return;
        }
        self.screens.remove(&ScreenId::Setup);
        let target = self.previous_screen.take().unwrap_or(ScreenId::Dashboard);
        self.set_active_screen(target);
    }

    /// Connect the session with `config`, which a form tested and saved
    /// as `profile_name`. Everything from the previous controller goes.
    /// `keep_flags` says whether the launch flags still apply to it.
    pub(super) fn apply_connection(
        &mut self,
        profile_name: &str,
        config: &unifly_api::ControllerConfig,
        notices: &[String],
        keep_flags: bool,
    ) -> Result<()> {
        if self.demo_fleet_active() {
            self.screens.remove(&ScreenId::Settings);
            return self.refuse_in_demo_fleet();
        }
        self.reset_data_bridge();
        self.action_tx.send(Action::ControllerChanged)?;
        self.connection_error = None;

        let controller = Controller::new(config.clone());
        self.controller = Some(controller.clone());
        self.spawn_data_bridge(controller);
        crate::tui::forms::session::switch_to(profile_name, keep_flags);

        self.screens.remove(&ScreenId::Settings);
        self.screens.remove(&ScreenId::Setup);
        // Esc on the dashboard must not walk back to where Settings was
        // opened from.
        self.previous_screen = None;
        self.set_active_screen(ScreenId::Dashboard);

        // A secret that fell back to plaintext must not slip past: it
        // stays on screen until dismissed.
        for notice in notices {
            self.action_tx
                .send(Action::Notify(Notification::warning(notice.clone())))?;
        }
        self.action_tx
            .send(Action::Notify(Notification::success(format!(
                "Saved profile {}, connecting\u{2026}",
                self.shown_text(profile_name)
            ))))?;

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

        app.open_settings_with(crate::tui::screens::settings::SettingsScreen::blank)
            .expect("settings should open");

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

    /// Settings takes every key, so a standing toast over it has no other
    /// way out: opening Settings clears the ones already up, and Esc
    /// dismisses one that arrives later before it closes the screen.
    #[test]
    fn a_standing_toast_never_traps_the_settings_form() {
        let mut app = App::new(None, None, false, true);
        app.active_screen = ScreenId::Dashboard;
        app.show_notification(Notification::error("connection failed: refused"));
        assert!(app.notices.has_sticky());

        app.open_settings_with(crate::tui::screens::settings::SettingsScreen::blank)
            .expect("settings should open");
        assert!(!app.notices.has_sticky(), "the form is not covered");

        app.show_notification(Notification::warning("certificate expires soon"));
        app.close_settings();
        assert_eq!(app.active_screen, ScreenId::Settings, "Esc took the toast");
        assert!(!app.notices.has_sticky());

        app.close_settings();
        assert_eq!(app.active_screen, ScreenId::Dashboard);
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

        // Even a forged apply cannot swap a real controller in, and no
        // wizard opens to make one.
        app.apply_connection("home", &unifly_api::ControllerConfig::default(), &[], true)
            .expect("apply connection");
        assert!(app.demo_fleet_active(), "the demo controller must stay");
        app.open_setup_for("lab").expect("open setup");
        assert!(!app.screens.contains_key(&ScreenId::Setup));
    }
}
