use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use super::{OnboardingScreen, Purpose, TestPhase, WizardField, WizardHit, WizardStep};
use crate::tui::forms::controller_profile::{AuthMode, DraftField, EXAMPLE_URL};
use crate::tui::forms::probe::{FailureKind, Tone};
use crate::tui::forms::widgets::{
    BOX_ROWS, FieldView, message_lines, render_input_box, render_input_row, render_insecure_row,
    render_waiting,
};
use crate::tui::theme;
use crate::tui::widgets::click::{self, ClickLine, Hint};

const PANEL_COLUMNS: u16 = 66;
const PANEL_ROWS: u16 = 24;

fn heading_style() -> Style {
    Style::default()
        .fg(theme::accent_secondary())
        .add_modifier(Modifier::BOLD)
}

fn muted() -> Style {
    Style::default().fg(theme::text_muted())
}

/// `area` with `columns` taken off each side.
fn inset(area: Rect, columns: u16) -> Rect {
    Rect {
        x: area.x + columns.min(area.width / 2),
        width: area.width.saturating_sub(columns * 2),
        ..area
    }
}

/// Row `index` of `area`, empty once `area` runs out.
fn row(area: Rect, index: u16) -> Rect {
    Rect {
        y: area.y.saturating_add(index),
        height: 1,
        ..area
    }
    .intersection(area)
}

/// `area` from row `index` down.
fn below(area: Rect, index: u16) -> Rect {
    let skip = index.min(area.height);
    Rect {
        y: area.y + skip,
        height: area.height - skip,
        ..area
    }
}

impl OnboardingScreen {
    pub(super) fn render_screen(&self, frame: &mut Frame, area: Rect) {
        self.hits.borrow_mut().clear();
        self.targets.borrow_mut().clear();
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::bg_base())),
            area,
        );

        let inner = render_panel(frame, area);
        if inner.is_empty() {
            return;
        }

        let message = self.message_lines(inner);
        let roomy = inner.height >= 14;
        let layout = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(u16::from(roomy)),
            Constraint::Min(0),
            Constraint::Length(u16::try_from(message.len()).unwrap_or(0)),
            Constraint::Length(1),
        ])
        .split(inner);

        self.render_step_indicator(frame, layout[0]);
        frame.render_widget(
            Paragraph::new(message).alignment(Alignment::Center),
            inset(layout[3], 2),
        );
        self.render_key_hints(frame, layout[4]);

        let content = layout[2];
        match self.step {
            WizardStep::Welcome => self.render_welcome(frame, content),
            WizardStep::AuthMode => self.render_auth_mode(frame, content),
            WizardStep::Url => self.render_fields(
                frame,
                content,
                "Where is your controller?",
                "UniFi consoles ship with a self-signed certificate. Skipping verification \
                 accepts any certificate from then on; leave it on if yours has a trusted \
                 one.",
            ),
            WizardStep::Credentials => self.render_fields(frame, content, "Sign in", ""),
            WizardStep::Site if self.picks_site() => self.render_site_picker(frame, content),
            WizardStep::Site => self.render_fields(
                frame,
                content,
                "Which site?",
                "Most controllers have a single site named \"default\".",
            ),
            WizardStep::Testing => self.render_testing(frame, content),
            WizardStep::Done => self.render_done(frame, content),
        }
    }

    /// The step's error or notice, wrapped for the row above the hints.
    fn message_lines(&self, inner: Rect) -> Vec<Line<'static>> {
        let paragraph = match (&self.error, &self.notice) {
            (Some(error), _) => (Tone::Error, error.as_str()),
            (None, Some(notice)) => (Tone::Notice, notice.as_str()),
            (None, None) => return Vec::new(),
        };
        message_lines(
            &[paragraph],
            usize::from(inner.width.saturating_sub(4)),
            usize::from((inner.height / 4).clamp(1, 3)),
        )
    }

    fn render_step_indicator(&self, frame: &mut Frame, area: Rect) {
        let steps = ["Auth", "URL", "Keys", "Test", "Site"];
        let current = self.step.index();
        let mut spans = Vec::new();
        for (index, label) in steps.iter().enumerate() {
            let position = index + 1;
            let style = match position.cmp(&current) {
                std::cmp::Ordering::Equal => Style::default()
                    .fg(theme::accent_primary())
                    .add_modifier(Modifier::BOLD),
                std::cmp::Ordering::Less => Style::default().fg(theme::success()),
                std::cmp::Ordering::Greater => Style::default().fg(theme::border_unfocused()),
            };
            if index > 0 {
                spans.push(Span::styled(
                    " \u{203A} ",
                    Style::default().fg(theme::border_unfocused()),
                ));
            }
            spans.push(Span::styled(*label, style));
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
            area,
        );
    }

    /// Draw a step heading on the first row of `area` and return what is
    /// left under it, after a blank row when there is room for one.
    fn render_heading(frame: &mut Frame, area: Rect, heading: &str) -> Rect {
        frame.render_widget(
            Paragraph::new(Span::styled(heading.to_owned(), heading_style()))
                .alignment(Alignment::Center),
            row(area, 0),
        );
        below(area, if area.height >= 10 { 2 } else { 1 })
    }

    fn render_welcome(&self, frame: &mut Frame, area: Rect) {
        let body = Self::render_heading(frame, area, "Welcome to unifly");
        let name = &self.profile_name;
        let mut paragraphs: Vec<(Tone, String)> = Vec::new();
        if self.purpose == Purpose::NewProfile {
            paragraphs.push((
                Tone::Advice,
                format!(
                    "Set up a new profile, \"{name}\". Nothing changes until its connection \
                     test passes."
                ),
            ));
        } else if self.purpose == Purpose::Repair {
            paragraphs.push((
                Tone::Advice,
                format!("The profile \"{name}\" could not connect."),
            ));
            if let Some(reason) = self.repair_reason() {
                paragraphs.push((Tone::Error, reason.to_owned()));
            }
            paragraphs.push((
                Tone::Advice,
                "Step through its settings to repair it. What these steps do not show is \
                 kept as it is."
                    .into(),
            ));
        } else {
            paragraphs.push((
                Tone::Advice,
                "No configuration found. These steps connect unifly to your UniFi controller."
                    .into(),
            ));
        }
        paragraphs.push((Tone::Detail, "Press Enter to begin.".into()));

        let borrowed: Vec<(Tone, &str)> = paragraphs
            .iter()
            .map(|(tone, text)| (*tone, text.as_str()))
            .collect();
        let text = inset(body, 3);
        let lines = message_lines(&borrowed, usize::from(text.width), usize::from(text.height));
        frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), text);
    }

    fn render_auth_mode(&self, frame: &mut Frame, area: Rect) {
        let body = inset(
            Self::render_heading(frame, area, "How does unifly sign in?"),
            3,
        );
        let modes = AuthMode::ALL;
        // Each mode gets its description under it when the panel is tall
        // enough for all of them.
        let spaced = usize::from(body.height) >= modes.len() * 3 - 1;
        let stride: u16 = if spaced { 3 } else { 1 };

        for (index, mode) in modes.iter().enumerate() {
            let top = u16::try_from(index)
                .unwrap_or(u16::MAX)
                .saturating_mul(stride);
            let selected = index == self.auth_mode_index;
            let line = choice_line(mode.label(), selected);
            let label_row = row(body, top);
            frame.render_widget(Paragraph::new(line), label_row);
            self.targets
                .borrow_mut()
                .push(label_row, WizardHit::Choice(index));
            if spaced {
                let description = row(body, top + 1);
                frame.render_widget(
                    Paragraph::new(Span::styled(format!("    {}", mode.description()), muted())),
                    description,
                );
                self.targets
                    .borrow_mut()
                    .push(description, WizardHit::Choice(index));
            }
        }
        if !spaced {
            let selected = modes[self.auth_mode_index.min(modes.len() - 1)];
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!("    {}", selected.description()),
                    muted(),
                )),
                row(body, u16::try_from(modes.len()).unwrap_or(u16::MAX)),
            );
        }
    }

    /// Draw the step's fields: boxed when the panel has the rows for it,
    /// one row each when it does not, so every field stays on screen.
    fn render_fields(&self, frame: &mut Frame, area: Rect, heading: &str, help: &str) {
        let body = inset(Self::render_heading(frame, area, heading), 2);
        let fields = self.fields();
        let text_fields = fields
            .iter()
            .filter(|field| matches!(field, WizardField::Text(_)))
            .count();
        let toggles = fields.len() - text_fields;
        let boxed_rows = text_fields * usize::from(BOX_ROWS) + toggles * 2;
        let boxed = usize::from(body.height) >= boxed_rows;
        let spaced = !boxed && usize::from(body.height) >= fields.len() * 2;

        // Too short even for one row each: show the rows around the
        // focused field.
        let rows = usize::from(body.height);
        let first = if fields.len() > rows {
            self.field
                .saturating_sub(rows.saturating_sub(1))
                .min(fields.len() - rows)
        } else {
            0
        };

        let mut top = 0u16;
        for (index, field) in fields.iter().enumerate().skip(first) {
            let active = index == self.field;
            let height = match field {
                WizardField::Text(_) if boxed => BOX_ROWS,
                _ => 1,
            };
            let slot = Rect {
                y: body.y.saturating_add(top),
                height,
                ..body
            }
            .intersection(body);
            self.targets
                .borrow_mut()
                .push(slot, WizardHit::Field(index));
            match field {
                WizardField::Text(draft_field) => {
                    self.render_text_field(frame, slot, *draft_field, active, boxed);
                }
                WizardField::Insecure => {
                    render_insecure_row(frame, slot, self.draft.insecure(), active, None);
                }
            }
            top = top
                .saturating_add(height)
                .saturating_add(u16::from(spaced || (boxed && height == 1)));
        }

        let rest = below(body, top.saturating_add(1));
        let lines = message_lines(
            &[(Tone::Detail, help)],
            usize::from(rest.width),
            usize::from(rest.height),
        );
        if !help.is_empty() {
            frame.render_widget(Paragraph::new(lines), rest);
        }
    }

    fn render_text_field(
        &self,
        frame: &mut Frame,
        area: Rect,
        field: DraftField,
        active: bool,
        boxed: bool,
    ) {
        let Some(input) = self.draft.input(field) else {
            return;
        };
        let placeholder = self.draft.secret_source(field).or_else(|| match field {
            DraftField::Url => Some(EXAMPLE_URL.to_owned()),
            DraftField::Totp => Some("6 digits, only for two-factor sign-in".to_owned()),
            DraftField::Site => Some("default".to_owned()),
            _ => None,
        });
        // A repaired profile may already name where its codes come from.
        let tag = (field == DraftField::Totp)
            .then(|| self.draft.mfa_source())
            .flatten();
        let view = FieldView {
            label: field_label(field),
            input,
            active,
            masked: !self.reveal.shows(field),
            placeholder: placeholder.as_deref(),
            tag: tag.as_deref(),
        };
        if boxed {
            render_input_box(frame, area, &view);
        } else {
            render_input_row(frame, area, &view);
        }
    }

    fn render_site_picker(&self, frame: &mut Frame, area: Rect) {
        let body = inset(Self::render_heading(frame, area, "Which site?"), 3);
        let visible = usize::from(body.height);
        // Keep the selection in view when there are more sites than rows.
        let first = self
            .site_index
            .saturating_sub(visible.saturating_sub(1))
            .min(self.known_sites.len().saturating_sub(visible));
        for (line, (index, site)) in (0u16..).zip(
            self.known_sites
                .iter()
                .enumerate()
                .skip(first)
                .take(visible),
        ) {
            let label = if site.display_name == site.internal_reference {
                site.internal_reference.clone()
            } else {
                format!("{} ({})", site.internal_reference, site.display_name)
            };
            let target = row(body, line);
            frame.render_widget(
                Paragraph::new(choice_line(&label, index == self.site_index)),
                target,
            );
            self.targets
                .borrow_mut()
                .push(target, WizardHit::Choice(index));
        }
    }

    fn render_testing(&self, frame: &mut Frame, area: Rect) {
        let target = self.draft.target();
        match (self.phase, &self.failure) {
            (TestPhase::Testing, _) => render_waiting(
                frame,
                area,
                &self.throbber_state,
                "Testing connection",
                &format!("Connecting to {target}"),
            ),
            (TestPhase::Saving, _) => render_waiting(
                frame,
                area,
                &self.throbber_state,
                "Saving",
                &format!("Writing the profile \"{}\"", self.profile_name),
            ),
            (TestPhase::Idle, Some(failure)) => {
                let heading = if failure.kind == FailureKind::Save {
                    "Connected, but not saved"
                } else {
                    "Could not connect"
                };
                frame.render_widget(
                    Paragraph::new(Span::styled(
                        heading,
                        Style::default()
                            .fg(theme::error())
                            .add_modifier(Modifier::BOLD),
                    ))
                    .alignment(Alignment::Center),
                    row(area, 0),
                );
                let body = inset(below(area, if area.height >= 8 { 2 } else { 1 }), 3);
                let lines = message_lines(
                    &failure.paragraphs(false),
                    usize::from(body.width),
                    usize::from(body.height),
                );
                frame.render_widget(Paragraph::new(lines), body);
            }
            (TestPhase::Idle, None) => {}
        }
    }

    fn render_done(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "\u{2713} Connected and saved",
                Style::default()
                    .fg(theme::success())
                    .add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Center),
            row(area, 0),
        );
        let body = inset(below(area, if area.height >= 8 { 2 } else { 1 }), 3);
        let details = format!(
            "Profile {} \u{00B7} {} \u{00B7} site {}",
            self.profile_name,
            self.draft.target(),
            self.draft.site.text().trim()
        );
        let saved = format!("Saved to {}", crate::config::config_path().display());
        let mut paragraphs = vec![(Tone::Advice, details.as_str())];
        // A profile that checks no certificate says so before it is used.
        if self.skips_verification() {
            paragraphs.push((
                Tone::Notice,
                "TLS verification is off for this profile: it accepts any certificate. \
                 Settings can turn it back on or set a CA certificate.",
            ));
        }
        // Where a secret ended up is the user's to know before moving on.
        paragraphs.extend(
            self.notices
                .iter()
                .map(|notice| (Tone::Notice, notice.as_str())),
        );
        paragraphs.push((Tone::Detail, saved.as_str()));
        paragraphs.push((Tone::Advice, "Press Enter to open the dashboard."));
        let lines = message_lines(
            &paragraphs,
            usize::from(body.width),
            usize::from(body.height),
        );
        frame.render_widget(Paragraph::new(lines), body);
    }

    fn render_key_hints(&self, frame: &mut Frame, area: Rect) {
        // A narrow panel keeps the keys that move between steps: the hints
        // for the focused field come first and go first.
        let mut hints = self.hints();
        let build = |hints: &[Hint]| {
            let mut line = ClickLine::default();
            line.push_hints(hints, theme::key_hint_key(), theme::key_hint());
            line
        };
        let mut line = build(&hints);
        while line.width() > usize::from(area.width) && hints.len() > 1 {
            hints.remove(0);
            line = build(&hints);
        }
        line.register_centered(area, &mut self.hits.borrow_mut());
        frame.render_widget(
            Paragraph::new(line.into_line()).alignment(Alignment::Center),
            area,
        );
    }

    /// The keys that work on the step as it stands.
    fn hints(&self) -> Vec<Hint> {
        let back = Hint::button("Esc", "back", KeyCode::Esc);
        let next = Hint::button("Enter", "next", KeyCode::Enter);
        let select = Hint::info("\u{2191}/\u{2193}", "select");
        match self.step {
            WizardStep::Welcome if self.purpose == Purpose::NewProfile => vec![
                Hint::button("Enter", "begin", KeyCode::Enter),
                Hint::button("Esc", "cancel", KeyCode::Esc),
            ],
            WizardStep::Welcome => vec![
                Hint::button("Enter", "begin", KeyCode::Enter),
                Hint::info("Ctrl+C", "quit"),
            ],
            WizardStep::AuthMode => vec![select, next, back],
            WizardStep::Site if self.picks_site() => vec![
                select,
                Hint::button("Enter", "use it", KeyCode::Enter),
                back,
            ],
            WizardStep::Url | WizardStep::Credentials | WizardStep::Site => {
                let mut hints = self.field_hints();
                let enter = if self.step == WizardStep::Url {
                    next
                } else {
                    Hint::button("Enter", "test", KeyCode::Enter)
                };
                hints.extend([enter, back]);
                hints
            }
            WizardStep::Testing => match self.phase {
                TestPhase::Testing => vec![Hint::button("Esc", "cancel", KeyCode::Esc)],
                TestPhase::Saving => vec![Hint::info("", "saving the profile")],
                TestPhase::Idle => {
                    // First, so a narrow panel drops it before the keys
                    // that retry and go back.
                    let mut hints = Vec::new();
                    if self.offers_trust() {
                        hints.push(Hint::button(
                            "t",
                            "turn TLS verification off and retry",
                            click::char_key('t'),
                        ));
                    }
                    hints.extend([Hint::button("Enter", "retry", KeyCode::Enter), back]);
                    hints
                }
            },
            WizardStep::Done => vec![Hint::button("Enter", "open the dashboard", KeyCode::Enter)],
        }
    }

    fn field_hints(&self) -> Vec<Hint> {
        let mut hints = Vec::new();
        if self.fields().len() > 1 {
            hints.push(Hint::button("Tab", "next field", KeyCode::Tab));
        }
        match self.active_field() {
            Some(WizardField::Insecure) => {
                hints.push(Hint::button("Space", "toggle", click::char_key(' ')));
            }
            Some(WizardField::Text(field)) if field.is_secret() => hints.push(Hint::button(
                "Ctrl+T",
                "show",
                KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL),
            )),
            _ => {}
        }
        hints
    }
}

fn field_label(field: DraftField) -> &'static str {
    match field {
        DraftField::Url => "Controller URL",
        DraftField::ApiKey => "API key",
        DraftField::HostId => "Host ID",
        DraftField::Username => "Username",
        DraftField::Password => "Password",
        DraftField::Totp => "One-time code",
        DraftField::Site => "Site",
        DraftField::CaCert => "CA certificate",
        DraftField::Timeout => "Timeout",
        DraftField::AuthMode => "Auth mode",
        DraftField::Insecure => "Skip TLS verify",
    }
}

/// One row of a list: a marker on the selected row, then its label.
fn choice_line(label: &str, selected: bool) -> Line<'static> {
    let (marker, marker_style, label_style) = if selected {
        (
            "\u{25B8} ",
            Style::default().fg(theme::accent_primary()),
            heading_style(),
        )
    } else {
        (
            "  ",
            Style::default().fg(theme::border_unfocused()),
            Style::default().fg(theme::text_secondary()),
        )
    };
    Line::from(vec![
        Span::styled(marker, marker_style),
        Span::styled(label.to_owned(), label_style),
    ])
}

fn render_panel(frame: &mut Frame, area: Rect) -> Rect {
    let width = PANEL_COLUMNS.min(area.width.saturating_sub(4));
    let height = PANEL_ROWS.min(area.height.saturating_sub(2));
    let panel = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let block = Block::default()
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled("UniFi Setup", heading_style()),
            Span::raw(" "),
        ]))
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::accent_primary()))
        .style(Style::default().bg(theme::bg_base()));
    let inner = block.inner(panel);
    frame.render_widget(block, panel);
    inner
}
