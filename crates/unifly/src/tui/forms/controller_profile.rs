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

    pub(crate) fn from_config(value: &str) -> Self {
        match value {
            "session" => Self::Session,
            "hybrid" => Self::Hybrid,
            "cloud" => Self::Cloud,
            _ => Self::ApiKey,
        }
    }
}

use std::fmt::Write as _;

use secrecy::{ExposeSecret, SecretString};

use crate::config::{SecretStore, SecretStoreError, api_key_keyring_key, password_keyring_key};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControllerProfileDraft {
    pub(crate) url: String,
    pub(crate) auth_mode: AuthMode,
    pub(crate) api_key: String,
    pub(crate) api_key_env: Option<String>,
    pub(crate) host_id: String,
    pub(crate) host_id_env: Option<String>,
    pub(crate) username: String,
    pub(crate) password: String,
    pub(crate) site: String,
    pub(crate) insecure: bool,
    /// Whether the user changed the insecure toggle. Until they do, saving
    /// keeps the stored value, including "unset".
    insecure_touched: bool,
    /// What the form does not edit, from the profile it was loaded from.
    /// `None` for a new profile.
    pub(crate) loaded: Option<LoadedProfileFields>,
    pub(crate) api_key_origin: SecretOrigin,
    pub(crate) password_origin: SecretOrigin,
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

/// The profile to write after [`SavePlan::commit`], and any notice the user
/// should see about where a secret ended up.
pub(crate) struct SavedProfile {
    pub(crate) profile: crate::config::Profile,
    pub(crate) warnings: Vec<String>,
}

impl SavePlan {
    /// Store the pending secrets in `store`.
    ///
    /// When no keyring is usable, a secret falls back to plaintext in the
    /// profile (the documented fallback for hosts without one), with a
    /// warning naming the field. A keyring that is there but refuses a
    /// secret stops the save instead: entries this save already wrote are
    /// put back as they were, the error names the field (never the
    /// secret), and the caller must leave `config.toml` alone.
    pub(crate) fn commit(self, store: &dyn SecretStore) -> Result<SavedProfile, String> {
        let mut profile = self.profile;
        let mut warnings = Vec::new();
        let mut written = Vec::new();
        for pending in self.keyring {
            let label = pending.field.label();
            let previous = store.get(&pending.key).map(SecretString::from);
            match store.set(&pending.key, pending.secret.expose_secret()) {
                Ok(()) => written.push(WrittenSecret {
                    label,
                    key: pending.key,
                    previous,
                }),
                Err(SecretStoreError::Unavailable(reason)) => {
                    warnings.push(format!(
                        "No usable keyring ({reason}); saved the {label} to config.toml in \
                         plaintext"
                    ));
                    let plaintext = Some(pending.secret.expose_secret().to_owned());
                    match pending.field {
                        SecretField::ApiKey => profile.api_key = plaintext,
                        SecretField::Password => profile.password = plaintext,
                    }
                }
                Err(SecretStoreError::Rejected(reason)) => {
                    let mut message = format!(
                        "Couldn't store the {label} in the keyring ({reason}); config.toml was \
                         not changed"
                    );
                    for (restored, error) in roll_back(store, &written) {
                        let _ = write!(
                            message,
                            ", and the {restored} it had already stored could not be put back \
                             ({error})"
                        );
                    }
                    return Err(message);
                }
            }
        }
        Ok(SavedProfile { profile, warnings })
    }
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
    ca_cert: Option<std::path::PathBuf>,
    timeout: Option<u64>,
    /// The stored `insecure`, which may be unset (inherit `[defaults]`).
    /// An untouched toggle writes this back, so "unset" stays unset.
    insecure: Option<bool>,
}

impl Default for ControllerProfileDraft {
    fn default() -> Self {
        Self {
            url: "https://192.168.1.1".into(),
            auth_mode: AuthMode::ApiKey,
            api_key: String::new(),
            api_key_env: None,
            host_id: String::new(),
            host_id_env: None,
            username: String::new(),
            password: String::new(),
            site: "default".into(),
            insecure: true,
            insecure_touched: false,
            loaded: None,
            api_key_origin: SecretOrigin::Unset,
            password_origin: SecretOrigin::Unset,
        }
    }
}

impl ControllerProfileDraft {
    pub(crate) fn from_profile(profile: &crate::config::Profile) -> Self {
        let mut draft = Self {
            url: profile.controller.clone(),
            auth_mode: AuthMode::from_config(&profile.auth_mode),
            api_key: profile.api_key.clone().unwrap_or_default(),
            api_key_env: profile.api_key_env.clone(),
            host_id: profile.host_id.clone().unwrap_or_default(),
            host_id_env: profile.host_id_env.clone(),
            username: profile.username.clone().unwrap_or_default(),
            password: profile.password.clone().unwrap_or_default(),
            site: profile.site.clone(),
            insecure: profile.insecure.unwrap_or(false),
            insecure_touched: false,
            loaded: Some(LoadedProfileFields {
                totp_env: profile.totp_env.clone(),
                totp_command: profile.totp_command.clone(),
                ca_cert: profile.ca_cert.clone(),
                timeout: profile.timeout,
                insecure: profile.insecure,
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
        };
        draft.apply_auth_mode_defaults();
        draft
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
        if self.api_key_origin == SecretOrigin::Unset
            && store.get(&api_key_keyring_key(profile_name)).is_some()
        {
            self.api_key_origin = SecretOrigin::Keyring;
        }
        if self.password_origin == SecretOrigin::Unset
            && store.get(&password_keyring_key(profile_name)).is_some()
        {
            self.password_origin = SecretOrigin::Keyring;
        }
        self
    }

    /// Show what an unset `insecure` actually resolves to, so the toggle
    /// tells the truth: a profile CA means verification, otherwise
    /// `[defaults].insecure` applies. An explicit value is shown as is.
    pub(crate) fn with_inherited_insecure(mut self, defaults: &crate::config::Defaults) -> Self {
        if self.auth_mode != AuthMode::Cloud
            && let Some(loaded) = &self.loaded
            && loaded.insecure.is_none()
        {
            self.insecure = loaded.ca_cert.is_none() && defaults.insecure;
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
    /// always saves the toggle.
    fn insecure_to_save(&self) -> Option<bool> {
        match &self.loaded {
            Some(loaded) if !self.insecure_touched => loaded.insecure,
            _ => Some(self.insecure),
        }
    }

    pub(crate) fn apply_auth_mode_defaults(&mut self) {
        if self.auth_mode == AuthMode::Cloud {
            if self.url.trim().is_empty() || self.url == "https://192.168.1.1" {
                self.url = crate::config::DEFAULT_CLOUD_CONTROLLER_URL.into();
            }
            self.insecure = false;
        }
    }

    pub(crate) fn validate_url(&self) -> std::result::Result<(), String> {
        let trimmed = self.url.trim();
        if trimmed.is_empty() {
            return Err("URL cannot be empty".into());
        }
        if trimmed.parse::<url::Url>().is_err() {
            return Err("Invalid URL format".into());
        }
        Ok(())
    }

    pub(crate) fn validate_credentials(&self) -> std::result::Result<(), String> {
        match self.auth_mode {
            AuthMode::ApiKey => {
                if !self.has_api_key() {
                    return Err("API key cannot be empty".into());
                }
            }
            AuthMode::Cloud => {
                if !self.has_api_key() {
                    return Err("API key cannot be empty".into());
                }
                if !self.has_host_id() {
                    return Err("Host ID cannot be empty".into());
                }
            }
            AuthMode::Session => {
                if self.username.trim().is_empty() {
                    return Err("Username cannot be empty".into());
                }
                if !self.has_password() {
                    return Err("Password cannot be empty".into());
                }
            }
            AuthMode::Hybrid => {
                if !self.has_api_key() {
                    return Err("API key cannot be empty".into());
                }
                if self.username.trim().is_empty() {
                    return Err("Username cannot be empty".into());
                }
                if !self.has_password() {
                    return Err("Password cannot be empty".into());
                }
            }
        }

        Ok(())
    }

    fn has_password(&self) -> bool {
        !self.password.is_empty() || self.password_origin == SecretOrigin::Keyring
    }

    fn has_api_key(&self) -> bool {
        !self.api_key.trim().is_empty()
            || self.api_key_origin == SecretOrigin::Keyring
            || self
                .api_key_env
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
    }

    fn has_host_id(&self) -> bool {
        !self.host_id.trim().is_empty()
            || self
                .host_id_env
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
    }

    pub(crate) fn validate_complete(&self) -> std::result::Result<(), String> {
        self.validate_url()?;
        self.validate_credentials()?;

        if self.site.trim().is_empty() {
            return Err("Site name cannot be empty".into());
        }

        Ok(())
    }

    /// The profile to connect with: typed credentials inline, and those
    /// left in the keyring absent, so credential resolution reads them from
    /// there. Use [`Self::plan_save`] for what goes to `config.toml`.
    pub(crate) fn to_profile(&self) -> crate::config::Profile {
        let loaded = self.loaded.clone().unwrap_or_default();
        crate::config::Profile {
            controller: self.url.trim().to_string(),
            site: self.site.trim().to_string(),
            auth_mode: self.auth_mode.config_value().to_string(),
            api_key: match self.auth_mode {
                AuthMode::ApiKey | AuthMode::Hybrid | AuthMode::Cloud => {
                    let value = self.api_key.trim();
                    (!value.is_empty()).then(|| value.to_string())
                }
                AuthMode::Session => None,
            },
            api_key_env: match self.auth_mode {
                AuthMode::ApiKey | AuthMode::Hybrid | AuthMode::Cloud
                    if self.api_key.trim().is_empty() =>
                {
                    self.api_key_env.clone()
                }
                AuthMode::ApiKey | AuthMode::Session | AuthMode::Hybrid | AuthMode::Cloud => None,
            },
            host_id: match self.auth_mode {
                AuthMode::Cloud => {
                    let value = self.host_id.trim();
                    (!value.is_empty()).then(|| value.to_string())
                }
                AuthMode::ApiKey | AuthMode::Session | AuthMode::Hybrid => None,
            },
            host_id_env: match self.auth_mode {
                AuthMode::Cloud if self.host_id.trim().is_empty() => self.host_id_env.clone(),
                AuthMode::ApiKey | AuthMode::Session | AuthMode::Hybrid | AuthMode::Cloud => None,
            },
            username: match self.auth_mode {
                AuthMode::Session | AuthMode::Hybrid => Some(self.username.trim().to_string()),
                AuthMode::ApiKey | AuthMode::Cloud => None,
            },
            password: match self.auth_mode {
                AuthMode::Session | AuthMode::Hybrid => {
                    (!self.password.is_empty()).then(|| self.password.clone())
                }
                AuthMode::ApiKey | AuthMode::Cloud => None,
            },
            totp_env: loaded.totp_env,
            totp_command: loaded.totp_command,
            ca_cert: loaded.ca_cert,
            insecure: self.insecure_to_save(),
            timeout: loaded.timeout,
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

#[cfg(test)]
mod tests {
    use super::{AuthMode, ControllerProfileDraft, SecretOrigin};

    fn sample_draft() -> ControllerProfileDraft {
        ControllerProfileDraft {
            url: "https://console.example.com".into(),
            auth_mode: AuthMode::Hybrid,
            api_key: "api-key".into(),
            api_key_env: None,
            host_id: String::new(),
            host_id_env: None,
            username: "bliss".into(),
            password: "hunter2".into(),
            site: "default".into(),
            insecure: false,
            insecure_touched: false,
            loaded: None,
            api_key_origin: SecretOrigin::Unset,
            password_origin: SecretOrigin::Unset,
        }
    }

    #[test]
    fn validate_complete_rejects_invalid_url() {
        let mut draft = sample_draft();
        draft.url = "not a url".into();

        assert_eq!(draft.validate_complete(), Err("Invalid URL format".into()));
    }

    #[test]
    fn validate_credentials_requires_password_for_hybrid() {
        let mut draft = sample_draft();
        draft.password.clear();

        assert_eq!(
            draft.validate_credentials(),
            Err("Password cannot be empty".into())
        );
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
        assert_eq!(draft.url, crate::config::DEFAULT_CLOUD_CONTROLLER_URL);
        assert_eq!(draft.host_id, "console-123");
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
    /// a write returns instead of storing: for `failing_key` only, when that
    /// is set, otherwise for every key.
    #[derive(Default)]
    struct MemoryStore {
        entries: std::cell::RefCell<std::collections::HashMap<String, String>>,
        failure: Option<crate::config::SecretStoreError>,
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
        fn get(&self, key: &str) -> Option<String> {
            self.secret(key)
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

        let saved = draft.plan_save("home").commit(&store).expect("saves");
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
        draft.password = "new-password".into();

        assert_eq!(draft.to_profile().password.as_deref(), Some("new-password"));

        let saved = draft.plan_save("home").commit(&store).expect("saves");
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
        draft.api_key = "fresh-key".into();
        draft.password = "fresh-password".into();

        let saved = draft.plan_save("home").commit(&store).expect("saves");

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
        draft.password = "fresh-password".into();

        let saved = draft.plan_save("home").commit(&store).expect("falls back");

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
        draft.password = "fresh-password".into();

        let error = draft
            .plan_save("home")
            .commit(&store)
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
            draft.api_key = "new-key".into();
            draft.password = "new-password".into();

            let error = draft
                .plan_save("home")
                .commit(&store)
                .err()
                .expect("the refused password blocks the save");

            assert!(error.contains("password"), "{error}");
            assert!(!error.contains("could not be put back"), "{error}");
            assert_eq!(store.secret("home/api-key").as_deref(), previous);
            assert!(store.secret("home/password").is_none());
        }
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
        draft.password = "edited-password".into();

        assert_eq!(draft.api_key_origin, SecretOrigin::Config);
        assert_eq!(draft.password_origin, SecretOrigin::Config);

        let saved = draft.plan_save("home").commit(&store).expect("saves");
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
            draft.validate_credentials(),
            Err("Password cannot be empty".into())
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

        assert_eq!(draft.url, "https://console.example.com");
        assert_eq!(draft.auth_mode, AuthMode::Session);
        assert_eq!(draft.username, "bliss");
        assert_eq!(draft.password, "hunter2");
        assert!(!draft.insecure);
    }
}
