use super::{AuthMode, ControllerProfileDraft, CredentialField, OnboardingScreen, WizardStep};

impl WizardStep {
    pub(super) fn index(self) -> usize {
        match self {
            Self::Welcome => 0,
            Self::Url => 1,
            Self::AuthMode => 2,
            Self::Credentials => 3,
            Self::Site => 4,
            Self::Testing => 5,
            Self::Done => 6,
        }
    }
}

impl OnboardingScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            action_tx: None,
            step: WizardStep::Welcome,
            draft: ControllerProfileDraft::default(),
            auth_mode_index: 0,
            cred_field: CredentialField::ApiKey,
            show_password: false,
            testing: false,
            test_error: None,
            error: None,
            throbber_state: throbber_widgets_tui::ThrobberState::default(),
        }
    }

    pub(super) fn advance(&mut self) {
        self.error = None;
        match self.step {
            WizardStep::Welcome => self.step = WizardStep::Url,
            WizardStep::Url => {
                if let Err(message) = self.draft.validate_url() {
                    self.error = Some(message);
                    return;
                }
                self.step = WizardStep::AuthMode;
            }
            WizardStep::AuthMode => {
                self.draft.auth_mode = AuthMode::ALL[self.auth_mode_index];
                self.draft.apply_auth_mode_defaults();
                self.cred_field = match self.draft.auth_mode {
                    AuthMode::ApiKey | AuthMode::Hybrid | AuthMode::Cloud => {
                        CredentialField::ApiKey
                    }
                    AuthMode::Session => CredentialField::Username,
                };
                self.step = WizardStep::Credentials;
            }
            WizardStep::Credentials => {
                if let Err(message) = self.validate_credentials() {
                    self.error = Some(message);
                    return;
                }
                self.step = WizardStep::Site;
            }
            WizardStep::Site => {
                if self.draft.site.trim().is_empty() {
                    self.error = Some("Site name cannot be empty".into());
                    return;
                }
                self.step = WizardStep::Testing;
                self.start_connection_test();
            }
            WizardStep::Testing => {}
            WizardStep::Done => {
                self.send_completion();
            }
        }
    }

    pub(super) fn go_back(&mut self) {
        self.error = None;
        self.test_error = None;
        match self.step {
            WizardStep::Welcome => {}
            WizardStep::Url => self.step = WizardStep::Welcome,
            WizardStep::AuthMode => self.step = WizardStep::Url,
            WizardStep::Credentials => self.step = WizardStep::AuthMode,
            WizardStep::Site => self.step = WizardStep::Credentials,
            WizardStep::Testing => {
                self.testing = false;
                self.step = WizardStep::Site;
            }
            WizardStep::Done => self.step = WizardStep::Site,
        }
    }

    pub(super) fn validate_credentials(&self) -> std::result::Result<(), String> {
        self.draft.validate_credentials()
    }

    fn build_profile(&self) -> crate::config::Profile {
        self.draft.to_profile()
    }

    fn start_connection_test(&mut self) {
        self.testing = true;
        self.test_error = None;

        let profile = self.build_profile();
        let profile_name = "default".to_string();
        let save = self.draft.plan_save(&profile_name);

        let Some(tx) = self.action_tx.clone() else {
            return;
        };

        tokio::spawn(async move {
            let defaults = crate::config::load_config()
                .map(|cfg| cfg.defaults)
                .unwrap_or_default();
            let result = match crate::config::profile_to_controller_config(
                &profile,
                &profile_name,
                &defaults,
            ) {
                Ok(config) => {
                    let controller = unifly_api::Controller::new(config);
                    match controller.connect().await {
                        Ok(()) => {
                            controller.disconnect().await;
                            // A refused secret or a failed config write undoes
                            // the keyring changes; the message becomes the
                            // test result.
                            let saved = save.commit(&crate::config::OsKeyring, |profile| {
                                save_profile(&crate::config::config_path(), &profile_name, profile)
                                    .map_err(|error| {
                                        format!("Connected, but failed to save config: {error}")
                                    })
                            });
                            saved.map(|notices| {
                                for notice in notices {
                                    let _ = tx.send(crate::tui::action::Action::Notify(
                                        crate::tui::action::Notification::info(notice),
                                    ));
                                }
                            })
                        }
                        Err(error) => Err(format!("{error}")),
                    }
                }
                Err(error) => Err(format!("{error}")),
            };

            let _ = tx.send(crate::tui::action::Action::OnboardingTestResult(result));
        });
    }

    pub(super) fn send_completion(&self) {
        let profile = self.build_profile();
        let profile_name = "default";

        let Some(tx) = self.action_tx.clone() else {
            return;
        };

        let defaults = crate::config::load_config()
            .map(|cfg| cfg.defaults)
            .unwrap_or_default();
        match crate::config::profile_to_controller_config(&profile, profile_name, &defaults) {
            Ok(config) => {
                let _ = tx.send(crate::tui::action::Action::OnboardingComplete {
                    profile_name: profile_name.to_string(),
                    config: Box::new(config),
                });
            }
            Err(error) => {
                let _ = tx.send(crate::tui::action::Action::Notify(
                    crate::tui::action::Notification::error(format!("{error}")),
                ));
            }
        }
    }

    pub(super) fn active_input_mut(&mut self) -> Option<&mut String> {
        match self.step {
            WizardStep::Url => Some(&mut self.draft.url),
            WizardStep::Site => Some(&mut self.draft.site),
            WizardStep::Credentials => match self.cred_field {
                CredentialField::ApiKey => Some(&mut self.draft.api_key),
                CredentialField::HostId => Some(&mut self.draft.host_id),
                CredentialField::Username => Some(&mut self.draft.username),
                CredentialField::Password => Some(&mut self.draft.password),
            },
            _ => None,
        }
    }

    pub(super) fn next_cred_field(&mut self) {
        self.cred_field = match (self.draft.auth_mode, self.cred_field) {
            (AuthMode::Session | AuthMode::Hybrid, CredentialField::Username) => {
                CredentialField::Password
            }
            (AuthMode::Hybrid, CredentialField::Password) => CredentialField::ApiKey,
            (AuthMode::Cloud, CredentialField::ApiKey) => CredentialField::HostId,
            (AuthMode::Cloud, CredentialField::HostId) | (AuthMode::ApiKey, _) => {
                CredentialField::ApiKey
            }
            (AuthMode::Session, _)
            | (AuthMode::Hybrid, CredentialField::ApiKey | CredentialField::HostId)
            | (AuthMode::Cloud, CredentialField::Username | CredentialField::Password) => {
                CredentialField::Username
            }
        };
    }
}

/// Write the profile the wizard set up into the config file at `path` and
/// make it the default, keeping every other profile and setting in it.
fn save_profile(
    path: &std::path::Path,
    profile_name: &str,
    profile: crate::config::Profile,
) -> Result<(), crate::config::ConfigError> {
    let mut cfg = crate::config::load_config_from(path)?;
    cfg.profiles.insert(profile_name.to_owned(), profile);
    cfg.default_profile = Some(profile_name.to_owned());
    crate::config::save_config_to(path, &cfg)
}

#[cfg(test)]
mod tests {
    use super::{AuthMode, CredentialField, OnboardingScreen, WizardStep, save_profile};

    /// Onboarding also opens when the selected profile is missing or fails
    /// to resolve, so the file it saves into can hold other profiles and
    /// settings. Finishing the wizard adds its profile and keeps the rest.
    #[test]
    fn save_profile_keeps_the_rest_of_the_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "default_profile = \"work\"\n\n\
             [defaults]\ntimeout = 45\ntheme = \"nord\"\n\n\
             [profiles.work]\ncontroller = \"https://10.0.0.1\"\n\n\
             [demo]\nredact_names = [\"Bliss\"]\n",
        )
        .expect("write config");

        let mut screen = OnboardingScreen::new();
        screen.draft.url = "https://192.168.1.1".into();
        screen.draft.api_key = "api-key".into();
        save_profile(&path, "default", screen.build_profile()).expect("save");

        let cfg = crate::config::load_config_from(&path).expect("load saved config");
        assert_eq!(cfg.default_profile.as_deref(), Some("default"));
        assert_eq!(cfg.profiles["default"].controller, "https://192.168.1.1");
        assert_eq!(cfg.profiles["work"].controller, "https://10.0.0.1");
        assert_eq!(cfg.defaults.timeout, 45);
        assert_eq!(cfg.defaults.theme.as_deref(), Some("nord"));
        assert_eq!(cfg.demo.redact_names, ["Bliss"]);
    }

    /// A config that fails to load must fail the save and stay as it was.
    #[test]
    fn save_profile_leaves_a_config_that_fails_to_load_untouched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        let broken = "[defaults]\ntimeout = \"thirty\"\n";
        std::fs::write(&path, broken).expect("write config");

        let result = save_profile(&path, "default", OnboardingScreen::new().build_profile());

        assert!(result.is_err(), "the save should fail");
        assert_eq!(std::fs::read_to_string(&path).expect("read config"), broken);
    }

    #[test]
    fn advance_rejects_invalid_url() {
        let mut screen = OnboardingScreen::new();
        screen.step = WizardStep::Url;
        screen.draft.url = "not a url".into();

        screen.advance();

        assert_eq!(screen.step, WizardStep::Url);
        assert_eq!(screen.error.as_deref(), Some("Invalid URL format"));
    }

    #[test]
    fn auth_mode_step_sets_legacy_credential_field() {
        let mut screen = OnboardingScreen::new();
        screen.step = WizardStep::AuthMode;
        screen.auth_mode_index = 1;

        screen.advance();

        assert_eq!(screen.step, WizardStep::Credentials);
        assert_eq!(screen.draft.auth_mode, AuthMode::Session);
        assert_eq!(screen.cred_field, CredentialField::Username);
    }

    #[test]
    fn validate_credentials_requires_password_for_hybrid() {
        let mut screen = OnboardingScreen::new();
        screen.draft.auth_mode = AuthMode::Hybrid;
        screen.draft.api_key = "api-key".into();
        screen.draft.username = "bliss".into();
        screen.draft.password.clear();

        assert_eq!(
            screen.validate_credentials(),
            Err("Password cannot be empty".into())
        );
    }

    #[test]
    fn cloud_auth_mode_starts_with_api_key_then_cycles_to_host_id() {
        let mut screen = OnboardingScreen::new();
        screen.step = WizardStep::AuthMode;
        screen.auth_mode_index = 3;

        screen.advance();

        assert_eq!(screen.step, WizardStep::Credentials);
        assert_eq!(screen.draft.auth_mode, AuthMode::Cloud);
        assert_eq!(screen.cred_field, CredentialField::ApiKey);

        screen.next_cred_field();
        assert_eq!(screen.cred_field, CredentialField::HostId);
    }
}
