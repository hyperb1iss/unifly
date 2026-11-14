use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use unifly_api::Event;
use unifly_api::model::EventSeverity;

use crate::tui::screens::events::render::clip;
use crate::tui::theme;

use super::super::DashboardScreen;
use super::super::focus::Panel;
use super::muted_line;

/// One event as a row: time, a dot in its severity, and its message cut
/// to `width` columns.
fn event_spans(event: &Event, width: usize) -> Vec<Span<'static>> {
    let time = event
        .timestamp
        .with_timezone(&chrono::Local)
        .format("%H:%M")
        .to_string();
    let (dot, text) = match event.severity {
        EventSeverity::Error | EventSeverity::Critical => (theme::error(), theme::error()),
        EventSeverity::Warning => (theme::warning(), theme::warning()),
        EventSeverity::Info => (theme::success(), theme::accent_secondary()),
        _ => (theme::success(), theme::text_secondary()),
    };
    vec![
        Span::styled(time, Style::default().fg(theme::warning())),
        Span::styled(" ● ", Style::default().fg(dot)),
        Span::styled(clip(&event.message, width), Style::default().fg(text)),
    ]
}

impl DashboardScreen {
    /// What the events panel says with nothing to list.
    fn no_events(&self) -> &'static str {
        if self.access.is_some_and(|access| !access.session) {
            "Events need a controller session"
        } else {
            self.empty_text("No events yet")
        }
    }

    /// Recent Events: one event per row, newest first.
    pub(super) fn render_recent_events(
        &self,
        frame: &mut ratatui::Frame,
        area: ratatui::layout::Rect,
    ) {
        // The panel holds only the newest few, so its footer points at
        // the log rather than counting them as if they were all of it.
        let footer = Line::from(Span::styled(
            " Enter for the full log ",
            Style::default().fg(theme::border_unfocused()),
        ));
        let block = self
            .panel_block(Panel::Events, "Recent Events")
            .title_bottom(footer);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let width = usize::from(inner.width.saturating_sub(10));
        let mut lines: Vec<Line> = self
            .events
            .events()
            .iter()
            .rev()
            .take(usize::from(inner.height))
            .map(|event| {
                let mut spans = vec![Span::raw(" ")];
                spans.extend(event_spans(event, width));
                Line::from(spans)
            })
            .collect();
        if lines.is_empty() {
            lines.push(muted_line(self.no_events()));
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }

    /// The events panel on a short terminal: the newest event on one
    /// line, with no frame around it.
    pub(super) fn render_latest_event(
        &self,
        frame: &mut ratatui::Frame,
        area: ratatui::layout::Rect,
    ) {
        let label = self.panel_title(Panel::Events, "Latest");
        let width = usize::from(area.width).saturating_sub(label.width() + 9);
        let mut spans = vec![label];
        match self.events.events().back() {
            Some(event) => spans.extend(event_spans(event, width)),
            None => spans.push(Span::styled(
                self.no_events(),
                Style::default().fg(theme::border_unfocused()),
            )),
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}
