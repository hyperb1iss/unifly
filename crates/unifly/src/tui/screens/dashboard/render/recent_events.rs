use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use unifly_api::model::EventSeverity;

use crate::tui::theme;

use super::super::DashboardScreen;

impl DashboardScreen {
    /// Compact Recent Events - single column, most recent first.
    #[allow(clippy::cast_possible_truncation)]
    pub(super) fn render_recent_events(
        &self,
        frame: &mut ratatui::Frame,
        area: ratatui::layout::Rect,
    ) {
        let event_count = self.events.len();
        let title = Line::from(vec![Span::styled(" Recent Events ", theme::title_style())]);
        let footer = if event_count > 0 {
            Line::from(vec![Span::styled(
                format!(" ↓ {event_count} event log "),
                Style::default().fg(theme::border_unfocused()),
            )])
        } else {
            Line::from("")
        };

        let block = Block::default()
            .title(title)
            .title_bottom(footer)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_default());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let max_rows = usize::from(inner.height);
        let msg_width = usize::from(inner.width.saturating_sub(10));

        let mut lines = Vec::new();
        if self.events.is_empty() {
            lines.push(Line::from(Span::styled(
                " No events",
                Style::default().fg(theme::border_unfocused()),
            )));
        } else {
            for evt in self.events.iter().rev().take(max_rows) {
                let time_str = evt
                    .timestamp
                    .with_timezone(&chrono::Local)
                    .format("%H:%M")
                    .to_string();
                let severity_color = match evt.severity {
                    EventSeverity::Error | EventSeverity::Critical => theme::error(),
                    EventSeverity::Warning => theme::warning(),
                    EventSeverity::Info => theme::accent_secondary(),
                    _ => theme::text_secondary(),
                };
                let dot_color = match evt.severity {
                    EventSeverity::Error | EventSeverity::Critical => theme::error(),
                    EventSeverity::Warning => theme::warning(),
                    _ => theme::success(),
                };
                let msg: String = evt.message.chars().take(msg_width).collect();
                lines.push(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(time_str, Style::default().fg(theme::warning())),
                    Span::styled(" ● ", Style::default().fg(dot_color)),
                    Span::styled(msg, Style::default().fg(severity_color)),
                ]));
            }
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }
}
