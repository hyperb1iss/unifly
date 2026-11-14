//! The detail pane under the list: everything the selected event carries.

use chrono::{Local, Utc};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use unifly_api::MacAddress;

use super::EventsScreen;
use super::render::{category_label, severity_color, severity_label};
use crate::tui::theme;
use crate::tui::widgets::wrap;

/// Columns the field labels take.
const LABEL_COLUMNS: usize = 10;
/// Most lines of message the pane shows before it cuts with an ellipsis.
const MESSAGE_LINES: usize = 6;

/// How long ago `seconds` was, in the largest unit that fits.
fn age(seconds: i64) -> String {
    match seconds {
        ..60 => "just now".into(),
        60..3_600 => format!("{} min ago", seconds / 60),
        3_600..86_400 => format!("{} h ago", seconds / 3_600),
        _ => format!("{} d ago", seconds / 86_400),
    }
}

impl EventsScreen {
    /// The detail pane's lines for the selected event, wrapped to `width`
    /// columns of pane.
    pub(super) fn detail_lines(&self, width: u16) -> Option<Vec<Line<'static>>> {
        let event = self.selected()?;
        let value_columns = usize::from(width).saturating_sub(LABEL_COLUMNS + 4).max(8);
        let label = |name: &str| {
            Span::styled(
                format!(" {name:<LABEL_COLUMNS$}"),
                Style::default().fg(theme::text_secondary()),
            )
        };
        let plain = Style::default().fg(theme::text_primary());
        let muted = Style::default().fg(theme::text_muted());

        let local = event.timestamp.with_timezone(&Local);
        let elapsed = Utc::now().signed_duration_since(event.timestamp);
        let mut lines = vec![
            Line::from(vec![
                label("Time"),
                Span::styled(local.format("%Y-%m-%d %H:%M:%S").to_string(), plain),
                Span::styled(format!("  {}", age(elapsed.num_seconds())), muted),
            ]),
            Line::from(vec![
                label("Category"),
                Span::styled(format!("{:<10}", category_label(event.category)), plain),
                Span::styled("Severity  ", Style::default().fg(theme::text_secondary())),
                Span::styled(
                    severity_label(event.severity),
                    Style::default().fg(severity_color(event.severity)),
                ),
            ]),
            Line::from(vec![
                label("Type"),
                Span::styled(super::render::clip(&event.event_type, value_columns), plain),
            ]),
        ];

        if let Some(mac) = &event.device_mac {
            let name = self
                .devices
                .iter()
                .find(|device| &device.mac == mac)
                .map(|device| device.name.clone().unwrap_or_else(|| mac.to_string()));
            lines.push(entity_line(
                label("Device"),
                mac,
                name,
                "not on this controller",
            ));
        }
        if let Some(mac) = &event.client_mac {
            let name = self
                .clients
                .iter()
                .find(|client| &client.mac == mac)
                .map(|client| {
                    client
                        .name
                        .clone()
                        .or_else(|| client.hostname.clone())
                        .unwrap_or_else(|| mac.to_string())
                });
            lines.push(entity_line(label("Client"), mac, name, "not connected"));
        }

        let message = wrap::wrap_clamped(&event.message, value_columns, MESSAGE_LINES);
        for (index, text) in message.into_iter().enumerate() {
            lines.push(Line::from(vec![
                label(if index == 0 { "Message" } else { "" }),
                Span::styled(text, Style::default().fg(severity_color(event.severity))),
            ]));
        }
        Some(lines)
    }

    pub(super) fn render_detail(frame: &mut Frame, area: Rect, lines: Vec<Line<'static>>) {
        let block = Block::default()
            .title(" Event ")
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_focused());
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

/// A device or client line: its name and MAC, or its MAC and why it
/// cannot be opened. The status bar offers the key that opens it.
fn entity_line(
    label: Span<'static>,
    mac: &MacAddress,
    name: Option<String>,
    missing: &'static str,
) -> Line<'static> {
    let plain = Style::default().fg(theme::text_primary());
    let muted = Style::default().fg(theme::text_muted());
    match name {
        Some(name) if name != mac.to_string() => Line::from(vec![
            label,
            Span::styled(name, Style::default().fg(theme::accent_secondary())),
            Span::styled(format!("  {mac}"), muted),
        ]),
        Some(_) => Line::from(vec![label, Span::styled(mac.to_string(), plain)]),
        None => Line::from(vec![
            label,
            Span::styled(mac.to_string(), plain),
            Span::styled(format!("  ({missing})"), muted),
        ]),
    }
}
