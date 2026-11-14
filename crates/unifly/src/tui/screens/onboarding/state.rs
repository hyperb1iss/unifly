use super::{OnboardingScreen, Purpose, TestPhase, WizardField, WizardStep};
use crate::config::resolve::ConnectionOverrides;
use crate::tui::action::Action;
use crate::tui::forms::controller_profile::{AuthMode, ControllerProfileDraft, DraftField, Reveal};
use crate::tui::forms::probe::{
    FailureKind, ProbeFailure, ProbeRequest, ProbeResult, ProbeSlot, Tested,
};
use crate::tui::forms::session::{self, Session};

impl WizardStep {
    /// The step's position in the step indicator.
    pub(super) fn index(self) -> usize {
        match self {
            Self::Welcome => 0,
            Self::AuthMode => 1,
            Self::Url => 2,
            Self::Credentials => 3,
            Self::Testing => 4,
            Self::Site => 5,
            Self::Done => 6,
        }
    }
}

// ── Construction ────────────────────────────────────────────────────

impl OnboardingScreen {
    pub fn new() -> Self {
        Self::for_profile("default")
    }

    /// A wizard that sets up `profile_name`: the profile the launch asked
    /// for, so finishing it never writes over a different one.
    pub fn for_profile(profile_name: &str) -> Self {
        Self::in_session(profile_name, session::current())
    }

    /// A wizard for `profile_name` opened in `session`.
    pub(super) fn in_session(profile_name: &str, session: Session) -> Self {
        let mut draft = ControllerProfileDraft::default();
        prefill(&mut draft, &session);
        Self {
            focused: false,
            action_tx: None,
            step: WizardStep::Welcome,
            draft,
            auth_mode_index: 0,
            field: 0,
            reveal: Reveal::default(),
            phase: TestPhase::Idle,
            failure: None,
            error: None,
            notice: None,
            throbber_state: throbber_widgets_tui::ThrobberState::default(),
            profile_name: profile_name.to_owned(),
            session,
            purpose: Purpose::FirstRun,
            offers_ca: false,
            probe: ProbeSlot::default(),
            held: None,
            tested: None,
            known_sites: Vec::new(),
            site_index: 0,
            site_confirmed: false,
            notices: Vec::new(),
            hits: std::cell::RefCell::default(),
            targets: std::cell::RefCell::default(),
        }
    }

    /// A wizard that repairs `existing`, a saved profile that could not
    /// connect. It starts from that profile, as Settings does, so saving
    /// keeps what the wizard does not show (a CA certificate, a timeout, a
    /// TOTP command, an API key variable) and the secrets already in `store`.
    pub fn for_existing_profile(
        profile_name: &str,
        existing: &crate::config::Profile,
        defaults: &crate::config::Defaults,
        store: &dyn crate::config::SecretStore,
    ) -> Self {
        let mut screen = Self::for_profile(profile_name);
        screen.draft = ControllerProfileDraft::from_profile(existing)
            .with_keyring_origins(profile_name, store)
            .with_inherited_insecure(defaults);
        prefill(&mut screen.draft, &screen.session);
        screen.offers_ca = !screen.draft.ca_cert.is_empty();
        screen.auth_mode_index = screen.draft.auth_mode.index();
        screen.purpose = Purpose::Repair;
        // Its site was chosen when the profile was made.
        screen.site_confirmed = true;
        screen
    }

    /// A wizard opened from a running session to add `profile_name`.
    /// Leaving its first step goes back to that session.
    pub(crate) fn for_new_profile(profile_name: &str) -> Self {
        let mut screen = Self::in_session(profile_name, Session::default());
        screen.purpose = Purpose::NewProfile;
        screen
    }

    /// Why the wizard opened for a profile that already exists, when the
    /// launch recorded a reason.
    pub(super) fn repair_reason(&self) -> Option<&str> {
        (self.purpose == Purpose::Repair)
            .then_some(self.session.setup_reason.as_deref())
            .flatten()
    }
}

/// Start `draft` from what the launch flags gave. The wizard asks for
/// every value, so a flag is an answer filled in ahead of time, there to
/// be seen and changed, and what gets tested is what the steps show. An
/// API key given by flag or environment is left out: a secret is not
/// copied into a saved profile without being typed there.
fn prefill(draft: &mut ControllerProfileDraft, session: &Session) {
    use secrecy::ExposeSecret;

    let flags = &session.overrides;
    if let Some(url) = &flags.controller {
        draft.url.set(url.as_str());
    }
    if let Some(site) = &flags.site {
        draft.site.set(site.as_str());
    }
    if let Some(host_id) = &flags.host_id {
        draft.host_id.set(host_id.as_str());
    }
    if let Some(insecure) = flags.insecure
        && insecure != draft.insecure()
    {
        draft.set_insecure(insecure);
    }
    if let Some(timeout) = flags.timeout {
        draft.timeout.set(timeout.to_string());
    }
    // A code given at launch is for this sign-in.
    if let Some(code) = &session.launch_totp {
        draft.totp.set(code.expose_secret());
    }
}

// ── Fields ──────────────────────────────────────────────────────────

impl OnboardingScreen {
    /// What the current step lets the user focus, in order.
    pub(super) fn fields(&self) -> Vec<WizardField> {
        let text = |field| WizardField::Text(field);
        match self.step {
            WizardStep::Url => {
                let mut fields = vec![text(DraftField::Url), WizardField::Insecure];
                // A profile being repaired may carry a CA certificate, and
                // a broken one has to be fixable here.
                if self.offers_ca {
                    fields.push(text(DraftField::CaCert));
                }
                fields
            }
            WizardStep::Credentials => {
                let mode = self.draft.auth_mode;
                let mut fields = Vec::new();
                if mode.uses_api_key() {
                    fields.push(text(DraftField::ApiKey));
                }
                if mode == AuthMode::Cloud {
                    fields.push(text(DraftField::HostId));
                }
                if mode.uses_login() {
                    fields.extend([
                        text(DraftField::Username),
                        text(DraftField::Password),
                        text(DraftField::Totp),
                    ]);
                }
                fields
            }
            WizardStep::Site if !self.picks_site() => vec![text(DraftField::Site)],
            _ => Vec::new(),
        }
    }

    pub(super) fn active_field(&self) -> Option<WizardField> {
        self.fields().get(self.field).copied()
    }

    /// The text field typed characters go into, when one has focus.
    pub(super) fn active_text_field(&self) -> Option<DraftField> {
        match self.active_field()? {
            WizardField::Text(field) => Some(field),
            WizardField::Insecure => None,
        }
    }

    pub(super) fn step_field(&mut self, forward: bool) {
        let count = self.fields().len();
        if count == 0 {
            return;
        }
        self.field = if forward {
            (self.field + 1) % count
        } else {
            (self.field + count - 1) % count
        };
    }

    /// Focus `field` if the current step has it.
    fn focus(&mut self, field: DraftField) {
        let wanted = match field {
            DraftField::Insecure => WizardField::Insecure,
            other => WizardField::Text(other),
        };
        if let Some(index) = self.fields().iter().position(|have| *have == wanted) {
            self.field = index;
        }
    }

    /// Whether the Site step lists the controller's sites to pick from.
    pub(super) fn picks_site(&self) -> bool {
        !self.known_sites.is_empty()
    }

    /// The step that asks for `field`.
    fn step_for(&self, field: DraftField) -> WizardStep {
        match field {
            DraftField::AuthMode => WizardStep::AuthMode,
            DraftField::Url | DraftField::Insecure | DraftField::CaCert | DraftField::Timeout
                if self.draft.auth_mode != AuthMode::Cloud =>
            {
                WizardStep::Url
            }
            DraftField::Site => WizardStep::Site,
            _ => WizardStep::Credentials,
        }
    }

    /// Go to `step` with focus on its first field and nothing reported.
    pub(super) fn show_step(&mut self, step: WizardStep) {
        self.step = step;
        self.field = 0;
        self.error = None;
        self.notice = None;
    }
}

// ── Moving between steps ────────────────────────────────────────────

impl OnboardingScreen {
    pub(super) fn advance(&mut self) -> Option<Action> {
        self.error = None;
        self.notice = None;
        match self.step {
            WizardStep::Welcome => self.show_step(WizardStep::AuthMode),
            WizardStep::AuthMode => {
                self.draft.auth_mode = AuthMode::ALL[self.auth_mode_index];
                // Cloud goes through the connector: there is no address
                // to ask for.
                self.show_step(if self.draft.auth_mode == AuthMode::Cloud {
                    WizardStep::Credentials
                } else {
                    WizardStep::Url
                });
            }
            WizardStep::Url => self.advance_from_url(),
            WizardStep::Credentials => {
                if let Err(error) = self.draft.validate_credentials() {
                    self.focus(error.field);
                    self.error = Some(error.message);
                } else {
                    self.start_test();
                }
            }
            WizardStep::Site => self.advance_from_site(),
            WizardStep::Testing if self.phase == TestPhase::Idle => self.start_test(),
            WizardStep::Testing => {}
            WizardStep::Done => return self.complete(),
        }
        None
    }

    fn advance_from_url(&mut self) {
        if let Some(done) = self.draft.tidy_url() {
            self.focus(DraftField::Url);
            self.notice = Some(format!("{done} Press Enter to continue."));
            return;
        }
        if let Err(error) = self.draft.validate_url() {
            self.focus(DraftField::Url);
            self.error = Some(error.message);
            return;
        }
        self.show_step(WizardStep::Credentials);
    }

    fn advance_from_site(&mut self) {
        if self.picks_site() {
            if let Some(site) = self.known_sites.get(self.site_index) {
                self.draft.site.set(site.internal_reference.clone());
            }
        } else if self.draft.site.text().trim().is_empty() {
            self.error = Some("Site name cannot be empty".into());
            return;
        }
        self.site_confirmed = true;

        // A test that already passed with this very site needs no repeat.
        let chosen = self.draft.site.text().trim().to_owned();
        match self.held.take() {
            Some((tested, site)) if site == chosen => self.start_save(tested),
            _ => self.start_test(),
        }
    }

    /// Step back. On the first step this leaves a wizard that was opened
    /// from a running session.
    pub(super) fn go_back(&mut self) -> Option<Action> {
        match self.step {
            WizardStep::Welcome => {
                return (self.purpose == Purpose::NewProfile).then_some(Action::SetupCancelled);
            }
            WizardStep::AuthMode => self.show_step(WizardStep::Welcome),
            WizardStep::Url => self.show_step(WizardStep::AuthMode),
            WizardStep::Credentials => self.show_step(self.step_before_credentials()),
            WizardStep::Site => {
                self.held = None;
                self.show_step(WizardStep::Credentials);
            }
            WizardStep::Testing => self.leave_testing(),
            // The profile is saved; there is nothing to go back to.
            WizardStep::Done => {}
        }
        None
    }

    fn step_before_credentials(&self) -> WizardStep {
        if self.draft.auth_mode == AuthMode::Cloud {
            WizardStep::AuthMode
        } else {
            WizardStep::Url
        }
    }

    /// Leave the Testing step: cancel a running test, or go back to the
    /// field a failed one was about. A save runs to its end.
    fn leave_testing(&mut self) {
        match self.phase {
            TestPhase::Saving => {}
            TestPhase::Testing => {
                self.probe.cancel();
                self.phase = TestPhase::Idle;
                self.show_step(WizardStep::Credentials);
                self.notice = Some("Test cancelled. Nothing was saved.".into());
            }
            TestPhase::Idle => {
                let at = self.failure.take().and_then(|failure| failure.field);
                let step = at.map_or(WizardStep::Credentials, |field| self.step_for(field));
                self.show_step(step);
                if let Some(field) = at {
                    self.focus(field);
                }
            }
        }
    }
}

// ── Connection test and save ────────────────────────────────────────

impl OnboardingScreen {
    pub(super) fn start_test(&mut self) {
        self.step = WizardStep::Testing;
        self.phase = TestPhase::Testing;
        self.failure = None;
        self.held = None;
        self.tested = None;
        self.error = None;
        self.notice = None;

        let request = ProbeRequest {
            profile: self.draft.to_profile(),
            profile_name: self.profile_name.clone(),
            // The wizard asks for every value, so what its steps show is
            // what gets tested, with nothing from the launch on top.
            overrides: ConnectionOverrides::default(),
            totp: self.draft.totp_code(),
        };
        let plan = self.draft.plan_save(&self.profile_name);
        self.probe.start(self.action_tx.as_ref(), request, plan);
    }

    fn start_save(&mut self, tested: Tested) {
        self.step = WizardStep::Testing;
        self.phase = TestPhase::Saving;
        if let Some(live) = &self.tested {
            self.probe.save(self.action_tx.as_ref(), tested, live);
        }
    }

    /// Take the result of test `generation`. A result from a test that was
    /// cancelled or replaced is dropped, whatever step is showing.
    pub(super) fn finish_test(&mut self, generation: u64, result: &ProbeResult) {
        let Some(tested) = self.probe.claim(generation) else {
            return;
        };
        match result {
            Ok(pass) => {
                self.known_sites.clone_from(&pass.sites);
                self.tested = Some(pass.config.clone());
                if self.site_confirmed || pass.sites.len() < 2 {
                    self.start_save(tested);
                } else {
                    // Several sites and none chosen yet: ask before saving
                    // the one the test happened to use.
                    let site = self.draft.site.text().trim().to_owned();
                    self.held = Some((tested, site));
                    self.show_site_step(None);
                }
            }
            Err(failure) => self.show_failure(failure.clone()),
        }
    }

    fn show_failure(&mut self, failure: ProbeFailure) {
        self.phase = TestPhase::Idle;
        if failure.kind == FailureKind::Site {
            // The controller answered; only the site is wrong, and it may
            // have said which ones it has.
            self.known_sites.clone_from(&failure.sites);
            self.site_confirmed = false;
            self.show_site_step(Some(failure.summary));
            return;
        }
        self.failure = Some(failure);
    }

    fn show_site_step(&mut self, error: Option<String>) {
        let current = self.draft.site.text().trim().to_owned();
        self.site_index = self
            .known_sites
            .iter()
            .position(|site| site.internal_reference == current)
            .unwrap_or(0);
        self.phase = TestPhase::Idle;
        self.show_step(WizardStep::Site);
        self.error = error;
    }

    /// Take the result of the save that followed test `generation`.
    pub(super) fn finish_save(&mut self, generation: u64, result: &Result<Vec<String>, String>) {
        if self.phase != TestPhase::Saving || !self.probe.owns(generation) {
            return;
        }
        self.phase = TestPhase::Idle;
        match result {
            Ok(notices) => {
                self.notices.clone_from(notices);
                self.show_step(WizardStep::Done);
            }
            Err(message) => self.failure = Some(ProbeFailure::save(message.clone())),
        }
    }

    /// Accept the certificate the controller presented and test again: the
    /// answer to a certificate that could not be verified.
    pub(super) fn trust_and_retry(&mut self) {
        if self.offers_trust() {
            self.draft.toggle_insecure();
            self.start_test();
        }
    }

    /// Whether the failure showing is a certificate the user can choose
    /// to accept.
    pub(super) fn offers_trust(&self) -> bool {
        self.phase == TestPhase::Idle
            && !self.draft.insecure()
            && self.draft.auth_mode != AuthMode::Cloud
            && self
                .failure
                .as_ref()
                .is_some_and(|failure| failure.kind == FailureKind::Tls)
            && self.failure.as_ref().and_then(|failure| failure.field) == Some(DraftField::Insecure)
    }

    /// Hand the tested connection to the app.
    fn complete(&mut self) -> Option<Action> {
        let config = self.tested.clone()?;
        Some(Action::ApplyConnection {
            profile_name: self.profile_name.clone(),
            config,
            notices: self.notices.clone(),
            // The steps asked for every value the flags could override.
            keep_flags: false,
        })
    }
}
