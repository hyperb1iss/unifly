use super::{
    AuthMode, ControllerProfileDraft, FormEntry, SettingsField, SettingsScreen, SettingsState,
};

// ── Field metadata ──────────────────────────────────────────────────

impl SettingsField {
    pub(super) const ALL: [SettingsField; 10] = [
        Self::Url,
        Self::AuthMode,
        Self::ApiKey,
        Self::HostId,
        Self::Username,
        Self::Password,
        Self::Site,
        Self::Insecure,
        Self::Theme,
        Self::ShowDonate,
    ];

    /// Section label for this field — used to insert dividers.
    pub(super) fn section(self) -> &'static str {
        match self {
            Self::Url
            | Self::AuthMode
            | Self::ApiKey
            | Self::HostId
            | Self::Username
            | Self::Password
            | Self::Site
            | Self::Insecure => "Connection",
            Self::Theme | Self::ShowDonate => "Appearance",
        }
    }

    /// Row height this field occupies.
    pub(super) fn row_height(self) -> u16 {
        match self {
            Self::Insecure | Self::ShowDonate => 1,
            Self::Theme => 2,
            _ => 4,
        }
    }

    /// Whether this field is visible given the current auth mode.
    pub(super) fn visible_for(self, mode: AuthMode) -> bool {
        match self {
            Self::Url | Self::AuthMode | Self::Site | Self::Theme | Self::ShowDonate => true,
            Self::Insecure => mode != AuthMode::Cloud,
            Self::ApiKey => matches!(mode, AuthMode::ApiKey | AuthMode::Hybrid | AuthMode::Cloud),
            Self::HostId => mode == AuthMode::Cloud,
            Self::Username | Self::Password => {
                matches!(mode, AuthMode::Session | AuthMode::Hybrid)
            }
        }
    }
}

// ── Construction & config ───────────────────────────────────────────

impl SettingsScreen {
    pub fn new() -> Self {
        let mut screen = Self {
            focused: false,
            action_tx: None,
            state: SettingsState::Editing,
            active_field: SettingsField::Url,
            draft: ControllerProfileDraft::default(),
            auth_mode_index: 0,
            show_password: false,
            profile_name: "default".into(),
            test_error: None,
            throbber_state: throbber_widgets_tui::ThrobberState::default(),
            last_area: std::cell::Cell::new(ratatui::layout::Rect::default()),
            theme_selector: std::cell::RefCell::new(None),
            show_donate: true,
        };
        screen.load_from_config();
        screen
    }

    fn load_from_config(&mut self) {
        let Ok(cfg) = crate::config::load_config() else {
            return;
        };

        self.show_donate = cfg.defaults.show_donate;

        let profile_name = cfg.default_profile.as_deref().unwrap_or("default");
        let Some(profile) = cfg.profiles.get(profile_name) else {
            return;
        };

        self.profile_name = profile_name.to_string();
        self.draft = ControllerProfileDraft::from_profile(profile);
        self.auth_mode_index = AuthMode::ALL
            .iter()
            .position(|&mode| mode == self.draft.auth_mode)
            .unwrap_or(0);
    }

    // ── Field navigation ────────────────────────────────────────────

    /// All visible fields in display order.
    pub(super) fn visible_fields(&self) -> Vec<SettingsField> {
        SettingsField::ALL
            .iter()
            .copied()
            .filter(|f| f.visible_for(self.draft.auth_mode))
            .collect()
    }

    /// Build the full form layout with section headers interleaved.
    pub(super) fn form_layout(&self) -> Vec<FormEntry> {
        let mut entries = Vec::new();
        let mut current_section = "";

        for field in self.visible_fields() {
            let section = field.section();
            if section != current_section {
                entries.push(FormEntry::Section(section));
                current_section = section;
            }
            entries.push(FormEntry::Field(field, field.row_height()));
        }

        entries
    }

    pub(super) fn focus_next(&mut self) {
        let fields = self.visible_fields();
        if fields.is_empty() {
            return;
        }
        let pos = fields
            .iter()
            .position(|&f| f == self.active_field)
            .unwrap_or(0);
        self.active_field = fields[(pos + 1) % fields.len()];
    }

    pub(super) fn focus_prev(&mut self) {
        let fields = self.visible_fields();
        if fields.is_empty() {
            return;
        }
        let pos = fields
            .iter()
            .position(|&f| f == self.active_field)
            .unwrap_or(0);
        self.active_field = fields[(pos + fields.len() - 1) % fields.len()];
    }

    pub(super) fn clamp_focus(&mut self) {
        if !self.active_field.visible_for(self.draft.auth_mode) {
            self.active_field = SettingsField::AuthMode;
        }
    }

    // ── Auth mode cycling ───────────────────────────────────────────

    pub(super) fn cycle_auth_mode_previous(&mut self) {
        if self.auth_mode_index == 0 {
            self.auth_mode_index = AuthMode::ALL.len() - 1;
        } else {
            self.auth_mode_index -= 1;
        }
        self.draft.auth_mode = AuthMode::ALL[self.auth_mode_index];
        self.draft.apply_auth_mode_defaults();
        self.clamp_focus();
    }

    pub(super) fn cycle_auth_mode_next(&mut self) {
        if self.auth_mode_index < AuthMode::ALL.len() - 1 {
            self.auth_mode_index += 1;
        } else {
            self.auth_mode_index = 0;
        }
        self.draft.auth_mode = AuthMode::ALL[self.auth_mode_index];
        self.draft.apply_auth_mode_defaults();
        self.clamp_focus();
    }

    // ── Text input access ───────────────────────────────────────────

    pub(super) fn active_input_mut(&mut self) -> Option<&mut String> {
        match self.active_field {
            SettingsField::Url => Some(&mut self.draft.url),
            SettingsField::ApiKey => Some(&mut self.draft.api_key),
            SettingsField::HostId => Some(&mut self.draft.host_id),
            SettingsField::Username => Some(&mut self.draft.username),
            SettingsField::Password => Some(&mut self.draft.password),
            SettingsField::Site => Some(&mut self.draft.site),
            SettingsField::AuthMode
            | SettingsField::Insecure
            | SettingsField::Theme
            | SettingsField::ShowDonate => None,
        }
    }

    // ── Preferences persistence ─────────────────────────────────────

    pub(super) fn toggle_show_donate(&mut self) {
        self.show_donate = !self.show_donate;

        if let Ok(mut cfg) = crate::config::load_config() {
            cfg.defaults.show_donate = self.show_donate;
            let _ = crate::config::save_config(&cfg);
        }

        if let Some(ref tx) = self.action_tx {
            let _ = tx.send(crate::tui::action::Action::SetShowDonate(self.show_donate));
        }
    }

    pub(super) fn save_theme_preference(theme_id: &str) {
        if let Ok(mut cfg) = crate::config::load_config() {
            cfg.defaults.theme = Some(theme_id.to_string());
            let _ = crate::config::save_config(&cfg);
        }
    }

    // ── Connection test / apply ─────────────────────────────────────

    pub(super) fn validate(&self) -> std::result::Result<(), String> {
        self.draft.validate_complete()
    }

    pub(super) fn submit_connection_test(&mut self) {
        if let Err(message) = self.validate() {
            self.test_error = Some(message);
        } else {
            self.start_connection_test();
        }
    }

    fn build_profile(&self) -> crate::config::Profile {
        self.draft.to_profile()
    }

    fn start_connection_test(&mut self) {
        self.state = SettingsState::Testing;
        self.test_error = None;

        let profile = self.build_profile();
        let profile_name = self.profile_name.clone();

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
                            let mut cfg = crate::config::load_config().unwrap_or_default();
                            cfg.profiles.insert(profile_name.clone(), profile);
                            if cfg.default_profile.is_none() {
                                cfg.default_profile = Some(profile_name.clone());
                            }
                            if let Err(error) = crate::config::save_config(&cfg) {
                                Err(format!("Connected, but failed to save config: {error}"))
                            } else {
                                Ok(())
                            }
                        }
                        Err(error) => Err(format!("{error}")),
                    }
                }
                Err(error) => Err(format!("{error}")),
            };

            let _ = tx.send(crate::tui::action::Action::SettingsTestResult(result));
        });
    }

    pub(super) fn send_apply(&self) {
        let profile = self.build_profile();
        let Some(tx) = self.action_tx.clone() else {
            return;
        };

        let defaults = crate::config::load_config()
            .map(|cfg| cfg.defaults)
            .unwrap_or_default();
        match crate::config::profile_to_controller_config(&profile, &self.profile_name, &defaults) {
            Ok(config) => {
                let _ = tx.send(crate::tui::action::Action::SettingsApply {
                    profile_name: self.profile_name.clone(),
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
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{AuthMode, SettingsField, SettingsScreen};

    fn test_screen() -> SettingsScreen {
        let mut screen = SettingsScreen::new();
        screen.draft.url = "https://console.example.com".into();
        screen.draft.api_key = "api-key".into();
        screen.draft.host_id = "console-123".into();
        screen.draft.username = "bliss".into();
        screen.draft.password = "hunter2".into();
        screen.draft.site = "default".into();
        screen.draft.auth_mode = AuthMode::ApiKey;
        screen.auth_mode_index = 0;
        screen.active_field = SettingsField::Url;
        screen.test_error = None;
        screen
    }

    #[test]
    fn field_layout_hides_unused_credentials() {
        let mut screen = test_screen();
        screen.draft.auth_mode = AuthMode::Session;
        screen.auth_mode_index = 1;

        let fields = screen.visible_fields();

        assert!(!fields.contains(&SettingsField::ApiKey));
        assert!(fields.contains(&SettingsField::Username));
        assert!(fields.contains(&SettingsField::Password));
    }

    #[test]
    fn field_layout_shows_cloud_host_id_and_hides_insecure() {
        let mut screen = test_screen();
        screen.draft.auth_mode = AuthMode::Cloud;
        screen.auth_mode_index = 3;
        screen.draft.apply_auth_mode_defaults();

        let fields = screen.visible_fields();

        assert!(fields.contains(&SettingsField::ApiKey));
        assert!(fields.contains(&SettingsField::HostId));
        assert!(!fields.contains(&SettingsField::Username));
        assert!(!fields.contains(&SettingsField::Password));
        assert!(!fields.contains(&SettingsField::Insecure));
    }

    #[test]
    fn build_profile_omits_non_selected_auth_fields() {
        let mut screen = test_screen();
        screen.draft.auth_mode = AuthMode::ApiKey;

        let profile = screen.build_profile();

        assert_eq!(profile.api_key.as_deref(), Some("api-key"));
        assert_eq!(profile.username, None);
        assert_eq!(profile.password, None);
    }

    #[test]
    fn build_profile_preserves_cloud_host_id() {
        let mut screen = test_screen();
        screen.draft.auth_mode = AuthMode::Cloud;
        screen.draft.apply_auth_mode_defaults();

        let profile = screen.build_profile();

        assert_eq!(profile.auth_mode, "cloud");
        assert_eq!(profile.host_id.as_deref(), Some("console-123"));
        assert_eq!(profile.username, None);
        assert_eq!(profile.password, None);
    }

    #[test]
    fn validate_requires_legacy_credentials() {
        let mut screen = test_screen();
        screen.draft.auth_mode = AuthMode::Session;
        screen.draft.username.clear();

        assert_eq!(screen.validate(), Err("Username cannot be empty".into()));
    }

    #[test]
    fn sections_group_fields_correctly() {
        let screen = test_screen();

        let conn: Vec<_> = SettingsField::ALL
            .iter()
            .filter(|f| f.section() == "Connection")
            .collect();
        assert!(conn.contains(&&SettingsField::Url));
        assert!(!conn.contains(&&SettingsField::Theme));

        let appearance: Vec<_> = SettingsField::ALL
            .iter()
            .filter(|f| f.section() == "Appearance")
            .collect();
        assert!(appearance.contains(&&SettingsField::Theme));
        assert!(appearance.contains(&&SettingsField::ShowDonate));

        drop(screen);
    }
}
