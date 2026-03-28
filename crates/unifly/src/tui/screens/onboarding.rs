//! Onboarding wizard — guides first-time setup when no config exists.
//!
//! Flow: Welcome → URL → AuthMode → Credentials → Site → Testing → Done
//!
//! On completion, saves the config to disk and emits `OnboardingComplete`
//! with the built `ControllerConfig` so the app can connect immediately.

use std::collections::HashMap;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use tokio::sync::mpsc::UnboundedSender;

use crate::tui::action::Action;
use crate::tui::component::Component;
use crate::tui::theme;

// ── State types ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WizardStep {
    Welcome,
    Url,
    AuthMode,
    Credentials,
    Site,
    Testing,
    Done,
}

impl WizardStep {
    /// 1-indexed step number for the progress indicator.
    fn index(self) -> usize {
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

    fn description(self) -> &'static str {
        match self {
            Self::ApiKey => "Recommended for most setups",
            Self::Legacy => "For stats, events, and admin operations",
            Self::Hybrid => "Full access to both API surfaces",
        }
    }

    fn config_value(self) -> &'static str {
        match self {
            Self::ApiKey => "integration",
            Self::Legacy => "legacy",
            Self::Hybrid => "hybrid",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CredentialField {
    ApiKey,
    Username,
    Password,
}

// ── Component ───────────────────────────────────────────────────────

pub struct OnboardingScreen {
    focused: bool,
    action_tx: Option<UnboundedSender<Action>>,
    step: WizardStep,
    // Inputs
    url_input: String,
    auth_mode: AuthMode,
    auth_mode_index: usize,
    api_key_input: String,
    username_input: String,
    password_input: String,
    site_input: String,
    cred_field: CredentialField,
    show_password: bool,
    // Test state
    testing: bool,
    test_error: Option<String>,
    // Validation error for current step
    error: Option<String>,
    // Throbber state
    throbber_state: throbber_widgets_tui::ThrobberState,
}

impl Default for OnboardingScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl OnboardingScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            action_tx: None,
            step: WizardStep::Welcome,
            url_input: "https://192.168.1.1".into(),
            auth_mode: AuthMode::ApiKey,
            auth_mode_index: 0,
            api_key_input: String::new(),
            username_input: String::new(),
            password_input: String::new(),
            site_input: "default".into(),
            cred_field: CredentialField::ApiKey,
            show_password: false,
            testing: false,
            test_error: None,
            error: None,
            throbber_state: throbber_widgets_tui::ThrobberState::default(),
        }
    }

    /// Advance to the next step, with validation.
    fn advance(&mut self) {
        self.error = None;
        match self.step {
            WizardStep::Welcome => self.step = WizardStep::Url,
            WizardStep::Url => {
                let trimmed = self.url_input.trim();
                if trimmed.is_empty() {
                    self.error = Some("URL cannot be empty".into());
                    return;
                }
                if trimmed.parse::<url::Url>().is_err() {
                    self.error = Some("Invalid URL format".into());
                    return;
                }
                self.step = WizardStep::AuthMode;
            }
            WizardStep::AuthMode => {
                self.auth_mode = AuthMode::ALL[self.auth_mode_index];
                // Set default credential field based on auth mode
                self.cred_field = match self.auth_mode {
                    AuthMode::ApiKey | AuthMode::Hybrid => CredentialField::ApiKey,
                    AuthMode::Legacy => CredentialField::Username,
                };
                self.step = WizardStep::Credentials;
            }
            WizardStep::Credentials => {
                if let Err(msg) = self.validate_credentials() {
                    self.error = Some(msg);
                    return;
                }
                self.step = WizardStep::Site;
            }
            WizardStep::Site => {
                if self.site_input.trim().is_empty() {
                    self.error = Some("Site name cannot be empty".into());
                    return;
                }
                self.step = WizardStep::Testing;
                self.start_connection_test();
            }
            WizardStep::Testing => {
                // Can't advance manually — test result moves us
            }
            WizardStep::Done => {
                self.send_completion();
            }
        }
    }

    /// Go back to the previous step.
    fn go_back(&mut self) {
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

    fn validate_credentials(&self) -> std::result::Result<(), String> {
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
        Ok(())
    }

    /// Build a Profile from wizard inputs.
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
            insecure: Some(true), // Local controllers are typically self-signed
            timeout: None,
            totp_env: None,
        }
    }

    /// Spawn an async connection test.
    fn start_connection_test(&mut self) {
        self.testing = true;
        self.test_error = None;

        let profile = self.build_profile();
        let profile_name = "default".to_string();

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
                            let cfg = crate::config::Config {
                                default_profile: Some(profile_name),
                                defaults: crate::config::Defaults::default(),
                                profiles: {
                                    let mut m = HashMap::new();
                                    m.insert("default".to_string(), profile);
                                    m
                                },
                            };
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

            let _ = tx.send(Action::OnboardingTestResult(result));
        });
    }

    /// Send OnboardingComplete to the app.
    fn send_completion(&self) {
        let profile = self.build_profile();
        let profile_name = "default";

        let Some(tx) = self.action_tx.clone() else {
            return;
        };

        match crate::config::profile_to_controller_config(&profile, profile_name) {
            Ok(config) => {
                let _ = tx.send(Action::OnboardingComplete {
                    profile_name: profile_name.to_string(),
                    config: Box::new(config),
                });
            }
            Err(e) => {
                self.action_tx.as_ref().map(|tx| {
                    tx.send(Action::Notify(crate::tui::action::Notification::error(
                        format!("{e}"),
                    )))
                });
            }
        }
    }

    /// Get the active input string for the current text-input step.
    fn active_input_mut(&mut self) -> Option<&mut String> {
        match self.step {
            WizardStep::Url => Some(&mut self.url_input),
            WizardStep::Site => Some(&mut self.site_input),
            WizardStep::Credentials => match self.cred_field {
                CredentialField::ApiKey => Some(&mut self.api_key_input),
                CredentialField::Username => Some(&mut self.username_input),
                CredentialField::Password => Some(&mut self.password_input),
            },
            _ => None,
        }
    }

    /// Cycle through credential fields for the current auth mode.
    fn next_cred_field(&mut self) {
        self.cred_field = match (self.auth_mode, self.cred_field) {
            (AuthMode::Legacy | AuthMode::Hybrid, CredentialField::Username) => {
                CredentialField::Password
            }
            (AuthMode::Hybrid, CredentialField::Password) | (AuthMode::ApiKey, _) => {
                CredentialField::ApiKey
            }
            (AuthMode::Legacy, _) | (AuthMode::Hybrid, CredentialField::ApiKey) => {
                CredentialField::Username
            }
        };
    }

    // ── Rendering helpers ───────────────────────────────────────────

    #[allow(clippy::unused_self)]
    fn render_centered_panel(&self, frame: &mut Frame, area: Rect) -> Rect {
        let panel_w = 62u16.min(area.width.saturating_sub(4));
        let panel_h = 22u16.min(area.height.saturating_sub(2));
        let x = (area.width.saturating_sub(panel_w)) / 2;
        let y = (area.height.saturating_sub(panel_h)) / 2;
        let panel = Rect::new(area.x + x, area.y + y, panel_w, panel_h);

        // Background
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::bg_base())),
            panel,
        );

        // Border
        let block = Block::default()
            .title(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    "UniFi Setup",
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

    fn render_step_indicator(&self, frame: &mut Frame, area: Rect) {
        let steps = ["URL", "Auth", "Keys", "Site", "Test"];
        let current = self.step.index();

        let spans: Vec<Span> = steps
            .iter()
            .enumerate()
            .flat_map(|(i, label)| {
                let step_num = i + 1;
                let style = match step_num.cmp(&current) {
                    std::cmp::Ordering::Equal => Style::default()
                        .fg(theme::accent_primary())
                        .add_modifier(Modifier::BOLD),
                    std::cmp::Ordering::Less => Style::default().fg(theme::success()),
                    std::cmp::Ordering::Greater => Style::default().fg(theme::border_unfocused()),
                };
                let sep = if i < steps.len() - 1 {
                    Span::styled(" > ", Style::default().fg(theme::border_unfocused()))
                } else {
                    Span::raw("")
                };
                vec![Span::styled(format!("{step_num} {label}"), style), sep]
            })
            .collect();

        let line = Line::from(spans);
        frame.render_widget(Paragraph::new(line).alignment(Alignment::Center), area);
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

    fn render_key_hints(&self, frame: &mut Frame, area: Rect) {
        let hints = match self.step {
            WizardStep::Welcome => "Enter continue  Ctrl+C quit",
            WizardStep::Url | WizardStep::Site => "Enter next  Esc back  Ctrl+C quit",
            WizardStep::AuthMode => "Up/Down select  Enter confirm  Esc back",
            WizardStep::Credentials => "Tab next field  Enter next  Esc back",
            WizardStep::Testing => "Esc cancel",
            WizardStep::Done => "Enter connect!",
        };

        frame.render_widget(
            Paragraph::new(Span::styled(hints, theme::key_hint())).alignment(Alignment::Center),
            area,
        );
    }
}

impl Component for OnboardingScreen {
    fn init(&mut self, action_tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(action_tx);
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        // Clear step error on any input
        if key.code != KeyCode::Enter {
            self.error = None;
        }

        match self.step {
            WizardStep::Welcome => {
                if key.code == KeyCode::Enter {
                    self.advance();
                }
            }

            WizardStep::Url | WizardStep::Site => match key.code {
                KeyCode::Enter => self.advance(),
                KeyCode::Esc => self.go_back(),
                KeyCode::Backspace => {
                    if let Some(input) = self.active_input_mut() {
                        input.pop();
                    }
                }
                KeyCode::Char(c) => {
                    if let Some(input) = self.active_input_mut() {
                        input.push(c);
                    }
                }
                _ => {}
            },

            WizardStep::AuthMode => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if self.auth_mode_index > 0 {
                        self.auth_mode_index -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if self.auth_mode_index < AuthMode::ALL.len() - 1 {
                        self.auth_mode_index += 1;
                    }
                }
                KeyCode::Enter => self.advance(),
                KeyCode::Esc => self.go_back(),
                _ => {}
            },

            WizardStep::Credentials => match key.code {
                KeyCode::Tab => self.next_cred_field(),
                KeyCode::Enter => self.advance(),
                KeyCode::Esc => self.go_back(),
                KeyCode::Backspace => {
                    if let Some(input) = self.active_input_mut() {
                        input.pop();
                    }
                }
                KeyCode::Char(c) => {
                    // Ctrl+U to toggle password visibility
                    if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'u' {
                        self.show_password = !self.show_password;
                    } else if let Some(input) = self.active_input_mut() {
                        input.push(c);
                    }
                }
                _ => {}
            },

            WizardStep::Testing => {
                if key.code == KeyCode::Esc {
                    self.go_back();
                }
            }

            WizardStep::Done => match key.code {
                KeyCode::Enter => self.send_completion(),
                KeyCode::Esc => self.go_back(),
                _ => {}
            },
        }

        Ok(None)
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        match action {
            Action::OnboardingTestResult(result) => {
                self.testing = false;
                match result {
                    Ok(()) => {
                        self.test_error = None;
                        self.step = WizardStep::Done;
                    }
                    Err(msg) => {
                        self.test_error = Some(msg.clone());
                    }
                }
            }
            Action::Tick => {
                if self.testing {
                    self.throbber_state.calc_next();
                }
            }
            _ => {}
        }
        Ok(None)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        // Full-screen dark background
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::bg_base())),
            area,
        );

        let inner = self.render_centered_panel(frame, area);

        // Layout: step indicator, content, error, hints
        let layout = Layout::vertical([
            Constraint::Length(2), // step indicator + spacer
            Constraint::Min(1),    // content
            Constraint::Length(1), // error
            Constraint::Length(1), // hints
        ])
        .split(inner);

        self.render_step_indicator(frame, layout[0]);
        self.render_key_hints(frame, layout[3]);

        // Error line
        if let Some(ref err) = self.error {
            frame.render_widget(
                Paragraph::new(Span::styled(err, Style::default().fg(theme::error())))
                    .alignment(Alignment::Center),
                layout[2],
            );
        } else if let Some(ref err) = self.test_error {
            frame.render_widget(
                Paragraph::new(Span::styled(err, Style::default().fg(theme::error())))
                    .alignment(Alignment::Center),
                layout[2],
            );
        }

        // Step content
        let content = layout[1];
        match self.step {
            WizardStep::Welcome => self.render_welcome(frame, content),
            WizardStep::Url => self.render_url(frame, content),
            WizardStep::AuthMode => self.render_auth_mode(frame, content),
            WizardStep::Credentials => self.render_credentials(frame, content),
            WizardStep::Site => self.render_site(frame, content),
            WizardStep::Testing => self.render_testing(frame, content),
            WizardStep::Done => self.render_done(frame, content),
        }
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

// ── Step renderers ──────────────────────────────────────────────────

impl OnboardingScreen {
    #[allow(clippy::unused_self)]
    fn render_welcome(&self, frame: &mut Frame, area: Rect) {
        let layout = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Min(0),
        ])
        .split(area);

        frame.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                "Welcome to UniFi TUI",
                Style::default()
                    .fg(theme::accent_secondary())
                    .add_modifier(Modifier::BOLD),
            )]))
            .alignment(Alignment::Center),
            layout[0],
        );

        let desc = vec![
            Line::from(Span::styled(
                "No configuration found. This wizard will help you",
                Style::default().fg(theme::text_secondary()),
            )),
            Line::from(Span::styled(
                "connect to your UniFi controller.",
                Style::default().fg(theme::text_secondary()),
            )),
        ];
        frame.render_widget(Paragraph::new(desc).alignment(Alignment::Center), layout[1]);

        frame.render_widget(
            Paragraph::new(Span::styled(
                "Press Enter to begin",
                Style::default().fg(theme::accent_primary()),
            ))
            .alignment(Alignment::Center),
            layout[2],
        );
    }

    fn render_url(&self, frame: &mut Frame, area: Rect) {
        let layout = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(4),
            Constraint::Min(0),
        ])
        .split(area);

        frame.render_widget(
            Paragraph::new(Span::styled(
                "Enter your controller URL",
                Style::default()
                    .fg(theme::accent_secondary())
                    .add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Center),
            layout[0],
        );

        self.render_input_field(
            frame,
            layout[1],
            "  Controller URL",
            &self.url_input,
            true,
            false,
        );
    }

    fn render_auth_mode(&self, frame: &mut Frame, area: Rect) {
        let layout = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).split(area);

        frame.render_widget(
            Paragraph::new(Span::styled(
                "Choose authentication method",
                Style::default()
                    .fg(theme::accent_secondary())
                    .add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Center),
            layout[0],
        );

        let list_area = Rect::new(
            layout[1].x + 3,
            layout[1].y,
            layout[1].width.saturating_sub(6),
            layout[1].height,
        );

        let mut lines = Vec::new();
        for (i, mode) in AuthMode::ALL.iter().enumerate() {
            let selected = i == self.auth_mode_index;
            let marker = if selected { "\u{25B8} " } else { "  " };
            let marker_style = if selected {
                Style::default().fg(theme::accent_primary())
            } else {
                Style::default().fg(theme::border_unfocused())
            };
            let label_style = if selected {
                Style::default()
                    .fg(theme::accent_secondary())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::text_secondary())
            };

            lines.push(Line::from(vec![
                Span::styled(marker, marker_style),
                Span::styled(mode.label(), label_style),
            ]));
            lines.push(Line::from(Span::styled(
                format!("    {}", mode.description()),
                Style::default().fg(theme::border_unfocused()),
            )));
            lines.push(Line::from(""));
        }

        frame.render_widget(Paragraph::new(lines), list_area);
    }

    fn render_credentials(&self, frame: &mut Frame, area: Rect) {
        let layout = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).split(area);

        frame.render_widget(
            Paragraph::new(Span::styled(
                "Enter credentials",
                Style::default()
                    .fg(theme::accent_secondary())
                    .add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Center),
            layout[0],
        );

        let fields_area = Rect::new(
            layout[1].x + 2,
            layout[1].y,
            layout[1].width.saturating_sub(4),
            layout[1].height,
        );

        let mut y_offset = 0u16;

        if matches!(self.auth_mode, AuthMode::ApiKey | AuthMode::Hybrid) {
            let input_area = Rect::new(
                fields_area.x,
                fields_area.y + y_offset,
                fields_area.width,
                4,
            );
            self.render_input_field(
                frame,
                input_area,
                "  API Key",
                &self.api_key_input,
                self.cred_field == CredentialField::ApiKey,
                true, // masked
            );
            y_offset += 5;
        }

        if matches!(self.auth_mode, AuthMode::Legacy | AuthMode::Hybrid) {
            let input_area = Rect::new(
                fields_area.x,
                fields_area.y + y_offset,
                fields_area.width,
                4,
            );
            self.render_input_field(
                frame,
                input_area,
                "  Username",
                &self.username_input,
                self.cred_field == CredentialField::Username,
                false,
            );
            y_offset += 5;

            let input_area = Rect::new(
                fields_area.x,
                fields_area.y + y_offset,
                fields_area.width,
                4,
            );
            self.render_input_field(
                frame,
                input_area,
                "  Password",
                &self.password_input,
                self.cred_field == CredentialField::Password,
                !self.show_password,
            );
        }
    }

    fn render_site(&self, frame: &mut Frame, area: Rect) {
        let layout = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(4),
            Constraint::Min(0),
        ])
        .split(area);

        frame.render_widget(
            Paragraph::new(Span::styled(
                "Enter site name",
                Style::default()
                    .fg(theme::accent_secondary())
                    .add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Center),
            layout[0],
        );

        self.render_input_field(frame, layout[1], "  Site", &self.site_input, true, false);
    }

    fn render_testing(&self, frame: &mut Frame, area: Rect) {
        let layout = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Min(0),
        ])
        .split(area);

        if self.testing {
            let throbber = throbber_widgets_tui::Throbber::default()
                .label("  Testing connection...")
                .style(Style::default().fg(theme::accent_secondary()))
                .throbber_style(Style::default().fg(theme::accent_primary()));

            frame.render_stateful_widget(throbber, layout[0], &mut self.throbber_state.clone());

            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!("  Connecting to {}", self.url_input.trim()),
                    Style::default().fg(theme::border_unfocused()),
                )),
                layout[1],
            );
        } else if let Some(ref err) = self.test_error {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(Span::styled(
                        "  Connection failed",
                        Style::default()
                            .fg(theme::error())
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        format!("  {err}"),
                        Style::default().fg(theme::error()),
                    )),
                ])
                .wrap(Wrap { trim: false }),
                area,
            );
        }
    }

    fn render_done(&self, frame: &mut Frame, area: Rect) {
        let layout = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(4),
            Constraint::Length(2),
            Constraint::Min(0),
        ])
        .split(area);

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "  \u{2713} ",
                    Style::default()
                        .fg(theme::success())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "Connection successful!",
                    Style::default()
                        .fg(theme::success())
                        .add_modifier(Modifier::BOLD),
                ),
            ]))
            .alignment(Alignment::Center),
            layout[0],
        );

        let saved_path = crate::config::config_path();
        let details = vec![
            Line::from(Span::styled(
                "  Profile: default".to_string(),
                Style::default().fg(theme::text_secondary()),
            )),
            Line::from(Span::styled(
                format!("  Controller: {}", self.url_input.trim()),
                Style::default().fg(theme::text_secondary()),
            )),
            Line::from(Span::styled(
                format!("  Config saved: {}", saved_path.display()),
                Style::default().fg(theme::text_secondary()),
            )),
        ];
        frame.render_widget(Paragraph::new(details), layout[1]);

        frame.render_widget(
            Paragraph::new(Span::styled(
                "Press Enter to launch the dashboard",
                Style::default().fg(theme::accent_primary()),
            ))
            .alignment(Alignment::Center),
            layout[2],
        );
    }
}
