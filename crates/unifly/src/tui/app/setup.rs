//! App-side handling for the setup wizard and the Settings screen: routing
//! their test and save results, and swapping the session's connection.

use color_eyre::eyre::Result;

use super::App;
use crate::tui::action::Action;
use crate::tui::screen::ScreenId;

impl App {
    /// Handle an action that belongs to setup or Settings. Returns whether
    /// it was one.
    pub(super) fn handle_setup_action(&mut self, action: &Action) -> Result<bool> {
        match action {
            // Either screen may have started the test or the save; each
            // one only takes a result that carries its own generation.
            Action::SetupProbeFinished { .. }
            | Action::SetupSaveFinished { .. }
            | Action::SetupKeyringChecked { .. } => {
                self.forward_to_screen(ScreenId::Setup, action)?;
                self.forward_to_screen(ScreenId::Settings, action)?;
            }
            Action::ApplyConnection {
                profile_name,
                config,
                notices,
                keep_flags,
            } => self.apply_connection(profile_name, config, notices, *keep_flags)?,
            Action::SetupNewProfile(name) => self.open_setup_for(name)?,
            Action::SetupCancelled => self.close_setup(),
            _ => return Ok(false),
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::action::NotificationLevel;

    /// A controller config that points nowhere: these tests never let a
    /// connection start, and this one could not reach anything if it did.
    fn nowhere() -> unifly_api::ControllerConfig {
        unifly_api::ControllerConfig {
            url: "http://127.0.0.1:1".parse().expect("url"),
            ..unifly_api::ControllerConfig::default()
        }
    }

    fn open_blank_settings(app: &mut App) {
        app.open_settings_with(crate::tui::screens::settings::SettingsScreen::blank)
            .expect("settings open");
    }

    fn live_app() -> App {
        let controller = unifly_api::Controller::new(nowhere());
        let mut app = App::new(Some(controller), None, false, true);
        app.active_screen = ScreenId::Dashboard;
        app
    }

    fn drain(app: &mut App) -> Vec<Action> {
        std::iter::from_fn(|| app.action_rx.try_recv().ok()).collect()
    }

    /// Applying a tested connection swaps the controller, tells every
    /// screen to drop the old one's data, and lands on a dashboard that
    /// Esc does not walk back from.
    #[tokio::test]
    async fn applying_a_connection_resets_the_session_onto_the_dashboard() {
        let mut app = live_app();
        app.active_screen = ScreenId::Events;
        open_blank_settings(&mut app);
        assert_eq!(app.previous_screen, Some(ScreenId::Events));

        let mut config = nowhere();
        config.site = "lab".into();
        app.process_action(&Action::ApplyConnection {
            profile_name: "work".into(),
            config: Box::new(config),
            notices: vec!["No usable keyring; saved the API key in plaintext".into()],
            keep_flags: true,
        })
        .expect("apply");

        assert_eq!(app.active_screen, ScreenId::Dashboard);
        assert_eq!(app.previous_screen, None);
        assert!(!app.screens.contains_key(&ScreenId::Settings));
        assert_eq!(
            app.controller.as_ref().map(|c| c.config().site.clone()),
            Some("lab".into())
        );

        let sent = drain(&mut app);
        assert!(
            sent.iter().any(|a| matches!(a, Action::ControllerChanged)),
            "{sent:?}"
        );
        // The plaintext fallback is a warning, which stays until dismissed.
        assert!(
            sent.iter().any(|a| matches!(
                a,
                Action::Notify(n) if n.level == NotificationLevel::Warning
                    && n.message.contains("plaintext")
            )),
            "{sent:?}"
        );
        app.data_cancel.cancel();
    }

    /// A wizard opened from Settings for a new profile can be left again,
    /// back to the running session.
    #[tokio::test]
    async fn a_wizard_opened_for_a_new_profile_can_be_cancelled() {
        let mut app = live_app();
        open_blank_settings(&mut app);

        app.process_action(&Action::SetupNewProfile("lab".into()))
            .expect("open setup");
        assert_eq!(app.active_screen, ScreenId::Setup);
        assert!(!app.screens.contains_key(&ScreenId::Settings));

        app.process_action(&Action::SetupCancelled).expect("cancel");
        assert_eq!(app.active_screen, ScreenId::Dashboard);
        assert!(!app.screens.contains_key(&ScreenId::Setup));
        assert!(app.controller.is_some(), "the session keeps its controller");
    }

    /// On a first run the wizard is the whole session: there is nothing
    /// to cancel back to.
    #[test]
    fn a_first_run_wizard_cannot_be_cancelled_away() {
        let mut app = App::new(None, None, false, true);

        app.process_action(&Action::SetupCancelled).expect("cancel");

        assert_eq!(app.active_screen, ScreenId::Setup);
        assert!(app.screens.contains_key(&ScreenId::Setup));
    }
}
