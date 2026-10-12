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
        };
        draft.apply_auth_mode_defaults();
        draft
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
                if self.password.is_empty() {
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
                if self.password.is_empty() {
                    return Err("Password cannot be empty".into());
                }
            }
        }

        Ok(())
    }

    fn has_api_key(&self) -> bool {
        !self.api_key.trim().is_empty()
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

    pub(crate) fn to_profile(&self) -> crate::config::Profile {
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
                AuthMode::Session | AuthMode::Hybrid => Some(self.password.clone()),
                AuthMode::ApiKey | AuthMode::Cloud => None,
            },
            totp_env: None,
            totp_command: None,
            ca_cert: None,
            insecure: Some(self.insecure),
            timeout: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthMode, ControllerProfileDraft};

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
