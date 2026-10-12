use super::super::{FormEntry, SettingsField, SettingsScreen};

use crate::tui::forms::controller_profile::SecretOrigin;
use crate::tui::forms::widgets::{render_input_field, render_input_field_with_hint};
use crate::tui::theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

impl SettingsScreen {
    pub(super) fn render_auth_selector(&self, frame: &mut Frame, area: Rect) {
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

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" \u{25C2} ", arrow_style),
                Span::styled(self.draft.auth_mode.label(), value_style),
                Span::styled(" \u{25B8}", arrow_style),
            ])),
            inner,
        );
    }

    #[allow(clippy::unused_self)]
    pub(super) fn render_toggle(
        &self,
        frame: &mut Frame,
        area: Rect,
        label: &str,
        value: bool,
        active: bool,
    ) {
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

    pub(super) fn render_theme_field(&self, frame: &mut Frame, area: Rect) {
        if area.height < 1 {
            return;
        }

        let active = self.active_field == SettingsField::Theme;
        let label_style = if active {
            Style::default().fg(theme::accent_secondary())
        } else {
            Style::default().fg(theme::text_secondary())
        };
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
                Span::styled(opaline::current().meta.name.clone(), value_style),
                Span::styled(hint, Style::default().fg(theme::border_unfocused())),
            ])),
            area,
        );
    }

    #[allow(clippy::unused_self)]
    fn render_section_header(&self, frame: &mut Frame, area: Rect, label: &str) {
        if area.height < 1 {
            return;
        }

        let rule_len = area
            .width
            .saturating_sub(u16::try_from(label.len()).unwrap_or(0) + 5);
        let rule = "\u{2500}".repeat(usize::from(rule_len));

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" {label} "),
                    Style::default()
                        .fg(theme::accent_secondary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(rule, Style::default().fg(theme::border_unfocused())),
            ])),
            Rect::new(area.x, area.y + 1, area.width, 1),
        );
    }

    pub(super) fn render_editing(&self, frame: &mut Frame, area: Rect) {
        let form_entries = self.form_layout();

        let mut constraints: Vec<_> = form_entries
            .iter()
            .map(|entry| match entry {
                FormEntry::Section(_) => Constraint::Length(2),
                FormEntry::Field(_, height) => Constraint::Length(*height),
            })
            .collect();
        constraints.push(Constraint::Min(0));

        let fields_area = Rect::new(
            area.x + 1,
            area.y,
            area.width.saturating_sub(2),
            area.height,
        );
        let chunks = Layout::vertical(constraints).split(fields_area);

        for (entry, chunk) in form_entries.iter().zip(chunks.iter().copied()) {
            match entry {
                FormEntry::Section(label) => {
                    self.render_section_header(frame, chunk, label);
                }
                FormEntry::Field(field, _) => match field {
                    SettingsField::Url => render_input_field(
                        frame,
                        chunk,
                        "  Controller URL",
                        &self.draft.url,
                        self.active_field == SettingsField::Url,
                        false,
                    ),
                    SettingsField::AuthMode => self.render_auth_selector(frame, chunk),
                    SettingsField::ApiKey => render_input_field_with_hint(
                        frame,
                        chunk,
                        "  API Key",
                        &self.draft.api_key,
                        self.active_field == SettingsField::ApiKey,
                        true,
                        keyring_hint(self.draft.api_key_origin),
                    ),
                    SettingsField::HostId => render_input_field(
                        frame,
                        chunk,
                        "  Host ID",
                        &self.draft.host_id,
                        self.active_field == SettingsField::HostId,
                        false,
                    ),
                    SettingsField::Username => render_input_field(
                        frame,
                        chunk,
                        "  Username",
                        &self.draft.username,
                        self.active_field == SettingsField::Username,
                        false,
                    ),
                    SettingsField::Password => render_input_field_with_hint(
                        frame,
                        chunk,
                        "  Password",
                        &self.draft.password,
                        self.active_field == SettingsField::Password,
                        !self.show_password,
                        keyring_hint(self.draft.password_origin),
                    ),
                    SettingsField::Site => render_input_field(
                        frame,
                        chunk,
                        "  Site",
                        &self.draft.site,
                        self.active_field == SettingsField::Site,
                        false,
                    ),
                    SettingsField::Insecure => self.render_toggle(
                        frame,
                        chunk,
                        "Skip TLS verification (insecure)",
                        self.draft.insecure,
                        self.active_field == SettingsField::Insecure,
                    ),
                    SettingsField::Theme => self.render_theme_field(frame, chunk),
                    SettingsField::ShowDonate => self.render_toggle(
                        frame,
                        chunk,
                        "Show sponsor button",
                        self.show_donate,
                        self.active_field == SettingsField::ShowDonate,
                    ),
                },
            }
        }
    }

    pub(super) fn render_testing(&self, frame: &mut Frame, area: Rect) {
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
                format!("  Connecting to {}", self.draft.url.trim()),
                Style::default().fg(theme::border_unfocused()),
            )),
            layout[2],
        );
    }
}

/// Placeholder for a credential the keyring holds: present, but never
/// loaded into the form.
fn keyring_hint(origin: SecretOrigin) -> Option<&'static str> {
    (origin == SecretOrigin::Keyring).then_some("stored in keyring; type to replace")
}
