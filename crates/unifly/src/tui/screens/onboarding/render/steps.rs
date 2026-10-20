use super::super::{AuthMode, CredentialField, OnboardingScreen};

use crate::tui::forms::widgets::render_input_field;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::tui::theme;

pub(super) fn render_welcome(_screen: &OnboardingScreen, frame: &mut Frame, area: Rect) {
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

    let description = vec![
        Line::from(Span::styled(
            "No configuration found. This wizard will help you",
            Style::default().fg(theme::text_secondary()),
        )),
        Line::from(Span::styled(
            "connect to your UniFi controller.",
            Style::default().fg(theme::text_secondary()),
        )),
    ];
    frame.render_widget(
        Paragraph::new(description).alignment(Alignment::Center),
        layout[1],
    );

    frame.render_widget(
        Paragraph::new(Span::styled(
            "Press Enter to begin",
            Style::default().fg(theme::accent_primary()),
        ))
        .alignment(Alignment::Center),
        layout[2],
    );
}

pub(super) fn render_url(screen: &OnboardingScreen, frame: &mut Frame, area: Rect) {
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

    render_input_field(
        frame,
        layout[1],
        "  Controller URL",
        &screen.draft.url,
        true,
        false,
    );
}

pub(super) fn render_auth_mode(screen: &OnboardingScreen, frame: &mut Frame, area: Rect) {
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
    for (idx, mode) in AuthMode::ALL.iter().enumerate() {
        let selected = idx == screen.auth_mode_index;
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

        let label = mode.label();
        lines.push(Line::from(vec![
            Span::styled(marker, marker_style),
            Span::styled(label, label_style),
        ]));
        lines.push(Line::from(Span::styled(
            format!("    {}", mode.description()),
            Style::default().fg(theme::border_unfocused()),
        )));
        lines.push(Line::from(""));
    }

    frame.render_widget(Paragraph::new(lines), list_area);
}

pub(super) fn render_credentials(screen: &OnboardingScreen, frame: &mut Frame, area: Rect) {
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

    if matches!(
        screen.draft.auth_mode,
        AuthMode::ApiKey | AuthMode::Hybrid | AuthMode::Cloud
    ) {
        let input_area = Rect::new(
            fields_area.x,
            fields_area.y + y_offset,
            fields_area.width,
            4,
        );
        render_input_field(
            frame,
            input_area,
            "  API Key",
            &screen.draft.api_key,
            screen.cred_field == CredentialField::ApiKey,
            true,
        );
        y_offset += 5;
    }

    if matches!(screen.draft.auth_mode, AuthMode::Cloud) {
        let input_area = Rect::new(
            fields_area.x,
            fields_area.y + y_offset,
            fields_area.width,
            4,
        );
        render_input_field(
            frame,
            input_area,
            "  Host ID",
            &screen.draft.host_id,
            screen.cred_field == CredentialField::HostId,
            false,
        );
        y_offset += 5;
    }

    if matches!(screen.draft.auth_mode, AuthMode::Session | AuthMode::Hybrid) {
        let username_area = Rect::new(
            fields_area.x,
            fields_area.y + y_offset,
            fields_area.width,
            4,
        );
        render_input_field(
            frame,
            username_area,
            "  Username",
            &screen.draft.username,
            screen.cred_field == CredentialField::Username,
            false,
        );
        y_offset += 5;

        let password_area = Rect::new(
            fields_area.x,
            fields_area.y + y_offset,
            fields_area.width,
            4,
        );
        render_input_field(
            frame,
            password_area,
            "  Password",
            &screen.draft.password,
            screen.cred_field == CredentialField::Password,
            !screen.show_password,
        );
    }
}

pub(super) fn render_site(screen: &OnboardingScreen, frame: &mut Frame, area: Rect) {
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

    render_input_field(frame, layout[1], "  Site", &screen.draft.site, true, false);
}

pub(super) fn render_testing(screen: &OnboardingScreen, frame: &mut Frame, area: Rect) {
    let layout = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(2),
        Constraint::Min(0),
    ])
    .split(area);

    if screen.testing {
        let throbber = throbber_widgets_tui::Throbber::default()
            .label("  Testing connection...")
            .style(Style::default().fg(theme::accent_secondary()))
            .throbber_style(Style::default().fg(theme::accent_primary()));

        frame.render_stateful_widget(throbber, layout[0], &mut screen.throbber_state.clone());
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("  Connecting to {}", screen.draft.url.trim()),
                Style::default().fg(theme::border_unfocused()),
            )),
            layout[1],
        );
    } else if let Some(ref error) = screen.test_error {
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
                    format!("  {error}"),
                    Style::default().fg(theme::error()),
                )),
            ])
            .wrap(Wrap { trim: false }),
            area,
        );
    }
}

pub(super) fn render_done(screen: &OnboardingScreen, frame: &mut Frame, area: Rect) {
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
            format!("  Profile: {}", screen.profile_name),
            Style::default().fg(theme::text_secondary()),
        )),
        Line::from(Span::styled(
            format!("  Controller: {}", screen.draft.url.trim()),
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
