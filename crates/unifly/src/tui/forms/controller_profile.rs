//! The controller profile a form edits: the draft both the setup wizard
//! and the Settings screen fill in, what it validates, and how it is saved.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use secrecy::{ExposeSecret, SecretString};

use crate::config::{SecretStore, SecretStoreError, api_key_keyring_key, password_keyring_key};
use crate::tui::widgets::line_input::LineInput;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthMode {
    ApiKey,
    Session,
    Hybrid,
    Cloud,
}

impl AuthMode {
    pub(crate) const ALL: [Self; 4] = [Self::ApiKey, Self::Session, Self::Hybrid, Self::Cloud];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::ApiKey => "API Key (Integration + Session HTTP)",
            Self::Session => "Username / Password (Session API)",
            Self::Hybrid => "Hybrid (API Key + Credentials)",
            Self::Cloud => "Cloud (Site Manager API Key + Host ID)",
        }
    }

    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::ApiKey => "Recommended for most setups",
            Self::Session => "Cookie session for live WebSocket events",
            Self::Hybrid => "Full access including live WebSocket",
            Self::Cloud => "Remote console access via api.ui.com",
        }
    }

    pub(crate) fn config_value(self) -> &'static str {
        match self {
            Self::ApiKey => "integration",
            Self::Session => "session",
            Self::Hybrid => "hybrid",
            Self::Cloud => "cloud",
        }
    }

    /// The mode a profile's `auth_mode` names, or `None` for a value
    /// nothing resolves. `legacy` is the older spelling of `session`.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "integration" => Some(Self::ApiKey),
            "session" | "legacy" => Some(Self::Session),
            "hybrid" => Some(Self::Hybrid),
            "cloud" => Some(Self::Cloud),
            _ => None,
        }
    }

    pub(crate) fn index(self) -> usize {
        Self::ALL.iter().position(|mode| *mode == self).unwrap_or(0)
    }

    pub(crate) fn uses_api_key(self) -> bool {
        matches!(self, Self::ApiKey | Self::Hybrid | Self::Cloud)
    }

    /// Whether the mode signs in with a username and password.
    pub(crate) fn uses_login(self) -> bool {
        matches!(self, Self::Session | Self::Hybrid)
    }
}

/// A field of the draft, for moving focus to the one a problem is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftField {
    Url,
    AuthMode,
    ApiKey,
    HostId,
    Username,
    Password,
    Totp,
    Site,
    Insecure,
    CaCert,
    Timeout,
}

impl DraftField {
    /// Whether typed characters are drawn as dots unless revealed.
    pub(crate) fn is_secret(self) -> bool {
        matches!(self, Self::ApiKey | Self::Password)
    }

    /// Whether `ch` may be typed or pasted into this field.
    pub(crate) fn accepts(self, ch: char) -> bool {
        match self {
            Self::Totp | Self::Timeout => ch.is_ascii_digit(),
            Self::Url | Self::ApiKey | Self::HostId | Self::Site => !ch.is_whitespace(),
            Self::AuthMode | Self::Username | Self::Password | Self::Insecure | Self::CaCert => {
                true
            }
        }
    }
}

/// Which secrets a form is showing in the clear.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Reveal {
    api_key: bool,
    password: bool,
}

impl Reveal {
    /// Show or hide `field`, when it is a secret.
    pub(crate) fn toggle(&mut self, field: DraftField) {
        match field {
            DraftField::ApiKey => self.api_key = !self.api_key,
            DraftField::Password => self.password = !self.password,
            _ => {}
        }
    }

    /// Whether `field` is drawn in the clear. Only secrets are ever hidden.
    pub(crate) fn shows(self, field: DraftField) -> bool {
        match field {
            DraftField::ApiKey => self.api_key,
            DraftField::Password => self.password,
            _ => true,
        }
    }
}

/// Why a draft cannot be used yet, and the field to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FieldError {
    pub(crate) field: DraftField,
    pub(crate) message: String,
}

impl FieldError {
    fn new(field: DraftField, message: impl Into<String>) -> Self {
        Self {
            field,
            message: message.into(),
        }
    }
}

/// Where a credential the form edits was stored when the profile loaded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum SecretOrigin {
    /// Not stored yet: a new profile, or a credential its mode never used.
    #[default]
    Unset,
    /// Plaintext in `config.toml`, loaded into the field. It stays there,
    /// even when edited, so saving never migrates it behind the user's back.
    Config,
    /// The OS keyring. The field stays empty and the stored value is used
    /// as is; typing a value replaces the keyring entry.
    Keyring,
}

/// First path segments of the pages a UniFi console serves in a browser.
/// An address that starts with one was copied from the address bar: the
/// API lives at the console's root, not under the page.
const CONSOLE_PAGES: [&str; 8] = [
    "network", "manage", "login", "protect", "access", "talk", "consoles", "setup",
];

/// The address shown as an example where a controller URL is asked for.
pub(crate) const EXAMPLE_URL: &str = "https://192.168.1.1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControllerProfileDraft {
    /// The controller's address. Cloud profiles do not use it: they keep
    /// the connector address they were loaded with.
    pub(crate) url: LineInput,
    pub(crate) auth_mode: AuthMode,
    pub(crate) api_key: LineInput,
    pub(crate) api_key_env: Option<String>,
    pub(crate) host_id: LineInput,
    pub(crate) host_id_env: Option<String>,
    pub(crate) username: LineInput,
    pub(crate) password: LineInput,
    /// A one-time MFA code for the next sign-in. Never saved.
    pub(crate) totp: LineInput,
    pub(crate) site: LineInput,
    pub(crate) insecure: bool,
    /// Whether the user changed the insecure toggle. Until they do, saving
    /// keeps the stored value, including "unset".
    insecure_touched: bool,
    pub(crate) ca_cert: LineInput,
    /// Request timeout in seconds; empty inherits `[defaults]`.
    pub(crate) timeout: LineInput,
    /// What the form does not edit, from the profile it was loaded from.
    /// `None` for a new profile.
    pub(crate) loaded: Option<LoadedProfileFields>,
    pub(crate) api_key_origin: SecretOrigin,
    pub(crate) password_origin: SecretOrigin,
    /// The profile's `auth_mode` when it names no mode unifly knows. The
    /// form then shows API Key, and saving writes that over it.
    pub(crate) unknown_auth_mode: Option<String>,
}

/// What saving a draft writes: the profile for `config.toml`, plus the typed
/// secrets that belong in the keyring instead of in that file.
pub(crate) struct SavePlan {
    profile: crate::config::Profile,
    keyring: Vec<PendingSecret>,
}

struct PendingSecret {
    field: SecretField,
    key: String,
    secret: SecretString,
}

#[derive(Clone, Copy)]
enum SecretField {
    ApiKey,
    Password,
}

impl SecretField {
    fn label(self) -> &'static str {
        match self {
            Self::ApiKey => "API key",
            Self::Password => "password",
        }
    }
}

/// What the keyring step of a save produced: the profile to write, notices
/// about where a secret ended up, and the entries it replaced, kept so a
/// later failure can put them back.
struct StoredSecrets {
    profile: crate::config::Profile,
    warnings: Vec<String>,
    written: Vec<WrittenSecret>,
}

impl SavePlan {
    /// Save the draft: store the pending secrets in `store`, then hand the
    /// resulting profile to `save`, which writes `config.toml`.
    ///
    /// If either step fails, every keyring entry this save wrote is put
    /// back as it was, so a failed save leaves the keyring exactly as it
    /// found it, and the error is returned for the user (it never carries a
    /// secret). On success, returns the notices to show, such as a secret
    /// that fell back to plaintext. Both the settings and the setup screen
    /// save through here.
    pub(crate) fn commit<S>(self, store: &dyn SecretStore, save: S) -> Result<Vec<String>, String>
    where
        S: FnOnce(crate::config::Profile) -> Result<(), String>,
    {
        let stored = self.store_secrets(store)?;
        match save(stored.profile) {
            Ok(()) => Ok(stored.warnings),
            Err(message) => Err(blocked(message, store, &stored.written)),
        }
    }

    /// Store the pending secrets in `store`.
    ///
    /// When no keyring is usable, a secret falls back to plaintext in the
    /// profile (the documented fallback for hosts without one), with a
    /// warning naming the field, and the keyring is not touched. A keyring
    /// that is there but refuses a secret, or holds an entry that cannot
    /// be read back (so a rollback could not restore it), stops the save:
    /// that entry is left alone, entries this save already wrote are put
    /// back as they were, and the error names the field (never the
    /// secret).
    fn store_secrets(self, store: &dyn SecretStore) -> Result<StoredSecrets, String> {
        let mut profile = self.profile;
        let mut warnings = Vec::new();
        let mut written = Vec::new();
        for pending in self.keyring {
            let label = pending.field.label();
            let previous = match store.get(&pending.key) {
                Ok(previous) => previous.map(SecretString::from),
                Err(SecretStoreError::Unavailable(reason)) => {
                    fall_back(&mut profile, &mut warnings, &pending, &reason);
                    continue;
                }
                Err(SecretStoreError::Rejected(reason)) => {
                    return Err(blocked(
                        format!(
                            "Couldn't read the {label} already in the keyring ({reason}), so it \
                             was not replaced; config.toml was not changed"
                        ),
                        store,
                        &written,
                    ));
                }
            };
            match store.set(&pending.key, pending.secret.expose_secret()) {
                Ok(()) => written.push(WrittenSecret {
                    label,
                    key: pending.key,
                    previous,
                }),
                Err(SecretStoreError::Unavailable(reason)) => {
                    fall_back(&mut profile, &mut warnings, &pending, &reason);
                }
                Err(SecretStoreError::Rejected(reason)) => {
                    return Err(blocked(
                        format!(
                            "Couldn't store the {label} in the keyring ({reason}); config.toml \
                             was not changed"
                        ),
                        store,
                        &written,
                    ));
                }
            }
        }
        Ok(StoredSecrets {
            profile,
            warnings,
            written,
        })
    }
}

/// Save `plan` for `profile_name`: its secrets go to `store` and its
/// profile into the config file at `path`. A refused secret or a failed
/// config write undoes the keyring changes and comes back as the message
/// to show; success returns the notices to show.
pub(crate) fn save_to(
    plan: SavePlan,
    store: &dyn SecretStore,
    path: &Path,
    profile_name: &str,
) -> Result<Vec<String>, String> {
    plan.commit(store, |profile| {
        write_profile(path, profile_name, profile)
            .map_err(|error| format!("Connected, but failed to save config: {error}"))
    })
}

/// Write `profile` into the config file at `path`, keeping every other
/// profile and setting in it. It becomes the default only when the config
/// has no default naming a saved profile, which is how a fresh config
/// (whose default names a profile that does not exist yet) gets one.
pub(crate) fn write_profile(
    path: &Path,
    profile_name: &str,
    profile: crate::config::Profile,
) -> Result<(), crate::config::ConfigError> {
    let mut cfg = crate::config::load_config_from(path)?;
    cfg.profiles.insert(profile_name.to_owned(), profile);
    if !cfg
        .default_profile
        .as_ref()
        .is_some_and(|name| cfg.profiles.contains_key(name))
    {
        cfg.default_profile = Some(profile_name.to_owned());
    }
    crate::config::save_config_to(path, &cfg)
}

/// The documented fallback for a host without a usable keyring: keep the
/// secret in the profile, in plaintext, and say so.
fn fall_back(
    profile: &mut crate::config::Profile,
    warnings: &mut Vec<String>,
    pending: &PendingSecret,
    reason: &str,
) {
    let label = pending.field.label();
    warnings.push(format!(
        "No usable keyring ({reason}); saved the {label} to config.toml in plaintext"
    ));
    let plaintext = Some(pending.secret.expose_secret().to_owned());
    match pending.field {
        SecretField::ApiKey => profile.api_key = plaintext,
        SecretField::Password => profile.password = plaintext,
    }
}

/// Undo a blocked save's keyring writes and finish its error `message`,
/// naming any entry that could not be put back.
fn blocked(mut message: String, store: &dyn SecretStore, written: &[WrittenSecret]) -> String {
    for (restored, error) in roll_back(store, written) {
        let _ = write!(
            message,
            ", and the {restored} it had already stored could not be put back ({error})"
        );
    }
    message
}

/// A keyring entry a save has replaced, and what it held before.
struct WrittenSecret {
    label: &'static str,
    key: String,
    previous: Option<SecretString>,
}

/// Put back entries a blocked save already wrote, newest first: restore
/// the previous value, or delete the entry if there was none. Returns the
/// label and error for each entry that could not be put back.
fn roll_back(
    store: &dyn SecretStore,
    written: &[WrittenSecret],
) -> Vec<(&'static str, SecretStoreError)> {
    written
        .iter()
        .rev()
        .filter_map(|entry| {
            let restored = match &entry.previous {
                Some(previous) => store.set(&entry.key, previous.expose_secret()),
                None => store.delete(&entry.key),
            };
            restored.err().map(|error| (entry.label, error))
        })
        .collect()
}

/// Profile settings the form has no field for, kept from the loaded
/// profile so saving writes them back unchanged instead of erasing them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LoadedProfileFields {
    totp_env: Option<String>,
    totp_command: Option<Vec<String>>,
    /// The stored `insecure`, which may be unset (inherit `[defaults]`).
    /// An untouched toggle writes this back, so "unset" stays unset.
    insecure: Option<bool>,
    /// The connector address of a profile that was loaded as a cloud
    /// profile, which may be empty (the default connector).
    cloud_controller: Option<String>,
}

impl Default for ControllerProfileDraft {
    fn default() -> Self {
        Self {
            url: LineInput::default(),
            auth_mode: AuthMode::ApiKey,
            api_key: LineInput::default(),
            api_key_env: None,
            host_id: LineInput::default(),
            host_id_env: None,
            username: LineInput::default(),
            password: LineInput::default(),
            totp: LineInput::default(),
            site: LineInput::new("default"),
            insecure: false,
            insecure_touched: false,
            ca_cert: LineInput::default(),
            timeout: LineInput::default(),
            loaded: None,
            api_key_origin: SecretOrigin::Unset,
            password_origin: SecretOrigin::Unset,
            unknown_auth_mode: None,
        }
    }
}

impl ControllerProfileDraft {
    pub(crate) fn from_profile(profile: &crate::config::Profile) -> Self {
        let auth_mode = AuthMode::parse(&profile.auth_mode);
        let is_cloud = auth_mode == Some(AuthMode::Cloud);
        let text = |value: &Option<String>| LineInput::new(value.clone().unwrap_or_default());
        Self {
            url: LineInput::new(if is_cloud {
                ""
            } else {
                profile.controller.as_str()
            }),
            auth_mode: auth_mode.unwrap_or(AuthMode::ApiKey),
            api_key: text(&profile.api_key),
            api_key_env: profile.api_key_env.clone(),
            host_id: text(&profile.host_id),
            host_id_env: profile.host_id_env.clone(),
            username: text(&profile.username),
            password: text(&profile.password),
            totp: LineInput::default(),
            site: LineInput::new(profile.site.as_str()),
            insecure: !is_cloud && profile.insecure.unwrap_or(false),
            insecure_touched: false,
            ca_cert: LineInput::new(
                profile
                    .ca_cert
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
            ),
            timeout: LineInput::new(
                profile
                    .timeout
                    .map(|secs| secs.to_string())
                    .unwrap_or_default(),
            ),
            loaded: Some(LoadedProfileFields {
                totp_env: profile.totp_env.clone(),
                totp_command: profile.totp_command.clone(),
                insecure: profile.insecure,
                cloud_controller: is_cloud.then(|| profile.controller.clone()),
            }),
            api_key_origin: if profile.api_key.is_some() {
                SecretOrigin::Config
            } else {
                SecretOrigin::Unset
            },
            password_origin: if profile.password.is_some() {
                SecretOrigin::Config
            } else {
                SecretOrigin::Unset
            },
            unknown_auth_mode: auth_mode.is_none().then(|| profile.auth_mode.clone()),
        }
    }

    /// Mark credentials that `profile_name` keeps in `store` as present.
    /// A plaintext value in the profile wins, as it does when credentials
    /// are resolved, so only a credential absent from the profile can come
    /// from the keyring.
    pub(crate) fn with_keyring_origins(
        mut self,
        profile_name: &str,
        store: &dyn SecretStore,
    ) -> Self {
        let (api_key, password) = keyring_holds(profile_name, store);
        self.note_keyring(api_key, password);
        self
    }

    /// Record which credentials the keyring holds for this profile.
    pub(crate) fn note_keyring(&mut self, api_key: bool, password: bool) {
        if self.api_key_origin == SecretOrigin::Unset && api_key {
            self.api_key_origin = SecretOrigin::Keyring;
        }
        if self.password_origin == SecretOrigin::Unset && password {
            self.password_origin = SecretOrigin::Keyring;
        }
    }

    /// Show what an unset `insecure` actually resolves to, so the toggle
    /// tells the truth: a profile CA means verification, otherwise
    /// `[defaults].insecure` applies. An explicit value is shown as is.
    pub(crate) fn with_inherited_insecure(mut self, defaults: &crate::config::Defaults) -> Self {
        if self.auth_mode != AuthMode::Cloud
            && let Some(loaded) = &self.loaded
            && loaded.insecure.is_none()
        {
            self.insecure = self.ca_cert.is_empty() && defaults.insecure;
        }
        self
    }

    /// Flip the insecure toggle. From then on saving writes it explicitly.
    pub(crate) fn toggle_insecure(&mut self) {
        self.insecure = !self.insecure;
        self.insecure_touched = true;
    }

    /// The `insecure` value to save: the stored one (possibly unset) until
    /// the toggle is touched, the toggle's value after that. A new profile
    /// always saves the toggle. A cloud profile never verifies less than
    /// the system does, so it only carries what it was loaded with.
    fn insecure_to_save(&self) -> Option<bool> {
        match &self.loaded {
            Some(loaded) if !self.insecure_touched || self.auth_mode == AuthMode::Cloud => {
                loaded.insecure
            }
            None if self.auth_mode == AuthMode::Cloud => None,
            _ => Some(self.insecure),
        }
    }

    /// The text field behind `field`, when it is one.
    pub(crate) fn input(&self, field: DraftField) -> Option<&LineInput> {
        match field {
            DraftField::Url => Some(&self.url),
            DraftField::ApiKey => Some(&self.api_key),
            DraftField::HostId => Some(&self.host_id),
            DraftField::Username => Some(&self.username),
            DraftField::Password => Some(&self.password),
            DraftField::Totp => Some(&self.totp),
            DraftField::Site => Some(&self.site),
            DraftField::CaCert => Some(&self.ca_cert),
            DraftField::Timeout => Some(&self.timeout),
            DraftField::AuthMode | DraftField::Insecure => None,
        }
    }

    pub(crate) fn input_mut(&mut self, field: DraftField) -> Option<&mut LineInput> {
        match field {
            DraftField::Url => Some(&mut self.url),
            DraftField::ApiKey => Some(&mut self.api_key),
            DraftField::HostId => Some(&mut self.host_id),
            DraftField::Username => Some(&mut self.username),
            DraftField::Password => Some(&mut self.password),
            DraftField::Totp => Some(&mut self.totp),
            DraftField::Site => Some(&mut self.site),
            DraftField::CaCert => Some(&mut self.ca_cert),
            DraftField::Timeout => Some(&mut self.timeout),
            DraftField::AuthMode | DraftField::Insecure => None,
        }
    }

    /// The address this draft connects to, for naming it on screen.
    pub(crate) fn target(&self) -> String {
        if self.auth_mode != AuthMode::Cloud {
            return self.url.text().trim().to_owned();
        }
        let saved = self.cloud_controller();
        if saved.trim().is_empty() {
            crate::config::DEFAULT_CLOUD_CONTROLLER_URL.into()
        } else {
            saved
        }
    }

    /// The connector address a cloud profile saves: the one it was loaded
    /// with, which may be empty (the default connector), and the default
    /// connector for a profile that is only now becoming a cloud profile.
    fn cloud_controller(&self) -> String {
        self.loaded
            .as_ref()
            .and_then(|loaded| loaded.cloud_controller.clone())
            .unwrap_or_else(|| crate::config::DEFAULT_CLOUD_CONTROLLER_URL.into())
    }

    /// The `controller` a profile saved from this draft holds.
    fn controller_to_save(&self) -> String {
        if self.auth_mode == AuthMode::Cloud {
            self.cloud_controller()
        } else {
            self.url.text().trim().to_owned()
        }
    }

    /// What identifies the controller a list of sites was read from. When
    /// it changes, that list no longer describes what the form points at.
    pub(crate) fn controller_identity(&self) -> String {
        format!(
            "{}|{}|{}",
            self.auth_mode.config_value(),
            self.target(),
            self.host_id.text().trim()
        )
    }

    /// Where MFA codes come from besides the one-time code field, when
    /// the profile names a source.
    pub(crate) fn mfa_source(&self) -> Option<String> {
        let loaded = self.loaded.as_ref()?;
        if let Some(argv) = &loaded.totp_command {
            return Some(format!("command: {}", argv.join(" ")));
        }
        loaded
            .totp_env
            .as_ref()
            .map(|name| format!("${name}, read at launch"))
    }

    /// The one-time code typed for the next sign-in, when the mode signs in.
    pub(crate) fn totp_code(&self) -> Option<SecretString> {
        let code = self.totp.text().trim();
        (self.auth_mode.uses_login() && !code.is_empty())
            .then(|| SecretString::from(code.to_owned()))
    }

    /// Correct the two common slips in a typed address, and say what was
    /// done: `https://` goes in front of one typed without a scheme, and
    /// a page address copied from the browser is cut back to the
    /// controller it is on. Returns `None` when the address was left
    /// alone.
    pub(crate) fn tidy_url(&mut self) -> Option<&'static str> {
        let typed = self.url.text().trim().to_owned();
        if typed.is_empty() {
            return None;
        }
        if !typed.contains("://") {
            let candidate = format!("https://{typed}");
            if !is_controller_url(&candidate) {
                return None;
            }
            self.url.set(candidate);
            return Some("Added https:// to the address.");
        }
        let url = typed.parse::<url::Url>().ok()?;
        let page = url
            .path_segments()
            .and_then(|mut segments| segments.next())
            .is_some_and(|first| CONSOLE_PAGES.contains(&first));
        if !is_controller_url(&typed) || !page {
            return None;
        }
        let origin = url.origin().ascii_serialization();
        self.url.set(origin);
        Some("Kept only the controller's address, without the page it was copied from.")
    }

    /// Set the insecure toggle to `insecure`, as a choice made for this
    /// profile.
    pub(crate) fn set_insecure(&mut self, insecure: bool) {
        self.insecure = insecure;
        self.insecure_touched = true;
    }

    pub(crate) fn validate_url(&self) -> Result<(), FieldError> {
        let trimmed = self.url.text().trim();
        let problem = if trimmed.is_empty() {
            format!("Enter the controller's address, for example {EXAMPLE_URL}")
        } else if is_controller_url(trimmed) {
            return Ok(());
        } else if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
            "The address must start with https:// or http://".into()
        } else {
            format!("The address needs a host, for example {EXAMPLE_URL}")
        };
        Err(FieldError::new(DraftField::Url, problem))
    }

    pub(crate) fn validate_credentials(&self) -> Result<(), FieldError> {
        let mode = self.auth_mode;
        if mode.uses_api_key() && !self.has_api_key() {
            return Err(FieldError::new(
                DraftField::ApiKey,
                "API key cannot be empty",
            ));
        }
        if mode == AuthMode::Cloud && !self.has_host_id() {
            return Err(FieldError::new(
                DraftField::HostId,
                "Host ID cannot be empty",
            ));
        }
        if mode.uses_login() {
            if self.username.text().trim().is_empty() {
                return Err(FieldError::new(
                    DraftField::Username,
                    "Username cannot be empty",
                ));
            }
            if !self.has_password() {
                return Err(FieldError::new(
                    DraftField::Password,
                    "Password cannot be empty",
                ));
            }
            let code = self.totp.text().trim();
            if !code.is_empty() && code.len() != 6 {
                return Err(FieldError::new(
                    DraftField::Totp,
                    "A one-time code is 6 digits",
                ));
            }
        }
        Ok(())
    }

    fn has_password(&self) -> bool {
        !self.password.is_empty() || self.password_origin == SecretOrigin::Keyring
    }

    fn has_api_key(&self) -> bool {
        !self.api_key.text().trim().is_empty()
            || self.api_key_origin == SecretOrigin::Keyring
            || self
                .api_key_env
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
    }

    fn has_host_id(&self) -> bool {
        !self.host_id.text().trim().is_empty()
            || self
                .host_id_env
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
    }

    pub(crate) fn validate_complete(&self) -> Result<(), FieldError> {
        if self.auth_mode != AuthMode::Cloud {
            self.validate_url()?;
        }
        self.validate_credentials()?;

        if self.site.text().trim().is_empty() {
            return Err(FieldError::new(
                DraftField::Site,
                "Site name cannot be empty",
            ));
        }
        if self.auth_mode != AuthMode::Cloud
            && let Some(path) = self.ca_cert_path()
            && !path.is_file()
        {
            return Err(FieldError::new(
                DraftField::CaCert,
                "No certificate file at that path",
            ));
        }
        let timeout = self.timeout.text().trim();
        if !timeout.is_empty() && !timeout.parse::<u64>().is_ok_and(|secs| secs > 0) {
            return Err(FieldError::new(
                DraftField::Timeout,
                "Timeout is a number of seconds, at least 1",
            ));
        }

        Ok(())
    }

    fn ca_cert_path(&self) -> Option<PathBuf> {
        let typed = self.ca_cert.text().trim();
        (!typed.is_empty()).then(|| PathBuf::from(typed))
    }

    /// What the form shows next to a credential it is not displaying:
    /// where the value in use comes from.
    pub(crate) fn secret_source(&self, field: DraftField) -> Option<String> {
        match field {
            DraftField::ApiKey if self.api_key.is_empty() => {
                if let Some(name) = self.api_key_env.as_deref().filter(|name| !name.is_empty()) {
                    return Some(env_source(name));
                }
                (self.api_key_origin == SecretOrigin::Keyring)
                    .then(|| "stored in keyring; type to replace".to_owned())
            }
            DraftField::Password if self.password.is_empty() => (self.password_origin
                == SecretOrigin::Keyring)
                .then(|| "stored in keyring; type to replace".to_owned()),
            DraftField::HostId if self.host_id.is_empty() => self
                .host_id_env
                .as_deref()
                .filter(|name| !name.is_empty())
                .map(env_source),
            _ => None,
        }
    }

    /// The profile to connect with: typed credentials inline, and those
    /// left in the keyring absent, so credential resolution reads them from
    /// there. Use [`Self::plan_save`] for what goes to `config.toml`.
    pub(crate) fn to_profile(&self) -> crate::config::Profile {
        let loaded = self.loaded.clone().unwrap_or_default();
        let mode = self.auth_mode;
        let is_cloud = mode == AuthMode::Cloud;
        let typed = |input: &LineInput| {
            let value = input.text().trim();
            (!value.is_empty()).then(|| value.to_owned())
        };
        let api_key = mode.uses_api_key().then(|| typed(&self.api_key)).flatten();
        let host_id = is_cloud.then(|| typed(&self.host_id)).flatten();
        crate::config::Profile {
            controller: self.controller_to_save(),
            site: self.site.text().trim().to_string(),
            auth_mode: mode.config_value().to_string(),
            api_key_env: (mode.uses_api_key() && api_key.is_none())
                .then(|| self.api_key_env.clone())
                .flatten(),
            api_key,
            host_id_env: (is_cloud && host_id.is_none())
                .then(|| self.host_id_env.clone())
                .flatten(),
            host_id,
            username: mode
                .uses_login()
                .then(|| self.username.text().trim().to_string()),
            password: (mode.uses_login() && !self.password.is_empty())
                .then(|| self.password.text().to_owned()),
            totp_env: loaded.totp_env,
            totp_command: loaded.totp_command,
            ca_cert: self.ca_cert_path(),
            insecure: self.insecure_to_save(),
            timeout: self.timeout.text().trim().parse().ok(),
        }
    }

    /// What saving writes for `profile_name`. A typed credential goes to the
    /// keyring, as `config init` stores it, unless the profile already kept
    /// it in plaintext, which is left where it was. A credential still in
    /// the keyring and not retyped is written nowhere: the entry stays as
    /// it is.
    pub(crate) fn plan_save(&self, profile_name: &str) -> SavePlan {
        let mut profile = self.to_profile();
        let mut keyring = Vec::new();
        if self.api_key_origin != SecretOrigin::Config
            && let Some(secret) = profile.api_key.take()
        {
            keyring.push(PendingSecret {
                field: SecretField::ApiKey,
                key: api_key_keyring_key(profile_name),
                secret: SecretString::from(secret),
            });
        }
        if self.password_origin != SecretOrigin::Config
            && let Some(secret) = profile.password.take()
        {
            keyring.push(PendingSecret {
                field: SecretField::Password,
                key: password_keyring_key(profile_name),
                secret: SecretString::from(secret),
            });
        }
        SavePlan { profile, keyring }
    }
}

/// Whether the keyring holds an API key and a password for `profile_name`.
pub(crate) fn keyring_holds(profile_name: &str, store: &dyn SecretStore) -> (bool, bool) {
    let holds = |key: String| matches!(store.get(&key), Ok(Some(_)));
    (
        holds(api_key_keyring_key(profile_name)),
        holds(password_keyring_key(profile_name)),
    )
}

/// Whether `text` is an http(s) URL with a host.
fn is_controller_url(text: &str) -> bool {
    text.parse::<url::Url>().is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https") && url.host_str().is_some_and(|h| !h.is_empty())
    })
}

/// How a value read from the environment variable `name` is described.
fn env_source(name: &str) -> String {
    if std::env::var_os(name).is_some() {
        format!("from ${name}; type to replace")
    } else {
        format!("${name} is not set")
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthMode, ControllerProfileDraft, DraftField, SecretOrigin};
    use crate::tui::widgets::line_input::LineInput;

    fn sample_draft() -> ControllerProfileDraft {
        ControllerProfileDraft {
            url: LineInput::new("https://console.example.com"),
            auth_mode: AuthMode::Hybrid,
            api_key: LineInput::new("api-key"),
            username: LineInput::new("bliss"),
            password: LineInput::new("hunter2"),
            ..ControllerProfileDraft::default()
        }
    }

    fn message(result: Result<(), super::FieldError>) -> Option<String> {
        result.err().map(|error| error.message)
    }

    #[test]
    fn a_url_needs_a_scheme_and_a_host() {
        let mut draft = sample_draft();
        for (typed, problem) in [
            ("", "Enter the controller's address"),
            ("not a url", "must start with https:// or http://"),
            ("unifi.local:8443", "must start with https:// or http://"),
            ("ftp://unifi.local", "must start with https:// or http://"),
            ("https://", "needs a host"),
        ] {
            draft.url.set(typed);
            let error = draft.validate_complete().expect_err(typed);
            assert_eq!(error.field, DraftField::Url, "{typed}");
            assert!(
                error.message.contains(problem),
                "{typed}: {}",
                error.message
            );
        }

        for typed in ["https://192.168.1.1", "http://unifi.local:8080/"] {
            draft.url.set(typed);
            assert_eq!(draft.validate_complete(), Ok(()), "{typed}");
        }
    }

    /// An address typed without a scheme is the common slip: the form
    /// offers the https:// form instead of only refusing it.
    #[test]
    fn a_missing_scheme_is_offered_as_https() {
        let mut draft = sample_draft();
        for (typed, fixed) in [
            ("192.168.1.1", "https://192.168.1.1"),
            ("unifi.local:8443", "https://unifi.local:8443"),
            ("localhost:8443", "https://localhost:8443"),
        ] {
            draft.url.set(typed);
            assert!(draft.tidy_url().is_some(), "{typed}");
            assert_eq!(draft.url.text(), fixed);
            assert_eq!(draft.validate_url(), Ok(()));
        }

        for typed in ["", "ftp://unifi.local", "https://unifi.local", "not a url"] {
            draft.url.set(typed);
            assert!(draft.tidy_url().is_none(), "{typed}");
            assert_eq!(draft.url.text(), typed);
        }
    }

    /// An address pasted from the browser carries the page it was on. The
    /// API is at the console's root, so the page is cut off; a path that
    /// is not a console page (a reverse proxy prefix) is left alone.
    #[test]
    fn a_pasted_page_address_is_cut_back_to_the_controller() {
        let mut draft = sample_draft();
        for (typed, fixed) in [
            (
                "https://192.168.1.1/network/default/dashboard",
                "https://192.168.1.1",
            ),
            (
                "https://unifi.local:8443/manage/site/default",
                "https://unifi.local:8443",
            ),
        ] {
            draft.url.set(typed);
            assert!(draft.tidy_url().is_some(), "{typed}");
            assert_eq!(draft.url.text(), fixed);
        }

        for typed in ["https://proxy.example.com/unifi", "https://192.168.1.1/"] {
            draft.url.set(typed);
            assert!(draft.tidy_url().is_none(), "{typed}");
            assert_eq!(draft.url.text(), typed);
        }
    }

    #[test]
    fn validate_credentials_requires_password_for_hybrid() {
        let mut draft = sample_draft();
        draft.password.clear();

        let error = draft.validate_credentials().expect_err("no password");
        assert_eq!(error.field, DraftField::Password);
        assert_eq!(error.message, "Password cannot be empty");
    }

    #[test]
    fn a_one_time_code_must_be_six_digits_and_is_never_saved() {
        let mut draft = sample_draft();
        draft.totp.set("12345");
        let error = draft.validate_credentials().expect_err("short code");
        assert_eq!(error.field, DraftField::Totp);

        draft.totp.set("123456");
        assert_eq!(draft.validate_credentials(), Ok(()));
        assert!(draft.totp_code().is_some());
        assert!(!toml_of(&draft.to_profile()).contains("123456"));

        // An API key profile never signs in, so it has no use for a code.
        draft.auth_mode = AuthMode::ApiKey;
        assert!(draft.totp_code().is_none());
    }

    /// The certificate path and the timeout are edited in the form, so
    /// what is typed there is what gets saved.
    #[test]
    fn the_certificate_and_timeout_fields_are_saved() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ca = dir.path().join("ca.pem");
        std::fs::write(&ca, "pem").expect("write ca");
        let mut draft = sample_draft();

        draft.ca_cert.set(ca.display().to_string());
        draft.timeout.set("45");
        assert_eq!(draft.validate_complete(), Ok(()));
        let saved = draft.to_profile();
        assert_eq!(saved.ca_cert.as_deref(), Some(ca.as_path()));
        assert_eq!(saved.timeout, Some(45));

        draft.timeout.set("0");
        let error = draft.validate_complete().expect_err("zero timeout");
        assert_eq!(error.field, DraftField::Timeout);

        draft.timeout.clear();
        draft
            .ca_cert
            .set(dir.path().join("missing.pem").display().to_string());
        let error = draft.validate_complete().expect_err("missing file");
        assert_eq!(error.field, DraftField::CaCert);

        draft.ca_cert.clear();
        let saved = draft.to_profile();
        assert_eq!(saved.ca_cert, None);
        assert_eq!(saved.timeout, None, "empty inherits [defaults]");
    }

    /// `legacy` is the older name for `session`; loading it as anything
    /// else would drop the username and password on the next save.
    #[test]
    fn a_legacy_profile_loads_as_session_and_an_unknown_mode_is_reported() {
        let mut profile = hybrid_profile(None, Some("p"));
        profile.auth_mode = "legacy".into();
        let draft = ControllerProfileDraft::from_profile(&profile);
        assert_eq!(draft.auth_mode, AuthMode::Session);
        assert_eq!(draft.unknown_auth_mode, None);
        let saved = draft.to_profile();
        assert_eq!(saved.auth_mode, "session");
        assert_eq!(saved.username.as_deref(), Some("bliss"));
        assert_eq!(saved.password.as_deref(), Some("p"));

        profile.auth_mode = "sesion".into();
        let draft = ControllerProfileDraft::from_profile(&profile);
        assert_eq!(draft.auth_mode, AuthMode::ApiKey);
        assert_eq!(draft.unknown_auth_mode.as_deref(), Some("sesion"));
    }

    /// Passing through Cloud on the way to another mode must leave the
    /// address and the TLS choice exactly as they were.
    #[test]
    fn choosing_cloud_and_leaving_it_changes_nothing() {
        let mut profile = hybrid_profile(Some("k"), Some("p"));
        profile.insecure = Some(true);
        let original = ControllerProfileDraft::from_profile(&profile);
        let mut draft = original.clone();

        draft.auth_mode = AuthMode::Cloud;
        let cloud = draft.to_profile();
        assert_eq!(
            cloud.controller,
            crate::config::DEFAULT_CLOUD_CONTROLLER_URL
        );
        assert_eq!(cloud.insecure, Some(true), "what the file held is kept");

        draft.auth_mode = AuthMode::Hybrid;
        assert_eq!(draft, original);
        assert_eq!(toml_of(&draft.to_profile()), toml_of(&profile));

        // A new profile set up as cloud asks for no TLS exception.
        let mut fresh = ControllerProfileDraft::default();
        fresh.toggle_insecure();
        fresh.auth_mode = AuthMode::Cloud;
        assert_eq!(fresh.to_profile().insecure, None);
        fresh.auth_mode = AuthMode::ApiKey;
        assert_eq!(fresh.to_profile().insecure, Some(true));
    }

    #[test]
    fn a_credential_not_shown_says_where_it_comes_from() {
        let store = MemoryStore::with(&[("home/password", "p")]);
        let mut profile = hybrid_profile(None, None);
        profile.api_key_env = Some("UNIFLY_UNSET_KEY".into());
        let mut draft =
            ControllerProfileDraft::from_profile(&profile).with_keyring_origins("home", &store);

        let key = draft.secret_source(DraftField::ApiKey).expect("env source");
        assert!(
            key.contains("$UNIFLY_UNSET_KEY") && key.contains("not set"),
            "{key}"
        );
        let password = draft
            .secret_source(DraftField::Password)
            .expect("keyring source");
        assert!(password.contains("stored in keyring"), "{password}");

        // A typed value is what gets used, so nothing else is claimed.
        draft.password.set("typed");
        assert_eq!(draft.secret_source(DraftField::Password), None);
    }

    #[test]
    fn to_profile_omits_non_selected_auth_fields() {
        let mut draft = sample_draft();
        draft.auth_mode = AuthMode::ApiKey;

        let profile = draft.to_profile();

        assert_eq!(profile.api_key.as_deref(), Some("api-key"));
        assert_eq!(profile.username, None);
        assert_eq!(profile.password, None);
    }

    #[test]
    fn cloud_profile_round_trips_host_id_and_defaults_url() {
        let profile = crate::config::Profile {
            controller: String::new(),
            site: "default".into(),
            auth_mode: "cloud".into(),
            api_key: Some("cloud-key".into()),
            api_key_env: None,
            host_id: Some("console-123".into()),
            host_id_env: None,
            username: None,
            password: None,
            totp_env: None,
            totp_command: None,
            ca_cert: None,
            insecure: Some(true),
            timeout: None,
        };

        let draft = ControllerProfileDraft::from_profile(&profile);
        let saved = draft.to_profile();

        assert_eq!(draft.auth_mode, AuthMode::Cloud);
        assert_eq!(draft.target(), crate::config::DEFAULT_CLOUD_CONTROLLER_URL);
        assert_eq!(draft.host_id.text(), "console-123");
        assert_eq!(saved.controller, "", "an unset connector stays unset");
        assert_eq!(saved.auth_mode, "cloud");
        assert_eq!(saved.host_id.as_deref(), Some("console-123"));
    }

    #[test]
    fn env_backed_cloud_profile_validates_and_preserves_env_fields() {
        let profile = crate::config::Profile {
            controller: String::new(),
            site: "default".into(),
            auth_mode: "cloud".into(),
            api_key: None,
            api_key_env: Some("UNIFI_CLOUD_API_KEY".into()),
            host_id: None,
            host_id_env: Some("UNIFI_HOST_ID".into()),
            username: None,
            password: None,
            totp_env: None,
            totp_command: None,
            ca_cert: None,
            insecure: Some(true),
            timeout: None,
        };

        let draft = ControllerProfileDraft::from_profile(&profile);
        let saved = draft.to_profile();

        assert_eq!(draft.validate_complete(), Ok(()));
        assert_eq!(saved.api_key, None);
        assert_eq!(saved.api_key_env.as_deref(), Some("UNIFI_CLOUD_API_KEY"));
        assert_eq!(saved.host_id, None);
        assert_eq!(saved.host_id_env.as_deref(), Some("UNIFI_HOST_ID"));
    }

    /// Saving from the settings form must write back every profile field
    /// the form does not show, exactly as loaded.
    #[test]
    fn saving_keeps_every_field_the_form_does_not_show() {
        for insecure in [None, Some(false), Some(true)] {
            let profile = crate::config::Profile {
                controller: "https://192.168.1.1".into(),
                site: "default".into(),
                auth_mode: "hybrid".into(),
                api_key: Some("api-key".into()),
                api_key_env: None,
                host_id: None,
                host_id_env: None,
                username: Some("bliss".into()),
                password: Some("hunter2".into()),
                totp_env: Some("UNIFI_TOTP".into()),
                totp_command: Some(vec![
                    "op".into(),
                    "item".into(),
                    "get".into(),
                    "UniFi".into(),
                    "--otp".into(),
                ]),
                ca_cert: Some("/etc/ssl/certs/unifi-ca.pem".into()),
                insecure,
                timeout: Some(45),
            };

            let saved = ControllerProfileDraft::from_profile(&profile).to_profile();

            assert_eq!(
                toml::to_string(&saved).expect("profile serializes"),
                toml::to_string(&profile).expect("profile serializes"),
                "insecure = {insecure:?}"
            );
        }
    }

    #[test]
    fn a_touched_insecure_toggle_is_saved_explicitly() {
        let mut draft = sample_draft();
        draft.loaded = Some(super::LoadedProfileFields::default());
        assert_eq!(draft.to_profile().insecure, None, "untouched keeps unset");

        draft.toggle_insecure();
        assert_eq!(draft.to_profile().insecure, Some(true));

        // Back to the starting value, but the user chose it: explicit.
        draft.toggle_insecure();
        assert_eq!(draft.to_profile().insecure, Some(false));

        draft.loaded = None;
        assert_eq!(
            draft.to_profile().insecure,
            Some(false),
            "a new profile always saves the toggle"
        );
    }

    /// With `insecure` unset and `[defaults].insecure = true`, the toggle
    /// shows the inherited "on", and switching it off must save an explicit
    /// `false`; saving "unset" would leave TLS verification disabled.
    #[test]
    fn an_unset_insecure_shows_and_overrides_the_inherited_value() {
        let defaults = crate::config::Defaults {
            insecure: true,
            ..crate::config::Defaults::default()
        };
        let mut profile = hybrid_profile(Some("k"), Some("p"));
        profile.insecure = None;

        let mut draft =
            ControllerProfileDraft::from_profile(&profile).with_inherited_insecure(&defaults);
        assert!(draft.insecure, "shows the inherited value");
        assert_eq!(
            draft.to_profile().insecure,
            None,
            "untouched stays inherited"
        );

        draft.toggle_insecure();
        assert!(!draft.insecure);
        assert_eq!(draft.to_profile().insecure, Some(false));

        // A profile CA means verification, whatever [defaults] says.
        profile.ca_cert = Some("/etc/ssl/certs/unifi-ca.pem".into());
        let draft =
            ControllerProfileDraft::from_profile(&profile).with_inherited_insecure(&defaults);
        assert!(!draft.insecure);
    }

    /// In-memory stand-in for the OS keyring. `failure`, when set, is what
    /// a write returns instead of storing, and `read_failure` what a read
    /// returns: for `failing_key` only, when that is set, otherwise for
    /// every key.
    #[derive(Default)]
    struct MemoryStore {
        entries: std::cell::RefCell<std::collections::HashMap<String, String>>,
        failure: Option<crate::config::SecretStoreError>,
        read_failure: Option<crate::config::SecretStoreError>,
        failing_key: Option<&'static str>,
    }

    impl MemoryStore {
        fn with(entries: &[(&str, &str)]) -> Self {
            let store = Self::default();
            for (key, secret) in entries {
                store
                    .entries
                    .borrow_mut()
                    .insert((*key).to_owned(), (*secret).to_owned());
            }
            store
        }

        fn secret(&self, key: &str) -> Option<String> {
            self.entries.borrow().get(key).cloned()
        }
    }

    impl crate::config::SecretStore for MemoryStore {
        fn get(&self, key: &str) -> Result<Option<String>, crate::config::SecretStoreError> {
            if let Some(failure) = &self.read_failure
                && self.failing_key.is_none_or(|failing| failing == key)
            {
                return Err(failure.clone());
            }
            Ok(self.secret(key))
        }

        fn set(&self, key: &str, secret: &str) -> Result<(), crate::config::SecretStoreError> {
            if let Some(failure) = &self.failure
                && self.failing_key.is_none_or(|failing| failing == key)
            {
                return Err(failure.clone());
            }
            self.entries
                .borrow_mut()
                .insert(key.to_owned(), secret.to_owned());
            Ok(())
        }

        fn delete(&self, key: &str) -> Result<(), crate::config::SecretStoreError> {
            self.entries.borrow_mut().remove(key);
            Ok(())
        }
    }

    fn hybrid_profile(api_key: Option<&str>, password: Option<&str>) -> crate::config::Profile {
        crate::config::Profile {
            controller: "https://192.168.1.1".into(),
            site: "default".into(),
            auth_mode: "hybrid".into(),
            api_key: api_key.map(Into::into),
            api_key_env: None,
            host_id: None,
            host_id_env: None,
            username: Some("bliss".into()),
            password: password.map(Into::into),
            totp_env: None,
            totp_command: None,
            ca_cert: None,
            insecure: Some(true),
            timeout: None,
        }
    }

    fn toml_of(profile: &crate::config::Profile) -> String {
        toml::to_string(profile).expect("profile serializes")
    }

    /// Credentials held by the keyring need no retyping, and saving
    /// writes no plaintext and leaves the keyring entries alone.
    #[test]
    fn keyring_backed_profile_saves_without_plaintext() {
        let store = MemoryStore::with(&[
            ("home/api-key", "keyring-api-key"),
            ("home/password", "keyring-password"),
        ]);
        let draft = ControllerProfileDraft::from_profile(&hybrid_profile(None, None))
            .with_keyring_origins("home", &store);

        assert_eq!(draft.api_key_origin, SecretOrigin::Keyring);
        assert_eq!(draft.password_origin, SecretOrigin::Keyring);
        assert_eq!(draft.validate_complete(), Ok(()), "no retyping needed");

        // Connecting leaves both to the resolver, which reads the keyring.
        let connect = draft.to_profile();
        assert_eq!(connect.api_key, None);
        assert_eq!(connect.password, None);

        let saved = draft
            .plan_save("home")
            .store_secrets(&store)
            .expect("saves");
        assert_eq!(saved.warnings, Vec::<String>::new());
        assert_eq!(saved.profile.api_key, None);
        assert_eq!(saved.profile.password, None);
        let written = toml_of(&saved.profile);
        assert!(!written.contains("keyring-"), "{written}");
        assert_eq!(
            store.secret("home/api-key").as_deref(),
            Some("keyring-api-key")
        );
        assert_eq!(
            store.secret("home/password").as_deref(),
            Some("keyring-password")
        );
    }

    /// A value typed over a keyring-held credential replaces the keyring
    /// entry, as `config init` stores it, and is used for the test connect.
    #[test]
    fn typed_replacement_goes_to_the_keyring() {
        let store = MemoryStore::with(&[("home/password", "old-password")]);
        let mut draft = ControllerProfileDraft::from_profile(&hybrid_profile(Some("k"), None))
            .with_keyring_origins("home", &store);
        draft.password.set("new-password");

        assert_eq!(draft.to_profile().password.as_deref(), Some("new-password"));

        let saved = draft
            .plan_save("home")
            .store_secrets(&store)
            .expect("saves");
        assert_eq!(saved.warnings, Vec::<String>::new());
        assert_eq!(saved.profile.password, None);
        assert!(!toml_of(&saved.profile).contains("new-password"));
        assert_eq!(
            store.secret("home/password").as_deref(),
            Some("new-password")
        );
    }

    /// A brand-new credential (a new profile, or one the old mode did not
    /// use) also goes to the keyring.
    #[test]
    fn new_credentials_go_to_the_keyring() {
        let store = MemoryStore::default();
        let mut draft = sample_draft();
        draft.api_key.set("fresh-key");
        draft.password.set("fresh-password");

        let saved = draft
            .plan_save("home")
            .store_secrets(&store)
            .expect("saves");

        assert_eq!(saved.profile.api_key, None);
        assert_eq!(saved.profile.password, None);
        assert_eq!(store.secret("home/api-key").as_deref(), Some("fresh-key"));
        assert_eq!(
            store.secret("home/password").as_deref(),
            Some("fresh-password")
        );
    }

    /// Without a usable keyring, saving falls back to plaintext in the
    /// profile and says so, rather than losing the credential.
    #[test]
    fn unavailable_keyring_falls_back_to_plaintext_with_a_warning() {
        let store = MemoryStore {
            failure: Some(crate::config::SecretStoreError::Unavailable(
                "no keyring daemon".into(),
            )),
            ..MemoryStore::default()
        };
        let mut draft = sample_draft();
        draft.password.set("fresh-password");

        let saved = draft
            .plan_save("home")
            .store_secrets(&store)
            .expect("falls back");

        assert_eq!(saved.profile.password.as_deref(), Some("fresh-password"));
        assert_eq!(saved.warnings.len(), 2, "{:?}", saved.warnings);
        assert!(
            saved.warnings.iter().all(|w| !w.contains("fresh-password")),
            "a warning must not carry the secret"
        );
    }

    /// A keyring that is there but refuses the secret must not be swapped
    /// for plaintext: the save stops, naming the field but not the secret.
    #[test]
    fn a_refusing_keyring_blocks_the_save_and_writes_no_plaintext() {
        let store = MemoryStore {
            failure: Some(crate::config::SecretStoreError::Rejected(
                "Entry is matched by 2 credentials".into(),
            )),
            ..MemoryStore::default()
        };
        let mut draft = sample_draft();
        draft.api_key.clear();
        draft.api_key_origin = SecretOrigin::Keyring;
        draft.password.set("fresh-password");

        let error = draft
            .plan_save("home")
            .store_secrets(&store)
            .err()
            .expect("a refused secret blocks the save");

        assert!(error.contains("password"), "{error}");
        assert!(error.contains("matched by 2 credentials"), "{error}");
        assert!(
            !error.contains("fresh-password"),
            "the secret leaked: {error}"
        );
        assert!(store.secret("home/password").is_none());
    }

    /// A blocked save leaves the keyring exactly as it was: an entry this
    /// save already replaced gets its old value back, and one it created
    /// is removed again.
    #[test]
    fn a_refused_secret_rolls_back_what_the_save_already_stored() {
        for previous in [Some("old-key"), None] {
            let store = MemoryStore {
                failure: Some(crate::config::SecretStoreError::Rejected(
                    "Value of 'password' is longer than the platform limit".into(),
                )),
                failing_key: Some("home/password"),
                ..MemoryStore::default()
            };
            if let Some(previous) = previous {
                store
                    .entries
                    .borrow_mut()
                    .insert("home/api-key".into(), previous.into());
            }
            let mut draft = sample_draft();
            draft.api_key.set("new-key");
            draft.password.set("new-password");

            let error = draft
                .plan_save("home")
                .store_secrets(&store)
                .err()
                .expect("the refused password blocks the save");

            assert!(error.contains("password"), "{error}");
            assert!(!error.contains("could not be put back"), "{error}");
            assert_eq!(store.secret("home/api-key").as_deref(), previous);
            assert!(store.secret("home/password").is_none());
        }
    }

    /// An entry whose current value cannot be read could not be restored
    /// by a rollback, so it is never overwritten: the save stops and the
    /// entry keeps its value. What the save already wrote is undone too.
    #[test]
    fn an_unreadable_entry_blocks_the_save_and_is_left_alone() {
        let store = MemoryStore {
            read_failure: Some(crate::config::SecretStoreError::Rejected(
                "Password data is not valid UTF-8".into(),
            )),
            failing_key: Some("home/password"),
            ..MemoryStore::with(&[("home/password", "existing-password")])
        };
        let mut draft = sample_draft();
        draft.api_key.set("new-key");
        draft.password.set("new-password");

        let error = draft
            .plan_save("home")
            .store_secrets(&store)
            .err()
            .expect("an unreadable entry blocks the save");

        assert!(error.contains("password"), "{error}");
        assert!(
            !error.contains("new-password"),
            "the secret leaked: {error}"
        );
        assert_eq!(
            store.secret("home/password").as_deref(),
            Some("existing-password")
        );
        assert!(
            store.secret("home/api-key").is_none(),
            "the API key was not rolled back"
        );
    }

    /// With no usable keyring even for reading, the documented plaintext
    /// fallback applies and the keyring is not written at all.
    #[test]
    fn an_unavailable_keyring_on_read_falls_back_without_touching_it() {
        let store = MemoryStore {
            read_failure: Some(crate::config::SecretStoreError::Unavailable(
                "no keyring daemon".into(),
            )),
            ..MemoryStore::with(&[("home/password", "existing-password")])
        };
        let mut draft = sample_draft();
        draft.password.set("new-password");

        let saved = draft
            .plan_save("home")
            .store_secrets(&store)
            .expect("falls back");

        assert_eq!(saved.profile.password.as_deref(), Some("new-password"));
        assert_eq!(saved.warnings.len(), 2, "{:?}", saved.warnings);
        assert_eq!(
            store.secret("home/password").as_deref(),
            Some("existing-password")
        );
    }

    /// A config.toml write that fails after the keyring step must not leave
    /// the keyring changed: the settings screen would otherwise pair the old
    /// profile with a new credential, and setup would leave a credential
    /// with no profile.
    #[test]
    fn a_failed_config_save_rolls_the_keyring_back() {
        for previous in [Some("old-key"), None] {
            let store = MemoryStore::default();
            if let Some(previous) = previous {
                store
                    .entries
                    .borrow_mut()
                    .insert("home/api-key".into(), previous.into());
            }
            let mut draft = sample_draft();
            draft.api_key.set("new-key");
            draft.password.set("new-password");

            let error = draft
                .plan_save("home")
                .commit(&store, |profile| {
                    assert_eq!(profile.api_key, None, "the key went to the keyring");
                    assert_eq!(
                        store.secret("home/api-key").as_deref(),
                        Some("new-key"),
                        "the keyring step ran before the config write"
                    );
                    Err("Connected, but failed to save config: disk full".into())
                })
                .expect_err("the failed config write fails the save");

            assert!(error.contains("disk full"), "{error}");
            assert!(!error.contains("new-key"), "a secret leaked: {error}");
            assert!(!error.contains("new-password"), "a secret leaked: {error}");
            assert_eq!(store.secret("home/api-key").as_deref(), previous);
            assert!(store.secret("home/password").is_none());
        }
    }

    /// When the config write succeeds, the keyring keeps the new entries
    /// and the plaintext-fallback notices come back to be shown.
    #[test]
    fn a_successful_save_keeps_the_keyring_changes() {
        let store = MemoryStore::default();
        let mut draft = sample_draft();
        draft.api_key.set("new-key");

        let mut written = None;
        let notices = draft
            .plan_save("home")
            .commit(&store, |profile| {
                written = Some(profile);
                Ok(())
            })
            .expect("saves");

        assert_eq!(notices, Vec::<String>::new());
        assert_eq!(written.expect("config written").api_key, None);
        assert_eq!(store.secret("home/api-key").as_deref(), Some("new-key"));
    }

    /// Profiles that keep credentials in config.toml keep them there,
    /// edited or not, and the keyring is never consulted or written.
    #[test]
    fn config_stored_credentials_stay_in_config() {
        let store = MemoryStore::with(&[("home/password", "stale-keyring-password")]);
        let mut draft = ControllerProfileDraft::from_profile(&hybrid_profile(
            Some("config-key"),
            Some("config-password"),
        ))
        .with_keyring_origins("home", &store);
        draft.password.set("edited-password");

        assert_eq!(draft.api_key_origin, SecretOrigin::Config);
        assert_eq!(draft.password_origin, SecretOrigin::Config);

        let saved = draft
            .plan_save("home")
            .store_secrets(&store)
            .expect("saves");
        assert_eq!(saved.profile.api_key.as_deref(), Some("config-key"));
        assert_eq!(saved.profile.password.as_deref(), Some("edited-password"));
        assert_eq!(
            store.secret("home/password").as_deref(),
            Some("stale-keyring-password"),
            "the keyring is left alone"
        );
        assert!(store.secret("home/api-key").is_none());
    }

    #[test]
    fn a_missing_keyring_entry_still_requires_the_credential() {
        let draft = ControllerProfileDraft::from_profile(&hybrid_profile(Some("k"), None))
            .with_keyring_origins("home", &MemoryStore::default());

        assert_eq!(draft.password_origin, SecretOrigin::Unset);
        assert_eq!(
            message(draft.validate_credentials()).as_deref(),
            Some("Password cannot be empty")
        );
    }

    #[test]
    fn from_profile_restores_draft_fields() {
        let profile = crate::config::Profile {
            controller: "https://console.example.com".into(),
            site: "default".into(),
            auth_mode: "session".into(),
            api_key: None,
            api_key_env: None,
            host_id: None,
            host_id_env: None,
            username: Some("bliss".into()),
            password: Some("hunter2".into()),
            totp_env: None,
            totp_command: None,
            ca_cert: None,
            insecure: Some(false),
            timeout: None,
        };

        let draft = ControllerProfileDraft::from_profile(&profile);

        assert_eq!(draft.url.text(), "https://console.example.com");
        assert_eq!(draft.auth_mode, AuthMode::Session);
        assert_eq!(draft.username.text(), "bliss");
        assert_eq!(draft.password.text(), "hunter2");
        assert!(!draft.insecure);
    }
}
