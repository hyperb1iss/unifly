//! Settings screen — edit controller config from within the TUI.
//!
//! Opened with `,`, not in the tab bar. Esc cancels without saving.
//! On successful connection test, saves config and emits `SettingsApply`
//! so the app can reconnect with the new configuration.

use std::cell::RefCell;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use opaline::ThemeSelectorAction;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use tokio::sync::mpsc::UnboundedSender;

use crate::tui::action::Action;
use crate::tui::component::Component;
use crate::tui::theme;

// ── Types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsState {
    Editing,
    Testing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthMode {
    ApiKey,
    Legacy,
    Hybrid,
}

impl AuthMode {
    const ALL: [AuthMode; 3] = [Self::ApiKey, Self::Legacy, Self::Hybrid];

    fn label(self) -> &'static str {
        match self {
            Self::ApiKey => "API Key (Integration API)",
            Self::Legacy => "Username / Password (Legacy API)",
            Self::Hybrid => "Hybrid (API Key + Credentials)",
        }
    }

    fn config_value(self) -> &'static str {
        match self {
            Self::ApiKey => "integration",
            Self::Legacy => "legacy",
            Self::Hybrid => "hybrid",
        }
    }

    fn from_config(s: &str) -> Self {
        match s {
            "legacy" => Self::Legacy,
            "hybrid" => Self::Hybrid,
            _ => Self::ApiKey,
        }
    }
}

/// Which form field has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsField {
    Url,
    AuthMode,
    ApiKey,
    Username,
    Password,
    Site,
    Insecure,
    Theme,
}

impl SettingsField {
    /// All fields in tab order. Credential fields are skipped dynamically
    /// based on current auth mode.
    const ALL: [SettingsField; 8] = [
        Self::Url,
        Self::AuthMode,
        Self::ApiKey,
        Self::Username,
        Self::Password,
        Self::Site,
        Self::Insecure,
        Self::Theme,
    ];

    /// Whether this field is visible for the given auth mode.
    fn visible_for(self, mode: AuthMode) -> bool {
        match self {
            Self::Url | Self::AuthMode | Self::Site | Self::Insecure | Self::Theme => true,
            Self::ApiKey => matches!(mode, AuthMode::ApiKey | AuthMode::Hybrid),
            Self::Username | Self::Password => matches!(mode, AuthMode::Legacy | AuthMode::Hybrid),
        }
    }
}

// ── Component ────────────────────────────────────────────────────────

pub struct SettingsScreen {
    focused: bool,
    action_tx: Option<UnboundedSender<Action>>,
    state: SettingsState,
    active_field: SettingsField,
    // Form data
    url_input: String,
    auth_mode: AuthMode,
    auth_mode_index: usize,
    api_key_input: String,
    username_input: String,
    password_input: String,
    site_input: String,
    insecure: bool,
    show_password: bool,
    // Profile name we're editing
    profile_name: String,
    // Test state
    test_error: Option<String>,
    throbber_state: throbber_widgets_tui::ThrobberState,
    // Last full-screen area, for mouse hit-testing
    last_area: std::cell::Cell<Rect>,
    // Theme selector overlay (RefCell for interior mutability in render)
    theme_selector: RefCell<Option<opaline::ThemeSelectorState>>,
}

impl Default for SettingsScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsScreen {
    /// Create a new settings screen, pre-populated from the current config.
    pub fn new() -> Self {
        let mut screen = Self {
            focused: false,
            action_tx: None,
            state: SettingsState::Editing,
            active_field: SettingsField::Url,
            url_input: "https://192.168.1.1".into(),
            auth_mode: AuthMode::ApiKey,
            auth_mode_index: 0,
            api_key_input: String::new(),
            username_input: String::new(),
            password_input: String::new(),
            site_input: "default".into(),
            insecure: true,
            show_password: false,
            profile_name: "default".into(),
            test_error: None,
            throbber_state: throbber_widgets_tui::ThrobberState::default(),
            last_area: std::cell::Cell::new(Rect::default()),
            theme_selector: RefCell::new(None),
        };
        screen.load_from_config();
        screen
    }

    /// Pre-populate form fields from the saved config file.
    fn load_from_config(&mut self) {
        let Ok(cfg) = crate::config::load_config() else {
            return;
        };

        let profile_name = cfg.default_profile.as_deref().unwrap_or("default");
        let Some(profile) = cfg.profiles.get(profile_name) else {
            return;
        };

        self.profile_name = profile_name.to_string();
        self.url_input.clone_from(&profile.controller);
        self.site_input.clone_from(&profile.site);
        self.insecure = profile.insecure.unwrap_or(false);

        self.auth_mode = AuthMode::from_config(&profile.auth_mode);
        self.auth_mode_index = AuthMode::ALL
            .iter()
            .position(|&m| std::mem::discriminant(&m) == std::mem::discriminant(&self.auth_mode))
            .unwrap_or(0);

        if let Some(ref key) = profile.api_key {
            self.api_key_input.clone_from(key);
        }
        if let Some(ref user) = profile.username {
            self.username_input.clone_from(user);
        }
        if let Some(ref pass) = profile.password {
            self.password_input.clone_from(pass);
        }
    }

    // ── Field navigation ─────────────────────────────────────────────

    /// Visible fields in tab order for the current auth mode.
    fn visible_fields(&self) -> Vec<SettingsField> {
        SettingsField::ALL
            .iter()
            .copied()
            .filter(|f| f.visible_for(self.auth_mode))
            .collect()
    }

    fn focus_next(&mut self) {
        let fields = self.visible_fields();
        let pos = fields
            .iter()
            .position(|&f| f == self.active_field)
            .unwrap_or(0);
        self.active_field = fields[(pos + 1) % fields.len()];
    }

    fn focus_prev(&mut self) {
        let fields = self.visible_fields();
        let pos = fields
            .iter()
            .position(|&f| f == self.active_field)
            .unwrap_or(0);
        self.active_field = fields[(pos + fields.len() - 1) % fields.len()];
    }

    /// Ensure active field is still visible after auth mode change.
    fn clamp_focus(&mut self) {
        if !self.active_field.visible_for(self.auth_mode) {
            self.active_field = SettingsField::AuthMode;
        }
    }

    // ── Active input ─────────────────────────────────────────────────

    fn active_input_mut(&mut self) -> Option<&mut String> {
        match self.active_field {
            SettingsField::Url => Some(&mut self.url_input),
            SettingsField::ApiKey => Some(&mut self.api_key_input),
            SettingsField::Username => Some(&mut self.username_input),
            SettingsField::Password => Some(&mut self.password_input),
            SettingsField::Site => Some(&mut self.site_input),
            SettingsField::AuthMode | SettingsField::Insecure | SettingsField::Theme => None,
        }
    }

    // ── Theme persistence ────────────────────────────────────────────

    fn save_theme_preference(theme_id: &str) {
        if let Ok(mut cfg) = crate::config::load_config() {
            cfg.defaults.theme = Some(theme_id.to_string());
            let _ = crate::config::save_config(&cfg);
        }
    }

    // ── Validation & submission ──────────────────────────────────────

    fn validate(&self) -> std::result::Result<(), String> {
        let trimmed = self.url_input.trim();
        if trimmed.is_empty() {
            return Err("URL cannot be empty".into());
        }
        if trimmed.parse::<url::Url>().is_err() {
            return Err("Invalid URL format".into());
        }
        match self.auth_mode {
            AuthMode::ApiKey => {
                if self.api_key_input.trim().is_empty() {
                    return Err("API key cannot be empty".into());
                }
            }
            AuthMode::Legacy => {
                if self.username_input.trim().is_empty() {
                    return Err("Username cannot be empty".into());
                }
                if self.password_input.is_empty() {
                    return Err("Password cannot be empty".into());
                }
            }
            AuthMode::Hybrid => {
                if self.api_key_input.trim().is_empty() {
                    return Err("API key cannot be empty".into());
                }
                if self.username_input.trim().is_empty() {
                    return Err("Username cannot be empty".into());
                }
                if self.password_input.is_empty() {
                    return Err("Password cannot be empty".into());
                }
            }
        }
        if self.site_input.trim().is_empty() {
            return Err("Site name cannot be empty".into());
        }
        Ok(())
    }

    fn build_profile(&self) -> crate::config::Profile {
        crate::config::Profile {
            controller: self.url_input.trim().to_string(),
            site: self.site_input.trim().to_string(),
            auth_mode: self.auth_mode.config_value().to_string(),
            api_key: match self.auth_mode {
                AuthMode::ApiKey | AuthMode::Hybrid => Some(self.api_key_input.trim().to_string()),
                AuthMode::Legacy => None,
            },
            api_key_env: None,
            username: match self.auth_mode {
                AuthMode::Legacy | AuthMode::Hybrid => Some(self.username_input.trim().to_string()),
                AuthMode::ApiKey => None,
            },
            password: match self.auth_mode {
                AuthMode::Legacy | AuthMode::Hybrid => Some(self.password_input.clone()),
                AuthMode::ApiKey => None,
            },
            ca_cert: None,
            insecure: Some(self.insecure),
            timeout: None,
            totp_env: None,
        }
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
            let result = match crate::config::profile_to_controller_config(&profile, &profile_name)
            {
                Ok(config) => {
                    let controller = unifly_api::Controller::new(config);
                    match controller.connect().await {
                        Ok(()) => {
                            controller.disconnect().await;
                            // Save config on success
                            let mut cfg = crate::config::load_config().unwrap_or_default();
                            cfg.profiles.insert(profile_name.clone(), profile);
                            if cfg.default_profile.is_none() {
                                cfg.default_profile = Some(profile_name.clone());
                            }
                            if let Err(e) = crate::config::save_config(&cfg) {
                                Err(format!("Connected, but failed to save config: {e}"))
                            } else {
                                Ok(())
                            }
                        }
                        Err(e) => Err(format!("{e}")),
                    }
                }
                Err(e) => Err(format!("{e}")),
            };

            let _ = tx.send(Action::SettingsTestResult(result));
        });
    }

    fn send_apply(&self) {
        let profile = self.build_profile();
        let Some(tx) = self.action_tx.clone() else {
            return;
        };

        match crate::config::profile_to_controller_config(&profile, &self.profile_name) {
            Ok(config) => {
                let _ = tx.send(Action::SettingsApply {
                    profile_name: self.profile_name.clone(),
                    config: Box::new(config),
                });
            }
            Err(e) => {
                let _ = tx.send(Action::Notify(crate::tui::action::Notification::error(
                    format!("{e}"),
                )));
            }
        }
    }

    // ── Rendering ────────────────────────────────────────────────────

    #[allow(clippy::unused_self)]
    fn render_centered_panel(&self, frame: &mut Frame, area: Rect) -> Rect {
        let panel_w = 62u16.min(area.width.saturating_sub(4));
        let panel_h = 32u16.min(area.height.saturating_sub(2));
        let x = (area.width.saturating_sub(panel_w)) / 2;
        let y = (area.height.saturating_sub(panel_h)) / 2;
        let panel = Rect::new(area.x + x, area.y + y, panel_w, panel_h);

        frame.render_widget(
            Block::default().style(Style::default().bg(theme::bg_base())),
            panel,
        );

        let block = Block::default()
            .title(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    "Settings",
                    Style::default()
                        .fg(theme::accent_secondary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
            ]))
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::accent_primary()));

        let inner = block.inner(panel);
        frame.render_widget(block, panel);
        inner
    }

    #[allow(clippy::unused_self)]
    fn render_input_field(
        &self,
        frame: &mut Frame,
        area: Rect,
        label: &str,
        value: &str,
        active: bool,
        masked: bool,
    ) {
        if area.height < 3 {
            return;
        }

        let label_area = Rect::new(area.x, area.y, area.width, 1);
        let label_style = if active {
            Style::default().fg(theme::accent_secondary())
        } else {
            Style::default().fg(theme::text_secondary())
        };
        frame.render_widget(Paragraph::new(Span::styled(label, label_style)), label_area);

        let display = if masked && !value.is_empty() {
            "\u{25CF}".repeat(value.len())
        } else {
            value.to_string()
        };

        let border_color = if active {
            theme::accent_primary()
        } else {
            theme::border_unfocused()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color));

        let block_area = Rect::new(area.x, area.y + 1, area.width, 3.min(area.height - 1));
        let inner = block.inner(block_area);
        frame.render_widget(block, block_area);

        let text = if active {
            format!("{display}\u{2588}")
        } else {
            display
        };
        frame.render_widget(
            Paragraph::new(Span::styled(
                text,
                Style::default().fg(theme::accent_secondary()),
            )),
            inner,
        );
    }

    fn render_auth_selector(&self, frame: &mut Frame, area: Rect) {
        if area.height < 3 {
            return;
        }

        let active = self.active_field == SettingsField::AuthMode;
        let label_style = if active {
            Style::default().fg(theme::accent_secondary())
        } else {
            Style::default().fg(theme::text_secondary())
        };
        frame.render_widget(
            Paragraph::new(Span::styled("  Auth Mode", label_style)),
            Rect::new(area.x, area.y, area.width, 1),
        );

        let border_color = if active {
            theme::accent_primary()
        } else {
            theme::border_unfocused()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color));

        let block_area = Rect::new(area.x, area.y + 1, area.width, 3.min(area.height - 1));
        let inner = block.inner(block_area);
        frame.render_widget(block, block_area);

        // Inline selector: ◂ label ▸
        let arrow_style = if active {
            Style::default().fg(theme::accent_primary())
        } else {
            Style::default().fg(theme::border_unfocused())
        };
        let value_style = if active {
            Style::default()
                .fg(theme::accent_secondary())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::text_secondary())
        };
        let label = self.auth_mode.label();
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" \u{25C2} ", arrow_style),
                Span::styled(label, value_style),
                Span::styled(" \u{25B8}", arrow_style),
            ])),
            inner,
        );
    }

    #[allow(clippy::unused_self)]
    fn render_toggle(&self, frame: &mut Frame, area: Rect, label: &str, value: bool, active: bool) {
        if area.height < 1 {
            return;
        }
        let marker = if value { "[\u{2713}]" } else { "[ ]" };
        let marker_style = if active {
            Style::default().fg(theme::accent_primary())
        } else if value {
            Style::default().fg(theme::success())
        } else {
            Style::default().fg(theme::border_unfocused())
        };
        let label_style = if active {
            Style::default().fg(theme::accent_secondary())
        } else {
            Style::default().fg(theme::text_secondary())
        };

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("  {marker} "), marker_style),
                Span::styled(label, label_style),
            ])),
            area,
        );
    }

    fn render_theme_field(&self, frame: &mut Frame, area: Rect) {
        if area.height < 1 {
            return;
        }

        let active = self.active_field == SettingsField::Theme;
        let label_style = if active {
            Style::default().fg(theme::accent_secondary())
        } else {
            Style::default().fg(theme::text_secondary())
        };

        let theme_name = opaline::current().meta.name.clone();
        let value_style = if active {
            Style::default()
                .fg(theme::accent_primary())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::text_primary())
        };

        let hint = if active { "  (Enter to change)" } else { "" };

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  Theme: ", label_style),
                Span::styled(theme_name, value_style),
                Span::styled(hint, Style::default().fg(theme::border_unfocused())),
            ])),
            area,
        );
    }

    fn render_editing(&self, frame: &mut Frame, area: Rect) {
        // Dynamic layout based on visible fields
        let has_api_key = SettingsField::ApiKey.visible_for(self.auth_mode);
        let has_legacy = SettingsField::Username.visible_for(self.auth_mode);

        // Calculate total height needed
        let mut constraints = vec![
            Constraint::Length(4), // URL
            Constraint::Length(4), // Auth mode inline selector
        ];
        if has_api_key {
            constraints.push(Constraint::Length(4)); // API Key
        }
        if has_legacy {
            constraints.push(Constraint::Length(4)); // Username
            constraints.push(Constraint::Length(4)); // Password
        }
        constraints.push(Constraint::Length(4)); // Site
        constraints.push(Constraint::Length(1)); // Insecure toggle
        constraints.push(Constraint::Length(2)); // Theme field (spacer + row)
        constraints.push(Constraint::Min(0)); // Spacer

        let fields_area = Rect::new(
            area.x + 1,
            area.y,
            area.width.saturating_sub(2),
            area.height,
        );
        let chunks = Layout::vertical(constraints).split(fields_area);

        let mut i = 0;

        // URL
        self.render_input_field(
            frame,
            chunks[i],
            "  Controller URL",
            &self.url_input,
            self.active_field == SettingsField::Url,
            false,
        );
        i += 1;

        // Auth Mode
        self.render_auth_selector(frame, chunks[i]);
        i += 1;

        // API Key
        if has_api_key {
            self.render_input_field(
                frame,
                chunks[i],
                "  API Key",
                &self.api_key_input,
                self.active_field == SettingsField::ApiKey,
                true,
            );
            i += 1;
        }

        // Username + Password
        if has_legacy {
            self.render_input_field(
                frame,
                chunks[i],
                "  Username",
                &self.username_input,
                self.active_field == SettingsField::Username,
                false,
            );
            i += 1;

            self.render_input_field(
                frame,
                chunks[i],
                "  Password",
                &self.password_input,
                self.active_field == SettingsField::Password,
                !self.show_password,
            );
            i += 1;
        }

        // Site
        self.render_input_field(
            frame,
            chunks[i],
            "  Site",
            &self.site_input,
            self.active_field == SettingsField::Site,
            false,
        );
        i += 1;

        // Insecure toggle
        self.render_toggle(
            frame,
            chunks[i],
            "Skip TLS verification (insecure)",
            self.insecure,
            self.active_field == SettingsField::Insecure,
        );
        i += 1;

        // Theme field
        self.render_theme_field(frame, chunks[i]);
    }

    fn render_testing(&self, frame: &mut Frame, area: Rect) {
        let layout = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Min(0),
        ])
        .split(area);

        let throbber = throbber_widgets_tui::Throbber::default()
            .label("  Testing connection...")
            .style(Style::default().fg(theme::accent_secondary()))
            .throbber_style(Style::default().fg(theme::accent_primary()));

        frame.render_stateful_widget(throbber, layout[1], &mut self.throbber_state.clone());

        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("  Connecting to {}", self.url_input.trim()),
                Style::default().fg(theme::border_unfocused()),
            )),
            layout[2],
        );
    }

    fn render_key_hints(&self, frame: &mut Frame, area: Rect) {
        let hints = match self.state {
            SettingsState::Editing => {
                if self.active_field == SettingsField::AuthMode {
                    "\u{25C2}/\u{25B8} select  Tab next  Enter test & save  Esc cancel"
                } else if self.active_field == SettingsField::Insecure {
                    "Space toggle  Tab next  Enter test & save  Esc cancel"
                } else if self.active_field == SettingsField::Password {
                    "Ctrl+U reveal  Tab next  Enter test & save  Esc cancel"
                } else if self.active_field == SettingsField::Theme {
                    "Enter choose theme  Tab next  Esc cancel"
                } else {
                    "Tab next  Shift+Tab prev  Enter test & save  Esc cancel"
                }
            }
            SettingsState::Testing => "Esc cancel",
        };

        frame.render_widget(
            Paragraph::new(Span::styled(hints, theme::key_hint())).alignment(Alignment::Center),
            area,
        );
    }
}

// ── Component impl ───────────────────────────────────────────────────

impl Component for SettingsScreen {
    fn init(&mut self, action_tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(action_tx);
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        // ── Theme selector intercept ─────────────────────────────────
        {
            let mut selector = self.theme_selector.borrow_mut();
            if let Some(ref mut sel) = *selector {
                return match sel.handle_key(key) {
                    ThemeSelectorAction::Select(id) => {
                        SettingsScreen::save_theme_preference(&id);
                        *selector = None;
                        Ok(None)
                    }
                    ThemeSelectorAction::Cancel => {
                        *selector = None;
                        Ok(None)
                    }
                    _ => Ok(None),
                };
            }
        }

        match self.state {
            SettingsState::Testing => {
                if key.code == KeyCode::Esc {
                    self.state = SettingsState::Editing;
                    self.test_error = None;
                }
                return Ok(None);
            }
            SettingsState::Editing => {}
        }

        // Clear test error on any input
        self.test_error = None;

        match self.active_field {
            SettingsField::AuthMode => match key.code {
                KeyCode::Up | KeyCode::Left | KeyCode::Char('k' | 'h') => {
                    if self.auth_mode_index > 0 {
                        self.auth_mode_index -= 1;
                    } else {
                        self.auth_mode_index = AuthMode::ALL.len() - 1;
                    }
                    self.auth_mode = AuthMode::ALL[self.auth_mode_index];
                    self.clamp_focus();
                }
                KeyCode::Down | KeyCode::Right | KeyCode::Char('j' | 'l') => {
                    if self.auth_mode_index < AuthMode::ALL.len() - 1 {
                        self.auth_mode_index += 1;
                    } else {
                        self.auth_mode_index = 0;
                    }
                    self.auth_mode = AuthMode::ALL[self.auth_mode_index];
                    self.clamp_focus();
                }
                KeyCode::Tab => self.focus_next(),
                KeyCode::BackTab => self.focus_prev(),
                KeyCode::Enter => {
                    if let Err(msg) = self.validate() {
                        self.test_error = Some(msg);
                    } else {
                        self.start_connection_test();
                    }
                }
                KeyCode::Esc => return Ok(Some(Action::CloseSettings)),
                _ => {}
            },
            SettingsField::Insecure => match key.code {
                KeyCode::Char(' ') => self.insecure = !self.insecure,
                KeyCode::Tab => self.focus_next(),
                KeyCode::BackTab => self.focus_prev(),
                KeyCode::Enter => {
                    if let Err(msg) = self.validate() {
                        self.test_error = Some(msg);
                    } else {
                        self.start_connection_test();
                    }
                }
                KeyCode::Esc => return Ok(Some(Action::CloseSettings)),
                _ => {}
            },
            SettingsField::Theme => match key.code {
                KeyCode::Enter => {
                    *self.theme_selector.borrow_mut() = Some(
                        opaline::ThemeSelectorState::with_current_selected()
                            .with_derive(crate::tui::theme::derive_tokens),
                    );
                }
                KeyCode::Tab => self.focus_next(),
                KeyCode::BackTab => self.focus_prev(),
                KeyCode::Esc => return Ok(Some(Action::CloseSettings)),
                _ => {}
            },
            // Text input fields
            _ => match key.code {
                KeyCode::Tab => self.focus_next(),
                KeyCode::BackTab => self.focus_prev(),
                KeyCode::Enter => {
                    if let Err(msg) = self.validate() {
                        self.test_error = Some(msg);
                    } else {
                        self.start_connection_test();
                    }
                }
                KeyCode::Esc => return Ok(Some(Action::CloseSettings)),
                KeyCode::Backspace => {
                    if let Some(input) = self.active_input_mut() {
                        input.pop();
                    }
                }
                KeyCode::Char(c) => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'u' {
                        self.show_password = !self.show_password;
                    } else if let Some(input) = self.active_input_mut() {
                        input.push(c);
                    }
                }
                _ => {}
            },
        }

        Ok(None)
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        if self.state != SettingsState::Editing {
            return Ok(None);
        }

        // Ignore mouse when theme selector is open
        if self.theme_selector.borrow().is_some() {
            return Ok(None);
        }

        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
            let row = mouse.row;

            // Reconstruct the content area geometry from last_area
            let area = self.last_area.get();
            if area.width == 0 {
                return Ok(None);
            }

            let panel_w = 62u16.min(area.width.saturating_sub(4));
            let panel_h = 32u16.min(area.height.saturating_sub(2));
            let px = (area.width.saturating_sub(panel_w)) / 2 + area.x;
            let py = (area.height.saturating_sub(panel_h)) / 2 + area.y;

            // Inner = panel minus border (1 each side) + layout spacer (1)
            let content_y = py + 1 + 1; // border + spacer
            let fields_x = px + 1 + 1; // border + padding
            let fields_w = panel_w.saturating_sub(4);

            // Walk the field layout to find which field was clicked
            let has_api_key = SettingsField::ApiKey.visible_for(self.auth_mode);
            let has_legacy = SettingsField::Username.visible_for(self.auth_mode);

            let mut y = content_y;
            let field_order = {
                let mut v = vec![(SettingsField::Url, 4u16), (SettingsField::AuthMode, 4)];
                if has_api_key {
                    v.push((SettingsField::ApiKey, 4));
                }
                if has_legacy {
                    v.push((SettingsField::Username, 4));
                    v.push((SettingsField::Password, 4));
                }
                v.push((SettingsField::Site, 4));
                v.push((SettingsField::Insecure, 1));
                v.push((SettingsField::Theme, 2));
                v
            };

            for (field, height) in &field_order {
                if row >= y && row < y + height {
                    // Hit! Focus this field
                    self.active_field = *field;

                    // Special: clicking insecure toggles it
                    if *field == SettingsField::Insecure {
                        self.insecure = !self.insecure;
                    }

                    // Special: clicking theme opens the selector
                    if *field == SettingsField::Theme {
                        *self.theme_selector.borrow_mut() = Some(
                            opaline::ThemeSelectorState::with_current_selected()
                                .with_derive(crate::tui::theme::derive_tokens),
                        );
                    }

                    // Special: clicking auth mode cycles it
                    if *field == SettingsField::AuthMode {
                        // Check which side of the field was clicked to determine direction
                        let mid_x = fields_x + fields_w / 2;
                        if mouse.column < mid_x {
                            // Left half — previous
                            if self.auth_mode_index > 0 {
                                self.auth_mode_index -= 1;
                            } else {
                                self.auth_mode_index = AuthMode::ALL.len() - 1;
                            }
                        } else {
                            // Right half — next
                            if self.auth_mode_index < AuthMode::ALL.len() - 1 {
                                self.auth_mode_index += 1;
                            } else {
                                self.auth_mode_index = 0;
                            }
                        }
                        self.auth_mode = AuthMode::ALL[self.auth_mode_index];
                        self.clamp_focus();
                    }

                    break;
                }
                y += height;
            }
        }

        Ok(None)
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        match action {
            Action::SettingsTestResult(result) => {
                match result {
                    Ok(()) => {
                        // Test passed — apply immediately
                        self.send_apply();
                    }
                    Err(msg) => {
                        self.state = SettingsState::Editing;
                        self.test_error = Some(msg.clone());
                    }
                }
            }
            Action::Tick => {
                if self.state == SettingsState::Testing {
                    self.throbber_state.calc_next();
                }
            }
            _ => {}
        }
        Ok(None)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        self.last_area.set(area);

        // Full-screen dark background
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::bg_base())),
            area,
        );

        let inner = self.render_centered_panel(frame, area);

        let layout = Layout::vertical([
            Constraint::Length(1), // spacer
            Constraint::Min(1),    // content
            Constraint::Length(1), // error
            Constraint::Length(1), // hints
        ])
        .split(inner);

        // Error line
        if let Some(ref err) = self.test_error {
            frame.render_widget(
                Paragraph::new(Span::styled(err, Style::default().fg(theme::error())))
                    .alignment(Alignment::Center),
                layout[2],
            );
        }

        self.render_key_hints(frame, layout[3]);

        match self.state {
            SettingsState::Editing => self.render_editing(frame, layout[1]),
            SettingsState::Testing => self.render_testing(frame, layout[1]),
        }

        // ── Theme selector overlay (rendered on top) ─────────────────
        let mut selector = self.theme_selector.borrow_mut();
        if let Some(ref mut sel) = *selector {
            let overlay = centered_rect(area, 80, 28);
            frame.render_widget(Clear, overlay);
            frame.render_stateful_widget(opaline::ThemeSelector::new(), overlay, sel);
        }
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn id(&self) -> &'static str {
        "settings"
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Compute a centered rectangle of (cols x rows) within `area`.
fn centered_rect(area: Rect, cols: u16, rows: u16) -> Rect {
    let w = cols.min(area.width.saturating_sub(2));
    let h = rows.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}
