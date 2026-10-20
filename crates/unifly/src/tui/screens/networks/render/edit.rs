use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use crate::tui::screens::networks::state::NetworkEditState;
use crate::tui::theme;
use crate::tui::widgets::click::{self, Click, Hint, HitMap};

pub(super) fn render_edit_overlay(
    frame: &mut Frame,
    area: Rect,
    edit: &NetworkEditState,
    hits: &mut HitMap<Click>,
) {
    // The form holds the input while it is open, so it swallows taps on
    // the rows behind it.
    hits.push(area, Click::Absorb);

    let overlay_w = 44u16.min(area.width.saturating_sub(4));
    #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
    let overlay_h = (NetworkEditState::FIELD_COUNT as u16 + 6).min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(overlay_w)) / 2;
    let y = area.y + (area.height.saturating_sub(overlay_h)) / 2;
    let overlay_area = Rect::new(x, y, overlay_w, overlay_h);

    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(" Edit Network ")
        .title_style(
            Style::default()
                .fg(theme::warning())
                .add_modifier(Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(theme::accent_primary()));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let label = Style::default().fg(theme::text_secondary());
    let value_style = Style::default().fg(theme::accent_secondary());
    let focused_label = Style::default()
        .fg(theme::warning())
        .add_modifier(Modifier::BOLD);
    let enabled_style = Style::default().fg(theme::success());
    let disabled_style = Style::default().fg(theme::border_unfocused());

    let mut lines = Vec::new();

    for index in 0..NetworkEditState::FIELD_COUNT {
        let is_focused = index == edit.field_idx;
        let label_style = if is_focused { focused_label } else { label };
        let marker = if is_focused { "▸ " } else { "  " };
        let field_label = NetworkEditState::field_label(index);
        let field_value = edit.field_value(index);

        let field_style = if NetworkEditState::is_bool_field(index) {
            if matches!(field_value.as_str(), "Enabled") {
                enabled_style
            } else {
                disabled_style
            }
        } else {
            value_style
        };

        let cursor = if is_focused && !NetworkEditState::is_bool_field(index) {
            "▎"
        } else {
            ""
        };

        lines.push(Line::from(vec![
            Span::styled(marker, label_style),
            Span::styled(format!("{field_label:<14}"), label_style),
            Span::styled(field_value, field_style),
            Span::styled(cursor, Style::default().fg(theme::warning())),
        ]));
    }

    lines.push(Line::from(""));
    let bar = click::key_hints(&[
        Hint::button("Tab", "next", KeyCode::Tab),
        Hint::button("Space", "toggle", click::char_key(' ')),
        Hint::button("Enter", "save", KeyCode::Enter),
        Hint::button("Esc", "cancel", KeyCode::Esc),
    ]);
    let hint_row = click::paragraph_row(inner, lines.len());
    lines.push(bar.draw(hint_row, hits));

    frame.render_widget(Paragraph::new(lines), inner);
}
