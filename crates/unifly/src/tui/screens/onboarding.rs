//! Onboarding wizard - guides setup when no usable profile is configured.
//!
//! Flow: Welcome -> URL -> AuthMode -> Credentials -> Site -> Testing -> Done.
//! On completion, saves its profile into the config file under the name the
//! launch selected, keeping any other profiles and settings there, and emits
//! `OnboardingComplete` with the built `ControllerConfig` so the app can
//! connect immediately.

mod input;
mod render;
mod state;

use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::Rect;
use tokio::sync::mpsc::UnboundedSender;

use crate::tui::action::Action;
use crate::tui::component::Component;
pub(super) use crate::tui::forms::controller_profile::{AuthMode, ControllerProfileDraft};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WizardStep {
    Welcome,
    Url,
    AuthMode,
    Credentials,
    Site,
    Testing,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CredentialField {
    ApiKey,
    HostId,
    Username,
    Password,
}

pub struct OnboardingScreen {
    focused: bool,
    action_tx: Option<UnboundedSender<Action>>,
    step: WizardStep,
    draft: ControllerProfileDraft,
    auth_mode_index: usize,
    cred_field: CredentialField,
    show_password: bool,
    testing: bool,
    test_error: Option<String>,
    error: Option<String>,
    throbber_state: throbber_widgets_tui::ThrobberState,
    /// The profile the wizard sets up and saves.
    profile_name: String,
}

impl Default for OnboardingScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for OnboardingScreen {
    fn init(&mut self, action_tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(action_tx);
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        Ok(self.handle_key_input(key))
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        self.apply_action(action);
        Ok(None)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        self.render_screen(frame, area);
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn id(&self) -> &'static str {
        "onboarding"
    }
}
