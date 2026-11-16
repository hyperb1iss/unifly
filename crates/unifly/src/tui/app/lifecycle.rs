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
        let mut wizard =
            crate::tui::screens::onboarding::OnboardingScreen::for_new_profile(profile_name);
        if self.sanitizer.is_some() {
            // As with Settings: a redacting session shows no real values.
            wizard.hide_values();
        }
        self.install_screen(ScreenId::Setup, wizard)?;
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
    /// `flags` are the launch flags it was tested under.
    pub(super) fn apply_connection(
        &mut self,
        profile_name: &str,
        config: &unifly_api::ControllerConfig,
        notices: &[String],
        flags: &crate::config::resolve::ConnectionOverrides,
    ) -> Result<()> {
        if self.demo_fleet_active() {
            self.screens.remove(&ScreenId::Settings);
            return self.refuse_in_demo_fleet();
        }
        self.reset_data_bridge();
        self.action_tx.send(Action::ControllerChanged)?;
        // The status bar speaks for the controller being connected to, not
        // the one just left, from this moment and not from whenever the
        // bridge first reports. Nothing is being located on it either.
        self.connection_status = super::ConnectionStatus::Connecting;
        self.connection_error = None;
        self.locating.clear();

        let controller = Controller::new(config.clone());
        self.controller = Some(controller.clone());
        self.spawn_data_bridge_for_new_controller(controller);
        crate::tui::forms::session::switch_to(profile_name, flags.clone());

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

    /// Start the bridge for `controller`, after the bridge before it has
    /// finished. Both drive the same controller here (a launch, a retry).
    pub(super) fn spawn_data_bridge(&mut self, controller: Controller) {
        // Chain on the previous bridge so its teardown (including the
        // mid-connect disconnect cleanup) fully completes before the next
        // bridge touches shared controller state.
        let predecessor = self.bridge_handle.take();
        self.spawn_data_bridge_after(controller, predecessor);
    }

    /// Start the bridge for a controller the session has just switched
    /// to, at once. The bridge before it drives another controller and
    /// shares nothing with this one: cancelled, it finishes its disconnect
    /// in the background. Waiting on it would hold this connect for as
    /// long as a controller that stopped answering takes to time out.
    fn spawn_data_bridge_for_new_controller(&mut self, controller: Controller) {
        drop(self.bridge_handle.take());
        self.spawn_data_bridge_after(controller, None);
    }

    fn spawn_data_bridge_after(
        &mut self,
        controller: Controller,
        predecessor: Option<tokio::task::JoinHandle<()>>,
    ) {
        let cancel = self.data_cancel.clone();
        let tx = self.action_tx.clone();
        let sanitizer = self.sanitizer.clone();
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

    /// A config that points nowhere: nothing answers at that port.
    fn nowhere() -> unifly_api::ControllerConfig {
        unifly_api::ControllerConfig {
            url: "http://127.0.0.1:1".parse().expect("url"),
            ..unifly_api::ControllerConfig::default()
        }
    }

    /// Switching controllers is not a reconnect. The status bar stops
    /// vouching for the controller that was left the moment the switch
    /// happens, and what was being located there is forgotten.
    #[tokio::test]
    async fn switching_controllers_resets_the_status_at_once() {
        let mut app = App::new(Some(Controller::new(nowhere())), None, false, true);
        app.active_screen = ScreenId::Dashboard;
        app.connection_status = crate::tui::app::ConnectionStatus::Connected;
        app.connection_error = Some("the old controller's last error".into());
        app.locating
            .insert(unifly_api::EntityId::Legacy("device-on-the-old-one".into()));

        let flags = crate::config::resolve::ConnectionOverrides::default();
        app.apply_connection("work", &nowhere(), &[], &flags)
            .expect("apply connection");

        // Read before any action is processed: no bridge has reported.
        assert_eq!(
            app.connection_status,
            crate::tui::app::ConnectionStatus::Connecting
        );
        assert_eq!(app.connection_error, None);
        assert!(app.locating.is_empty());
        app.data_cancel.cancel();
    }

    /// The bridge of the controller being left may be stuck tearing down
    /// (a controller that stopped answering holds its disconnect until the
    /// request times out). The new controller's bridge must not wait for
    /// it, while a retry on the same controller still waits its turn.
    #[tokio::test]
    async fn a_new_controller_connects_without_waiting_on_the_old_bridge() {
        async fn bridge_reports(app: &mut App, within_ms: u64) -> bool {
            tokio::time::timeout(std::time::Duration::from_millis(within_ms), async {
                loop {
                    match app.action_rx.recv().await {
                        Some(Action::Connecting) => break true,
                        Some(_) => {}
                        None => break false,
                    }
                }
            })
            .await
            .unwrap_or(false)
        }
        let stuck = || tokio::spawn(std::future::pending::<()>());

        let mut app = App::new(Some(Controller::new(nowhere())), None, false, true);
        app.active_screen = ScreenId::Dashboard;
        let old_bridge = stuck();
        let old_abort = old_bridge.abort_handle();
        app.bridge_handle = Some(old_bridge);

        let flags = crate::config::resolve::ConnectionOverrides::default();
        app.apply_connection("work", &nowhere(), &[], &flags)
            .expect("apply connection");
        assert!(
            bridge_reports(&mut app, 2000).await,
            "the new bridge started while the old one was still tearing down"
        );
        assert!(!old_abort.is_finished(), "left to finish in the background");
        old_abort.abort();

        // A retry drives the same controller, so it still queues behind
        // the bridge it replaces.
        app.data_cancel.cancel();
        app.data_cancel = CancellationToken::new();
        while app.action_rx.try_recv().is_ok() {}
        let predecessor = stuck();
        let release = predecessor.abort_handle();
        app.bridge_handle = Some(predecessor);
        let controller = app.controller.clone().expect("controller");
        app.spawn_data_bridge(controller);
        assert!(
            !bridge_reports(&mut app, 200).await,
            "a retry waits for its predecessor"
        );
        release.abort();
        assert!(
            bridge_reports(&mut app, 2000).await,
            "and starts once it is done"
        );
        app.data_cancel.cancel();
    }

    fn redacting() -> std::sync::Arc<crate::sanitizer::Sanitizer> {
        std::sync::Arc::new(crate::sanitizer::Sanitizer::new(
            &crate::config::DemoConfig::default(),
        ))
    }

    fn setup_frame(app: &App) -> String {
        let screen = app.screens.get(&ScreenId::Setup).expect("setup screen");
        crate::tui::table::render_lines(80, 24, |frame, area| screen.render(frame, area)).join("\n")
    }

    /// Settings hides its values while the session redacts what it shows,
    /// and the wizard holds the same ones: the launch's wizard and one
    /// opened for a new profile both come up hiding them.
    #[tokio::test]
    async fn a_redacting_session_opens_the_wizard_with_its_values_hidden() {
        use crossterm::event::{KeyCode, KeyEvent};

        let mut app =
            App::new(None, Some(redacting()), false, true).with_setup_profile("office", None);
        assert_eq!(app.active_screen, ScreenId::Setup);
        let type_key = |app: &mut App, code: KeyCode| {
            if let Some(action) = app.handle_key_event(KeyEvent::from(code)).expect("key") {
                app.process_action(&action).expect("action");
            }
        };
        type_key(&mut app, KeyCode::Enter);
        type_key(&mut app, KeyCode::Enter);
        for ch in "https://10.7.7.7".chars() {
            type_key(&mut app, KeyCode::Char(ch));
        }
        let rows = setup_frame(&app);
        assert!(rows.contains("Controller URL"), "{rows}");
        assert!(
            !rows.contains("10.7.7.7"),
            "the address is on screen:\n{rows}"
        );

        let controller = Controller::new(unifly_api::ControllerConfig {
            url: "http://127.0.0.1:1".parse().expect("url"),
            ..unifly_api::ControllerConfig::default()
        });
        let mut app = App::new(Some(controller), Some(redacting()), false, true);
        app.open_setup_for("lab-profile").expect("open setup");
        let rows = setup_frame(&app);
        assert!(rows.contains("Set up a new profile."), "{rows}");
        assert!(!rows.contains("lab-profile"), "{rows}");
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
        let flags = crate::config::resolve::ConnectionOverrides::default();
        app.apply_connection(
            "home",
            &unifly_api::ControllerConfig::default(),
            &[],
            &flags,
        )
        .expect("apply connection");
        assert!(app.demo_fleet_active(), "the demo controller must stay");
        app.open_setup_for("lab").expect("open setup");
        assert!(!app.screens.contains_key(&ScreenId::Setup));
    }
}
