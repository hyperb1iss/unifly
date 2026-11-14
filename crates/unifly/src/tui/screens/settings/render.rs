use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use super::{FieldHit, FormEntry, Leave, Message, SettingsField, SettingsScreen, SettingsState};
use crate::tui::forms::controller_profile::{DraftField, EXAMPLE_URL};
use crate::tui::forms::probe::Tone;
use crate::tui::forms::widgets::{
    self, FieldView, message_lines, render_choice_row, render_info_row, render_input_row,
    render_section, render_toggle_row, render_value_row, render_waiting,
};
use crate::tui::theme;
use crate::tui::widgets::click::{self, Click, ClickLine, Hint};

/// The panel never grows wider than this, so a row stays readable.
const PANEL_COLUMNS: u16 = 76;
/// Rows kept for the message under the form when the terminal has them,
/// so the panel does not change size as messages come and go.
const MESSAGE_ROWS: u16 = 6;

fn panel_rect(area: Rect, form_rows: usize) -> Rect {
    let wanted = u16::try_from(form_rows)
        .unwrap_or(u16::MAX)
        .saturating_add(MESSAGE_ROWS + 4);
    let width = PANEL_COLUMNS.min(area.width.saturating_sub(4));
    let height = wanted.min(area.height.saturating_sub(2));
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

fn hint_line(hints: &[Hint]) -> ClickLine<'static, Click> {
    let mut line = ClickLine::default();
    line.push_hints(hints, theme::key_hint_key(), theme::key_hint());
    line
}

fn centered_rect(area: Rect, cols: u16, rows: u16) -> Rect {
    let width = cols.min(area.width.saturating_sub(2));
    let height = rows.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

impl SettingsScreen {
    pub(super) fn render_screen(&self, frame: &mut Frame, area: Rect) {
        self.hits.borrow_mut().clear();
        self.field_hits.borrow_mut().clear();

        frame.render_widget(
            Block::default().style(Style::default().bg(theme::bg_base())),
            area,
        );

        let entries = self.form_layout();
        let inner = self.render_panel(frame, panel_rect(area, entries.len()));
        if inner.is_empty() {
            return;
        }

        let paragraphs = self.message_paragraphs();
        let borrowed: Vec<(Tone, &str)> = paragraphs
            .iter()
            .map(|(tone, text)| (*tone, text.as_str()))
            .collect();
        let message = message_lines(
            &borrowed,
            usize::from(inner.width.saturating_sub(4)),
            usize::from((inner.height.saturating_sub(1) / 3).min(MESSAGE_ROWS)),
        );

        // Whatever the form does not need goes to the message, up to its
        // reserve; a form taller than the panel scrolls instead.
        let form_rows = u16::try_from(entries.len()).unwrap_or(u16::MAX);
        let spare = inner.height.saturating_sub(1).saturating_sub(form_rows);
        let message_rows = u16::try_from(message.len())
            .unwrap_or(u16::MAX)
            .max(MESSAGE_ROWS.min(spare.saturating_sub(1)));
        let layout = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(u16::from(spare > message_rows)),
            Constraint::Length(message_rows),
            Constraint::Length(1),
        ])
        .split(inner);

        match self.state {
            SettingsState::Testing => self.render_waiting(frame, layout[0], "Testing connection"),
            SettingsState::Saving => self.render_waiting(frame, layout[0], "Saving"),
            _ => self.render_form(frame, layout[0], &entries),
        }
        let message_area = Rect {
            x: layout[2].x + 2,
            width: layout[2].width.saturating_sub(4),
            ..layout[2]
        };
        frame.render_widget(Paragraph::new(message), message_area);
        self.render_key_hints(frame, layout[3]);

        self.render_theme_selector(frame, area);
    }

    fn render_theme_selector(&self, frame: &mut Frame, area: Rect) {
        let mut selector = self.theme_selector.borrow_mut();
        if let Some(ref mut theme_selector) = *selector {
            let overlay = centered_rect(area, 80, 28);
            // A tap outside the picker cancels it, as Esc does. Taps inside
            // do nothing: the picker keeps its cursor private, so a tapped
            // row cannot be told apart from the highlighted one.
            let mut hits = self.hits.borrow_mut();
            hits.push(area, Click::Key(KeyCode::Esc.into()));
            hits.push(overlay, Click::Absorb);
            frame.render_widget(Clear, overlay);
            frame.render_stateful_widget(opaline::ThemeSelector::new(), overlay, theme_selector);
        }
    }

    /// Draw the panel and its title, which names the profile being edited.
    fn render_panel(&self, frame: &mut Frame, panel: Rect) -> Rect {
        let title_style = Style::default()
            .fg(theme::accent_secondary())
            .add_modifier(Modifier::BOLD);
        let mut title = vec![Span::raw(" "), Span::styled("Settings", title_style)];
        if !self.private {
            title.push(Span::styled(
                format!(" \u{00B7} {}", self.profile_name),
                Style::default().fg(theme::text_secondary()),
            ));
        }
        title.push(Span::raw(" "));

        let block = Block::default()
            .title(Line::from(title))
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::accent_primary()))
            .style(Style::default().bg(theme::bg_base()));
        let inner = block.inner(panel);
        frame.render_widget(block, panel);
        inner
    }

    fn render_waiting(&self, frame: &mut Frame, area: Rect, label: &str) {
        let detail = if self.private {
            "Esc cancels a test".to_owned()
        } else {
            format!("Connecting to {}", self.draft.target())
        };
        render_waiting(frame, area, &self.throbber_state, label, &detail);
    }

    /// The first row to draw so the focused field is in view. The wheel
    /// may scroll away from it until a key moves the focus again.
    fn resolve_scroll(&self, entries: &[FormEntry], visible: usize) -> usize {
        let mut scroll = self.scroll.get().min(entries.len().saturating_sub(visible));
        let active = entries
            .iter()
            .position(|entry| *entry == FormEntry::Field(self.active_field));
        if self.follow_focus.get()
            && visible > 0
            && let Some(active) = active
        {
            if active < scroll {
                scroll = active;
            } else if active >= scroll + visible {
                scroll = active + 1 - visible;
            }
            // Keep a field's section rule with it when there is room.
            if scroll == active
                && visible > 1
                && active > 0
                && matches!(entries[active - 1], FormEntry::Section(_))
            {
                scroll = active - 1;
            }
        }
        self.scroll.set(scroll);
        scroll
    }

    fn render_form(&self, frame: &mut Frame, area: Rect, entries: &[FormEntry]) {
        // The last column is kept for the marks that say the form scrolls.
        let rows = Rect {
            x: area.x + 1,
            width: area.width.saturating_sub(3),
            ..area
        };
        let visible = usize::from(rows.height);
        let scroll = self.resolve_scroll(entries, visible);

        for (line, entry) in (rows.y..rows.bottom()).zip(entries.iter().skip(scroll)) {
            let row = Rect {
                y: line,
                height: 1,
                ..rows
            };
            match entry {
                FormEntry::Section(label) => render_section(frame, row, label),
                FormEntry::Info(label, value) => render_info_row(frame, row, label, value),
                FormEntry::Field(field) => {
                    self.field_hits
                        .borrow_mut()
                        .push(row, FieldHit::Focus(*field));
                    self.render_field(frame, row, *field);
                }
            }
        }

        let mark = |frame: &mut Frame, y: u16, glyph: &'static str| {
            let cell = Rect::new(area.right().saturating_sub(1), y, 1, 1).intersection(area);
            frame.render_widget(
                Paragraph::new(Span::styled(
                    glyph,
                    Style::default().fg(theme::accent_primary()),
                )),
                cell,
            );
        };
        if scroll > 0 {
            mark(frame, area.y, "\u{25B2}");
        }
        if scroll + visible < entries.len() && area.height > 0 {
            mark(frame, area.bottom() - 1, "\u{25BC}");
        }
    }

    fn render_field(&self, frame: &mut Frame, row: Rect, field: SettingsField) {
        let active = self.active_field == field;
        let overridden = self.override_on(field);
        match field {
            SettingsField::Profile => self.render_profile(frame, row, active),
            SettingsField::AuthMode => {
                let note = overridden
                    .clone()
                    .or_else(|| {
                        let unknown = self.draft.unknown_auth_mode.as_ref()?;
                        Some(format!("was \"{unknown}\""))
                    })
                    .unwrap_or_default();
                let arrows = render_choice_row(
                    frame,
                    row,
                    field.label(),
                    self.draft.auth_mode.label(),
                    active,
                    &note,
                );
                self.register_arrows(field, arrows);
            }
            SettingsField::Site if self.picks_site() => self.render_site_picker(frame, row, active),
            SettingsField::Insecure => render_toggle_row(
                frame,
                row,
                field.label(),
                self.draft.insecure(),
                active,
                overridden
                    .as_deref()
                    .unwrap_or("accept a self-signed certificate"),
            ),
            SettingsField::ShowDonate => render_toggle_row(
                frame,
                row,
                field.label(),
                self.show_donate,
                active,
                "in the status bar",
            ),
            SettingsField::Theme => render_value_row(
                frame,
                row,
                field.label(),
                &opaline::current().meta.name,
                active,
                if active { "Enter to change" } else { "" },
            ),
            _ => {
                // A value the form shows may still lose to a variable.
                let tag = overridden.or_else(|| self.draft.env_in_use(field.draft_field()?));
                self.render_text_field(frame, row, field, tag.as_deref());
            }
        }
    }

    fn render_text_field(
        &self,
        frame: &mut Frame,
        row: Rect,
        field: SettingsField,
        tag: Option<&str>,
    ) {
        let Some(draft_field) = field.draft_field() else {
            return;
        };
        let Some(input) = self.draft.input(draft_field) else {
            return;
        };
        let placeholder = self.placeholder(draft_field);
        render_input_row(
            frame,
            row,
            &FieldView {
                label: field.label(),
                input,
                active: self.active_field == field,
                masked: self.private || !self.reveal.shows(draft_field),
                placeholder: placeholder.as_deref(),
                tag,
            },
        );
    }

    /// What an empty field shows: where its value comes from instead, or
    /// what belongs in it.
    fn placeholder(&self, field: DraftField) -> Option<String> {
        if let Some(source) = self.draft.secret_source(field) {
            return Some(source);
        }
        match field {
            DraftField::Url => Some(EXAMPLE_URL.to_owned()),
            DraftField::Totp => Some("6 digits, only for two-factor sign-in".to_owned()),
            DraftField::CaCert => Some("none: the system's certificates".to_owned()),
            DraftField::Timeout => Some(format!("{} seconds (default)", self.default_timeout)),
            _ => None,
        }
    }

    fn render_profile(&self, frame: &mut Frame, row: Rect, active: bool) {
        if self.state == SettingsState::Naming {
            render_input_row(
                frame,
                row,
                &FieldView {
                    label: "New profile",
                    input: &self.new_profile,
                    active: true,
                    masked: false,
                    placeholder: Some("a name, for example lab"),
                    tag: None,
                },
            );
            return;
        }
        let position = self
            .profiles
            .iter()
            .position(|name| *name == self.profile_name)
            .map_or(1, |index| index + 1);
        let shown = if self.private {
            format!("profile {position}")
        } else {
            self.profile_name.clone()
        };
        let note = format!("{position} of {}", self.profiles.len());
        let arrows = render_choice_row(frame, row, "Profile", &shown, active, &note);
        self.register_arrows(SettingsField::Profile, arrows);
    }

    fn render_site_picker(&self, frame: &mut Frame, row: Rect, active: bool) {
        let slug = self.draft.site.text();
        let position = self
            .known_sites
            .iter()
            .position(|site| site.internal_reference == slug);
        let shown = match position.and_then(|index| self.known_sites.get(index)) {
            _ if self.private => "\u{25CF}\u{25CF}\u{25CF}\u{25CF}".to_owned(),
            Some(site) if site.display_name != site.internal_reference => {
                format!("{} ({})", site.internal_reference, site.display_name)
            }
            Some(site) => site.internal_reference.clone(),
            None => slug.to_owned(),
        };
        let mut note = match position {
            Some(index) => format!("{} of {}", index + 1, self.known_sites.len()),
            None => "not on this controller".to_owned(),
        };
        if let Some(flag) = self.override_on(SettingsField::Site) {
            note = format!("{note} \u{00B7} {flag}");
        }
        let arrows = render_choice_row(frame, row, "Site", &shown, active, &note);
        self.register_arrows(SettingsField::Site, arrows);
    }

    fn register_arrows(&self, field: SettingsField, arrows: widgets::ChoiceArrows) {
        let mut hits = self.field_hits.borrow_mut();
        hits.push(arrows.previous, FieldHit::Step(field, -1));
        hits.push(arrows.next, FieldHit::Step(field, 1));
    }

    /// What the area under the form says: a question, the last result, or
    /// else what the focused field is for.
    pub(super) fn message_paragraphs(&self) -> Vec<(Tone, String)> {
        if let SettingsState::Confirming(to) = &self.state {
            let what = match to {
                Leave::Close => "Close Settings",
                Leave::Profile(_) => "Switch profiles",
                Leave::NewProfile(_) => "Set up a new profile",
            };
            return vec![(
                Tone::Notice,
                format!("{what} and drop the changes you have not saved?"),
            )];
        }
        match &self.message {
            Some(Message::Failure(failure)) => failure
                .paragraphs(self.private)
                .into_iter()
                .map(|(tone, text)| (tone, text.to_owned()))
                .collect(),
            Some(Message::Error(text)) => vec![(Tone::Error, text.clone())],
            Some(Message::Notice(text)) => vec![(Tone::Notice, text.clone())],
            None if self.state == SettingsState::Editing => self.field_help(),
            None => Vec::new(),
        }
    }

    fn field_help(&self) -> Vec<(Tone, String)> {
        let field = self.active_field;
        let mut out = Vec::new();
        if let Some(flag) = self.override_on(field) {
            out.push((
                Tone::Notice,
                format!(
                    "Launched with {flag}: it overrides this value until unifly is started \
                     without it."
                ),
            ));
        }
        let help = match field {
            SettingsField::Profile => {
                "The profile this form edits. Picking another one connects to it once its \
                 test passes."
            }
            SettingsField::AuthMode if self.draft.unknown_auth_mode.is_some() => {
                "The profile's auth_mode is not one unifly knows. Saving writes the mode \
                 shown here."
            }
            SettingsField::Totp => {
                "Used once, for the next test, and never saved. Leave it empty unless the \
                 account signs in with a second factor."
            }
            SettingsField::Site if self.picks_site() => {
                "These are the sites on this controller. Type to enter a name by hand."
            }
            SettingsField::Insecure => {
                "UniFi consoles ship with a self-signed certificate, which only connects \
                 with this on or with a CA certificate set."
            }
            SettingsField::CaCert => {
                "Path to a PEM file with the certificate authority that signed the \
                 controller's certificate."
            }
            SettingsField::Theme => {
                "Applies at once and is kept for the next launch, unless --theme or \
                 UNIFLY_THEME names another."
            }
            SettingsField::ShowDonate => "Applies at once.",
            _ => "",
        };
        if !help.is_empty() {
            out.push((Tone::Detail, help.to_owned()));
        }
        out
    }

    fn render_key_hints(&self, frame: &mut Frame, area: Rect) {
        let next = Hint::button("Tab", "next", KeyCode::Tab);
        let save = Hint::button("Enter", "test & save", KeyCode::Enter);
        let close = Hint::button("Esc", "close", KeyCode::Esc);
        let hints = match &self.state {
            SettingsState::Testing => vec![Hint::button("Esc", "cancel", KeyCode::Esc)],
            SettingsState::Saving => vec![Hint::info("", "saving the profile")],
            SettingsState::Confirming(_) => vec![
                Hint::button("y", "drop them", click::char_key('y')),
                Hint::button("n", "keep editing", click::char_key('n')),
            ],
            SettingsState::Naming => vec![
                Hint::button("Enter", "set it up", KeyCode::Enter),
                Hint::button("Esc", "cancel", KeyCode::Esc),
            ],
            SettingsState::Editing if self.active_field == SettingsField::Theme => vec![
                Hint::button("Enter", "choose theme", KeyCode::Enter),
                next,
                close,
            ],
            SettingsState::Editing => {
                let mut hints = self.field_hints();
                hints.extend([next, save, close]);
                hints
            }
        };

        // A narrow panel keeps the keys that leave and save: the hints for
        // the focused field come first and go first.
        let mut hints = hints;
        let mut line = hint_line(&hints);
        while line.width() > usize::from(area.width) && hints.len() > 1 {
            hints.remove(0);
            line = hint_line(&hints);
        }
        line.register_centered(area, &mut self.hits.borrow_mut());
        frame.render_widget(
            Paragraph::new(line.into_line()).alignment(Alignment::Center),
            area,
        );
    }

    /// The keys that only the focused field takes.
    fn field_hints(&self) -> Vec<Hint> {
        let pick = Hint::info("\u{2190}/\u{2192}", "pick");
        match self.active_field {
            SettingsField::Profile => {
                vec![pick, Hint::button("n", "new profile", click::char_key('n'))]
            }
            SettingsField::AuthMode => vec![pick],
            SettingsField::Site if self.picks_site() => vec![pick],
            SettingsField::Insecure | SettingsField::ShowDonate => {
                vec![Hint::button("Space", "toggle", click::char_key(' '))]
            }
            SettingsField::ApiKey | SettingsField::Password if !self.private => {
                vec![Hint::button(
                    "Ctrl+T",
                    "show",
                    KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL),
                )]
            }
            _ => Vec::new(),
        }
    }
}
