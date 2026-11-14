use std::path::Path;

use unifly_api::SiteHint;

use super::{FormEntry, Leave, Message, SettingsField, SettingsScreen, SettingsState};
use crate::config::resolve::ConnectionOverrides;
use crate::tui::action::{Action, ConfirmRequest};
use crate::tui::forms::controller_profile::{AuthMode, ControllerProfileDraft, DraftField, Reveal};
use crate::tui::forms::probe::{ProbeFailure, ProbeRequest, ProbeResult, ProbeSlot};
use crate::tui::forms::session::{self, Overridable, Session};
use crate::tui::widgets::line_input::LineInput;

// ── Field metadata ──────────────────────────────────────────────────

impl SettingsField {
    pub(super) const ALL: [SettingsField; 14] = [
        Self::Profile,
        Self::Url,
        Self::AuthMode,
        Self::ApiKey,
        Self::HostId,
        Self::Username,
        Self::Password,
        Self::Totp,
        Self::Site,
        Self::Insecure,
        Self::CaCert,
        Self::Timeout,
        Self::Theme,
        Self::ShowDonate,
    ];

    /// Section label for this field, used to insert the section rules.
    pub(super) fn section(self) -> &'static str {
        match self {
            Self::Profile => "Profile",
            Self::Theme | Self::ShowDonate => "Appearance",
            _ => "Connection",
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Profile => "Profile",
            Self::Url => "Controller URL",
            Self::AuthMode => "Auth mode",
            Self::ApiKey => "API key",
            Self::HostId => "Host ID",
            Self::Username => "Username",
            Self::Password => "Password",
            Self::Totp => "One-time code",
            Self::Site => "Site",
            Self::Insecure => "Skip TLS verify",
            Self::CaCert => "CA certificate",
            Self::Timeout => "Timeout",
            Self::Theme => "Theme",
            Self::ShowDonate => "Sponsor button",
        }
    }

    /// Whether this field is visible given the current auth mode. Cloud
    /// goes through the connector, so it has no address or TLS settings.
    pub(super) fn visible_for(self, mode: AuthMode) -> bool {
        match self {
            Self::Profile
            | Self::AuthMode
            | Self::Site
            | Self::Timeout
            | Self::Theme
            | Self::ShowDonate => true,
            Self::Url | Self::Insecure | Self::CaCert => mode != AuthMode::Cloud,
            Self::ApiKey => mode.uses_api_key(),
            Self::HostId => mode == AuthMode::Cloud,
            Self::Username | Self::Password | Self::Totp => mode.uses_login(),
        }
    }

    /// The launch flag setting this field can be overridden by.
    fn overridable(self) -> Option<Overridable> {
        match self {
            Self::Url => Some(Overridable::Url),
            Self::Site => Some(Overridable::Site),
            Self::ApiKey => Some(Overridable::ApiKey),
            Self::HostId => Some(Overridable::HostId),
            Self::Insecure => Some(Overridable::Insecure),
            Self::Timeout => Some(Overridable::Timeout),
            Self::AuthMode => Some(Overridable::AuthMode),
            _ => None,
        }
    }
}

// ── Construction ────────────────────────────────────────────────────

impl SettingsScreen {
    /// The screen for the profile this session runs on, read from the
    /// config file.
    pub fn new() -> Self {
        let mut screen = Self::blank();
        screen.session = session::current();
        let path = crate::config::config_path();
        if let Ok(cfg) = crate::config::load_config_from(&path) {
            let session_profile = screen.session.profile.clone();
            screen.load(&cfg, session_profile.as_deref());
        }
        screen.config_path = Some(path);
        screen
    }

    /// A screen with default values that reads and writes no config file
    /// and no keyring.
    pub(crate) fn blank() -> Self {
        Self {
            focused: false,
            action_tx: None,
            state: SettingsState::Editing,
            active_field: SettingsField::Url,
            draft: ControllerProfileDraft::default(),
            original: ControllerProfileDraft::default(),
            reveal: Reveal::default(),
            profile_name: "default".into(),
            profiles: vec!["default".into()],
            new_profile: LineInput::default(),
            session: Session::default(),
            default_timeout: crate::config::Defaults::default().timeout,
            config_path: None,
            message: None,
            probe: ProbeSlot::default(),
            tested: None,
            known_sites: Vec::new(),
            sites_for: String::new(),
            site_typed: false,
            private: false,
            throbber_state: throbber_widgets_tui::ThrobberState::default(),
            scroll: std::cell::Cell::new(0),
            follow_focus: std::cell::Cell::new(true),
            theme_selector: std::cell::RefCell::new(None),
            hits: std::cell::RefCell::default(),
            field_hits: std::cell::RefCell::default(),
            show_donate: true,
        }
    }

    /// Draw every value masked and never reveal one: the session is
    /// redacting what it shows.
    pub(crate) fn hide_values(&mut self) {
        self.private = true;
    }

    /// Offer `sites`, read from the controller the session is connected
    /// to, as the choices for the Site field.
    pub(crate) fn set_known_sites(&mut self, sites: Vec<SiteHint>) {
        self.sites_for = self.draft.controller_identity();
        self.known_sites = sites;
        self.site_typed = false;
    }

    /// Fill the form from the profile this session uses (`session_profile`,
    /// else the config's default), which is also the one saving writes.
    pub(super) fn load(&mut self, cfg: &crate::config::Config, session_profile: Option<&str>) {
        self.show_donate = cfg.defaults.show_donate;
        let profile_name = session_profile
            .or(cfg.default_profile.as_deref())
            .unwrap_or("default")
            .to_owned();
        self.show_profile(cfg, &profile_name);
    }

    /// Show `profile_name` from `cfg` in the form, as loaded.
    pub(super) fn show_profile(&mut self, cfg: &crate::config::Config, profile_name: &str) {
        self.default_timeout = cfg.defaults.timeout;
        self.profiles = cfg.profiles.keys().cloned().collect();
        if !self.profiles.iter().any(|name| name == profile_name) {
            self.profiles.push(profile_name.to_owned());
        }
        self.profiles.sort();
        profile_name.clone_into(&mut self.profile_name);
        self.draft = cfg.profiles.get(profile_name).map_or_else(
            ControllerProfileDraft::default,
            |profile| {
                ControllerProfileDraft::from_profile(profile).with_inherited_insecure(&cfg.defaults)
            },
        );
        self.original = self.draft.clone();
        self.reveal = Reveal::default();
        self.known_sites.clear();
        self.sites_for.clear();
        self.site_typed = false;
        self.message = None;
        self.clamp_focus();
    }

    /// Ask the keyring, off the UI loop, which of this profile's
    /// credentials it holds. An unlock prompt must not freeze the screen.
    pub(super) fn check_keyring(&self) {
        // A screen without a config file has no profile in the keyring to
        // ask about, and a unit test must never reach the real keyring.
        if cfg!(test) || self.config_path.is_none() {
            return;
        }
        let Some(tx) = self.action_tx.clone() else {
            return;
        };
        let profile = self.profile_name.clone();
        tokio::task::spawn_blocking(move || {
            let (api_key, password) = crate::tui::forms::controller_profile::keyring_holds(
                &profile,
                &crate::config::OsKeyring,
            );
            let _ = tx.send(Action::SetupKeyringChecked {
                profile,
                api_key,
                password,
            });
        });
    }

    /// Whether saving the form as it stands would write a profile other
    /// than the one it was loaded from.
    pub(super) fn is_dirty(&self) -> bool {
        let saved = |draft: &ControllerProfileDraft| {
            toml::to_string(&draft.to_profile()).unwrap_or_default()
        };
        saved(&self.draft) != saved(&self.original)
    }

    /// Whether the form holds something other than what was loaded in
    /// `field`.
    fn changed(&self, field: DraftField) -> bool {
        match field {
            DraftField::AuthMode => self.draft.auth_mode != self.original.auth_mode,
            DraftField::Insecure => self.draft.insecure() != self.original.insecure(),
            text => {
                self.draft.input(text).map(LineInput::text)
                    != self.original.input(text).map(LineInput::text)
            }
        }
    }

    /// Whether the launch flag on `setting` still applies to what the form
    /// tests. It stands down once the form changes a value it would keep
    /// the test from exercising: the test must connect with what is
    /// typed, because that is what gets saved.
    fn stands(&self, setting: Overridable) -> bool {
        let changed = |fields: &[DraftField]| fields.iter().any(|field| self.changed(*field));
        match setting {
            Overridable::Url => !changed(&[DraftField::Url]),
            Overridable::Site => !changed(&[DraftField::Site]),
            Overridable::HostId => !changed(&[DraftField::HostId]),
            Overridable::Timeout => !changed(&[DraftField::Timeout]),
            // `--insecure` decides whether a CA is consulted at all.
            Overridable::Insecure => !changed(&[DraftField::Insecure, DraftField::CaCert]),
            Overridable::ApiKey | Overridable::AuthMode => {
                // A key given at launch replaces the profile's, and can
                // switch the connection to a mode that never signs in.
                let profile = self.draft.to_profile();
                let forced = self.session.overrides.apply(&profile).auth_mode != profile.auth_mode;
                let sign_in = [
                    DraftField::AuthMode,
                    DraftField::Username,
                    DraftField::Password,
                    DraftField::HostId,
                ];
                let untested = changed(&[DraftField::ApiKey]) || (forced && changed(&sign_in));
                !untested
            }
        }
    }

    /// Whether the form shows the profile the session is running on.
    fn shows_session_profile(&self) -> bool {
        self.session
            .profile
            .as_deref()
            .is_none_or(|name| name == self.profile_name)
    }

    /// The launch flags that apply to what the form tests, and to the
    /// session once that test is saved. They were given for the session's
    /// profile, so another profile connects without them, and one the
    /// form has changed the value under stands down.
    pub(super) fn overrides(&self) -> ConnectionOverrides {
        let launch = &self.session.overrides;
        let mut kept = ConnectionOverrides {
            no_cache: launch.no_cache,
            ..ConnectionOverrides::default()
        };
        if !self.shows_session_profile() {
            return kept;
        }
        if self.stands(Overridable::Url) {
            kept.controller.clone_from(&launch.controller);
        }
        if self.stands(Overridable::Site) {
            kept.site.clone_from(&launch.site);
        }
        if self.stands(Overridable::ApiKey) {
            kept.api_key.clone_from(&launch.api_key);
        }
        if self.stands(Overridable::HostId) {
            kept.host_id.clone_from(&launch.host_id);
        }
        if self.stands(Overridable::Insecure) {
            kept.insecure = launch.insecure;
        }
        if self.stands(Overridable::Timeout) {
            kept.timeout = launch.timeout;
        }
        kept
    }

    /// The launch flag overriding `field` in the next test, if one does.
    pub(super) fn override_on(&self, field: SettingsField) -> Option<String> {
        let flag = session::describe(&self.overrides(), field.overridable()?)?;
        Some(self.shown_flag(flag))
    }

    /// The launch flag on `field` that the form's edits have set aside:
    /// the next test, and the session after it, use what is typed.
    pub(super) fn set_aside(&self, field: SettingsField) -> Option<String> {
        if !self.shows_session_profile() {
            return None;
        }
        let setting = field.overridable()?;
        let flag = self.session.override_on(setting)?;
        session::describe(&self.overrides(), setting)
            .is_none()
            .then(|| self.shown_flag(flag))
    }

    /// `flag` as the form may show it: its value is as private as the
    /// field's.
    fn shown_flag(&self, flag: String) -> String {
        if self.private {
            return flag.split([' ', '=']).next().unwrap_or_default().to_owned();
        }
        flag
    }
}

// ── Field navigation ────────────────────────────────────────────────

impl SettingsScreen {
    /// All visible fields in display order.
    pub(super) fn visible_fields(&self) -> Vec<SettingsField> {
        SettingsField::ALL
            .iter()
            .copied()
            .filter(|f| f.visible_for(self.draft.auth_mode))
            .collect()
    }

    /// The form's rows: every visible field under its section rule, plus
    /// the read-only rows.
    pub(super) fn form_layout(&self) -> Vec<FormEntry> {
        let mut entries = Vec::new();
        let mut current_section = "";

        for field in self.visible_fields() {
            let section = field.section();
            if section != current_section {
                entries.push(FormEntry::Section(section));
                current_section = section;
            }
            entries.push(FormEntry::Field(field));
            if field == SettingsField::Totp
                && let Some(source) = self.draft.mfa_source()
            {
                let shown = if self.private {
                    "set in the profile".to_owned()
                } else {
                    source
                };
                entries.push(FormEntry::Info("MFA codes", shown));
            }
        }

        entries
    }

    fn step_focus(&mut self, forward: bool) {
        let fields = self.visible_fields();
        let Some(pos) = fields.iter().position(|&f| f == self.active_field) else {
            return;
        };
        let next = if forward {
            (pos + 1) % fields.len()
        } else {
            (pos + fields.len() - 1) % fields.len()
        };
        self.focus(fields[next]);
    }

    pub(super) fn focus_next(&mut self) {
        self.step_focus(true);
    }

    pub(super) fn focus_prev(&mut self) {
        self.step_focus(false);
    }

    /// Move focus to `field` and scroll it into view.
    pub(super) fn focus(&mut self, field: SettingsField) {
        if field.visible_for(self.draft.auth_mode) {
            self.active_field = field;
        }
        self.follow_focus.set(true);
    }

    pub(super) fn clamp_focus(&mut self) {
        if !self.active_field.visible_for(self.draft.auth_mode) {
            self.active_field = SettingsField::AuthMode;
        }
    }

    /// Whether the Site field offers the controller's sites to pick from.
    /// They stop being offered once the form points at another controller
    /// or the user starts typing a name.
    pub(super) fn picks_site(&self) -> bool {
        !self.known_sites.is_empty()
            && !self.site_typed
            && self.sites_for == self.draft.controller_identity()
    }

    /// The text field the focused form field edits, when it is one.
    pub(super) fn active_text_field(&self) -> Option<DraftField> {
        let field = self.active_field.draft_field()?;
        let is_text =
            self.draft.input(field).is_some() && !(field == DraftField::Site && self.picks_site());
        is_text.then_some(field)
    }

    /// Whether the form is taking keys as they are typed. It does on every
    /// field: text fields edit, the site picker turns into one on the
    /// first letter, and the theme picker filters as it is typed in. Only
    /// a running test and a save do not.
    pub(super) fn typing(&self) -> bool {
        matches!(self.state, SettingsState::Editing | SettingsState::Naming)
    }

    // ── Pickers ─────────────────────────────────────────────────────

    pub(super) fn step_auth_mode(&mut self, by: isize) {
        let modes = AuthMode::ALL;
        self.draft.auth_mode = modes[stepped(self.draft.auth_mode.index(), modes.len(), by)];
        self.clamp_focus();
    }

    pub(super) fn step_site(&mut self, by: isize) {
        let current = self
            .known_sites
            .iter()
            .position(|site| site.internal_reference == self.draft.site.text());
        let next = match current {
            Some(index) => stepped(index, self.known_sites.len(), by),
            None => 0,
        };
        if let Some(site) = self.known_sites.get(next) {
            self.draft.site.set(site.internal_reference.clone());
        }
    }

    /// The profile `by` steps from the one shown.
    pub(super) fn neighbor_profile(&self, by: isize) -> Option<String> {
        let current = self
            .profiles
            .iter()
            .position(|name| *name == self.profile_name)?;
        let next = stepped(current, self.profiles.len(), by);
        (next != current).then(|| self.profiles[next].clone())
    }
}

/// `index` moved `by` steps around a list of `len`, wrapping at both ends.
fn stepped(index: usize, len: usize, by: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let len = isize::try_from(len).unwrap_or(isize::MAX);
    let index = isize::try_from(index).unwrap_or(0);
    usize::try_from((index + by).rem_euclid(len)).unwrap_or(0)
}

// ── Leaving the form ────────────────────────────────────────────────

impl SettingsScreen {
    /// Leave for `to`. When that would drop edits, the app's confirm
    /// dialog asks first, and only a yes goes on.
    pub(super) fn leave(&mut self, to: Leave) -> Option<Action> {
        if self.is_dirty() {
            let question = match &to {
                Leave::Close => "Close Settings?",
                Leave::Profile(_) => "Switch profiles?",
                Leave::NewProfile(_) => "Set up a new profile?",
            };
            let go = match to {
                Leave::Close => Action::CloseSettings,
                Leave::Profile(name) => Action::SettingsShowProfile(name),
                Leave::NewProfile(name) => Action::SetupNewProfile(name),
            };
            return Some(Action::ShowConfirm(
                ConfirmRequest::new(question, go)
                    .detail("The changes you have not saved are dropped."),
            ));
        }
        match to {
            Leave::Close => Some(Action::CloseSettings),
            Leave::NewProfile(name) => Some(Action::SetupNewProfile(name)),
            Leave::Profile(name) => {
                self.switch_to_profile(&name);
                None
            }
        }
    }

    /// Show another saved profile, read fresh from the config file.
    pub(super) fn switch_to_profile(&mut self, name: &str) {
        self.state = SettingsState::Editing;
        let Some(path) = self.config_path.clone() else {
            return;
        };
        match crate::config::load_config_from(&path) {
            Ok(cfg) => {
                self.show_profile(&cfg, name);
                self.focus(SettingsField::Profile);
                self.check_keyring();
            }
            Err(error) => self.message = Some(Message::Error(error.to_string())),
        }
    }

    /// Start naming a new profile.
    pub(super) fn begin_naming(&mut self) {
        self.new_profile.clear();
        self.message = None;
        self.state = SettingsState::Naming;
    }

    /// Take the typed name for a new profile, or say what is wrong with it.
    pub(super) fn finish_naming(&mut self) -> Option<Action> {
        let name = self.new_profile.text().trim().to_owned();
        let problem = if name.is_empty() {
            Some("Give the new profile a name".to_owned())
        } else if !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        {
            Some("A profile name uses letters, digits, '-', '_' and '.'".to_owned())
        } else if self.profiles.contains(&name) {
            Some(format!(
                "There is already a profile named {name}. Pick it with \u{25C2} \u{25B8}"
            ))
        } else {
            None
        };
        if let Some(problem) = problem {
            self.message = Some(Message::Error(problem));
            return None;
        }
        self.leave(Leave::NewProfile(name))
    }
}

// ── Preferences ─────────────────────────────────────────────────────

impl SettingsScreen {
    pub(super) fn toggle_show_donate(&mut self) {
        let show = !self.show_donate;
        if let Err(error) = self.write_default(|defaults| defaults.show_donate = show) {
            self.message = Some(Message::Error(format!(
                "Could not save the sponsor button setting: {error}"
            )));
            return;
        }
        self.show_donate = show;
        if let Some(ref tx) = self.action_tx {
            let _ = tx.send(Action::SetShowDonate(show));
        }
    }

    /// Keep the picked theme for the next launch. The theme is already
    /// showing; a failed write only means it will not be remembered.
    pub(super) fn save_theme_preference(&mut self, theme_id: &str) {
        let theme = theme_id.to_owned();
        if let Err(error) = self.write_default(|defaults| defaults.theme = Some(theme)) {
            self.message = Some(Message::Error(format!(
                "The theme is on for this session, but could not be saved: {error}"
            )));
        }
    }

    /// Change one `[defaults]` setting in the config file.
    fn write_default(
        &self,
        change: impl FnOnce(&mut crate::config::Defaults),
    ) -> Result<(), String> {
        let Some(path) = self.config_path.as_deref() else {
            return Ok(());
        };
        write_default_to(path, change)
    }
}

fn write_default_to(
    path: &Path,
    change: impl FnOnce(&mut crate::config::Defaults),
) -> Result<(), String> {
    let mut cfg = crate::config::load_config_from(path).map_err(|error| error.to_string())?;
    change(&mut cfg.defaults);
    crate::config::save_config_to(path, &cfg).map_err(|error| error.to_string())
}

// ── Connection test, save, apply ────────────────────────────────────

impl SettingsScreen {
    /// Test the connection the form describes. A problem the form can see
    /// without connecting is shown at the field it is in instead.
    pub(super) fn submit(&mut self) {
        if self.draft.auth_mode != AuthMode::Cloud
            && let Some(done) = self.draft.tidy_url()
        {
            self.focus(SettingsField::Url);
            self.message = Some(Message::Notice(format!(
                "{done} Press Enter again to test it."
            )));
            return;
        }
        if let Err(error) = self.draft.validate_complete() {
            self.focus(SettingsField::for_draft(error.field));
            self.message = Some(Message::Error(error.message));
            return;
        }

        self.state = SettingsState::Testing;
        self.message = None;
        self.tested = None;
        let request = ProbeRequest {
            profile: self.draft.to_profile(),
            profile_name: self.profile_name.clone(),
            overrides: self.overrides(),
            totp: self.draft.totp_code(),
        };
        let plan = self.draft.plan_save(&self.profile_name);
        self.probe.start(self.action_tx.as_ref(), request, plan);
    }

    /// Stop a running test. Nothing it finds is used and nothing is saved.
    pub(super) fn cancel_test(&mut self) {
        self.probe.cancel();
        self.state = SettingsState::Editing;
        self.message = Some(Message::Notice("Test cancelled. Nothing was saved.".into()));
    }

    /// Take the result of test `generation`. A result from a test that was
    /// cancelled or replaced is dropped: it must not save, apply, or put
    /// its error on a form that has moved on.
    pub(super) fn finish_test(&mut self, generation: u64, result: &ProbeResult) {
        let Some(tested) = self.probe.claim(generation) else {
            return;
        };
        match result {
            Ok(pass) => {
                self.remember_sites(&pass.sites);
                self.tested = Some(pass.config.clone());
                self.state = SettingsState::Saving;
                self.probe
                    .save(self.action_tx.as_ref(), tested, &pass.config);
            }
            Err(failure) => self.show_failure(failure.clone()),
        }
    }

    /// Take the result of the save that followed test `generation`: on
    /// success, hand the tested connection to the app.
    pub(super) fn finish_save(
        &mut self,
        generation: u64,
        result: &Result<Vec<String>, String>,
    ) -> Option<Action> {
        if self.state != SettingsState::Saving || !self.probe.owns(generation) {
            return None;
        }
        match result {
            Ok(notices) => {
                let config = self.tested.take()?;
                // The flags the test ran under, read before the saved form
                // becomes the one edits are told from.
                let flags = self.overrides();
                self.original = self.draft.clone();
                Some(Action::ApplyConnection {
                    profile_name: self.profile_name.clone(),
                    config,
                    notices: notices.clone(),
                    flags,
                })
            }
            Err(message) => {
                self.tested = None;
                self.show_failure(ProbeFailure::save(message.clone()));
                None
            }
        }
    }

    fn remember_sites(&mut self, sites: &[SiteHint]) {
        if !sites.is_empty() {
            // A test runs with the form as it stands, so these are the
            // sites of the controller the form points at now.
            self.known_sites = sites.to_vec();
            self.sites_for = self.draft.controller_identity();
            self.site_typed = false;
        }
    }

    /// Back to the form with `failure` explained under it and focus on the
    /// field it is about.
    fn show_failure(&mut self, failure: ProbeFailure) {
        self.state = SettingsState::Editing;
        self.remember_sites(&failure.sites);
        if let Some(field) = failure.field {
            self.focus(SettingsField::for_draft(field));
        }
        self.message = Some(Message::Failure(failure));
    }
}
