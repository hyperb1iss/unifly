use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use crate::tui::theme;

pub(crate) fn render_input_field(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    active: bool,
    masked: bool,
) {
    render_input_field_with_hint(frame, area, label, value, active, masked, None);
}

/// [`render_input_field`] that shows `hint`, dimmed, while `value` is empty.
pub(crate) fn render_input_field_with_hint(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    active: bool,
    masked: bool,
    hint: Option<&str>,
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

    let cursor = if active { "\u{2588}" } else { "" };
    let line = match hint {
        Some(hint) if value.is_empty() => Line::from(vec![
            Span::styled(cursor, Style::default().fg(theme::accent_secondary())),
            Span::styled(hint, Style::default().fg(theme::text_muted())),
        ]),
        _ => Line::from(Span::styled(
            format!("{display}{cursor}"),
            Style::default().fg(theme::accent_secondary()),
        )),
    };
    frame.render_widget(Paragraph::new(line), inner);
}
