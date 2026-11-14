//! Setup wizard: connects a profile step by step when the launch has no
//! usable one, or when Settings asks for a new one.
//!
//! Flow: Welcome -> Auth mode -> Address (skipped for Cloud) -> Credentials
//! -> Test -> Site (only when the controller has several, or lacks the one
//! asked for) -> Done. The test connects with what was typed; when it
//! passes, that profile is saved under the name the wizard was opened for,
//! keeping every other profile and setting in the config file, and Done
//! emits `ApplyConnection` so the app connects with the tested connection.

mod input;
mod render;
mod state;
#[cfg(test)]
mod tests;

use std::cell::RefCell;

use color_eyre::eyre::Result;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use tokio::sync::mpsc::UnboundedSender;
use unifly_api::SiteHint;

use crate::tui::action::Action;
use crate::tui::component::Component;
use crate::tui::forms::controller_profile::{ControllerProfileDraft, DraftField, Reveal};
use crate::tui::forms::probe::{ProbeFailure, ProbeSlot, Tested};
use crate::tui::forms::session::Session;
use crate::tui::widgets::click::{Click, HitMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WizardStep {
    Welcome,
    AuthMode,
    Url,
    Credentials,
    Testing,
    Site,
    Done,
}

/// Something a step lets the user focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WizardField {
    Text(DraftField),
    /// The "skip TLS verification" checkbox.
    Insecure,
}

/// What the Testing step is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TestPhase {
    /// Nothing is running: the last test failed, or none has started.
    Idle,
    Testing,
    Saving,
}

/// Why the wizard is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Purpose {
    /// The launch had no profile to connect with.
    FirstRun,
    /// The launch's profile exists but could not connect.
    Repair,
    /// A running session asked for another profile. Leaving the first
    /// step goes back to that session.
    NewProfile,
}

/// What a click on the wizard does, besides its key hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WizardHit {
    /// Pick a row of the list the step shows.
    Choice(usize),
    /// Focus one of the step's fields.
    Field(usize),
}

pub struct OnboardingScreen {
    focused: bool,
    action_tx: Option<UnboundedSender<Action>>,
    step: WizardStep,
    draft: ControllerProfileDraft,
    auth_mode_index: usize,
    /// Which of the step's fields has focus.
    field: usize,
    reveal: Reveal,
    phase: TestPhase,
    /// Why the last test or save failed, shown on the Testing step.
    failure: Option<ProbeFailure>,
    /// A problem with what was typed on this step.
    error: Option<String>,
    /// Something the step wants noticed that is not a problem.
    notice: Option<String>,
    throbber_state: throbber_widgets_tui::ThrobberState,
    /// The profile the wizard sets up and saves.
    profile_name: String,
    /// The session the wizard opened in: launch flags, and why it opened.
    session: Session,
    purpose: Purpose,
    probe: ProbeSlot,
    /// A test that passed and waits on the site choice before it is saved:
    /// what it tested, and the site it passed with.
    held: Option<(Tested, String)>,
    /// The connection the last passed test produced.
    tested: Option<Box<unifly_api::ControllerConfig>>,
    /// The controller's sites, once a test has seen them.
    known_sites: Vec<SiteHint>,
    site_index: usize,
    /// Whether the site was already chosen, so a passing test saves
    /// without asking again.
    site_confirmed: bool,
    /// What the save reported, such as a secret kept in plaintext.
    notices: Vec<String>,
    /// What the last frame drew where: key hints, then rows and fields.
    hits: RefCell<HitMap<Click>>,
    targets: RefCell<HitMap<WizardHit>>,
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

    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        Ok(self.handle_mouse_input(mouse))
    }

    fn handle_paste(&mut self, text: &str) -> Option<Action> {
        self.paste(text);
        None
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        Ok(self.apply_action(action))
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        self.render_screen(frame, area);
    }

    fn captures_input(&self) -> bool {
        self.active_text_field().is_some()
    }

    fn help(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("Enter", "Next step, or retry a failed test"),
            ("Esc", "Previous step, or cancel a running test"),
            ("Tab/\u{2193}  Shift+Tab/\u{2191}", "Next / previous field"),
            ("\u{2191}/\u{2193} j/k", "Move in a list"),
            ("Space", "Toggle a checkbox"),
            ("Ctrl+T", "Show or hide the API key or password"),
            ("Ctrl+C", "Quit"),
        ]
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
