//! How the setup wizard and the Settings screen draw their fields: a
//! one-row form field, a roomier boxed one, toggles, pickers, and the
//! wrapped message area under a form.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use super::probe::Tone;
use crate::tui::theme;
use crate::tui::widgets::line_input::LineInput;
use crate::tui::widgets::wrap::wrap;

/// The dot a hidden character is drawn as.
pub(crate) const MASK: char = '\u{25CF}';

/// Columns a row gives its focus marker and label before the value.
pub(crate) const LABEL_COLUMNS: u16 = 20;

/// A text field as a form draws it.
#[derive(Clone, Copy)]
pub(crate) struct FieldView<'a> {
    pub(crate) label: &'a str,
    pub(crate) input: &'a LineInput,
    pub(crate) active: bool,
    /// Draw every character as a dot.
    pub(crate) masked: bool,
    /// Shown dimmed while the field is empty.
    pub(crate) placeholder: Option<&'a str>,
    /// A short note at the row's right edge, such as the launch flag
    /// overriding the field.
    pub(crate) tag: Option<&'a str>,
}

fn label_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(theme::accent_secondary())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::text_secondary())
    }
}

fn value_style(active: bool) -> Style {
    if active {
        Style::default().fg(theme::accent_secondary())
    } else {
        Style::default().fg(theme::text_primary())
    }
}

fn muted() -> Style {
    Style::default().fg(theme::text_muted())
}

/// The marker and label that open a row: `▸ Label` padded to the value
/// column.
fn row_label(label: &str, active: bool) -> Vec<Span<'static>> {
    let marker = if active { "\u{25B8} " } else { "  " };
    let width = usize::from(LABEL_COLUMNS).saturating_sub(2);
    vec![
        Span::styled(marker, Style::default().fg(theme::accent_primary())),
        Span::styled(format!("{label:<width$}"), label_style(active)),
    ]
}

/// The value area of a row drawn in `area`.
pub(crate) fn value_area(area: Rect) -> Rect {
    let offset = LABEL_COLUMNS.min(area.width);
    Rect {
        x: area.x + offset,
        width: area.width - offset,
        ..area
    }
}

/// The spans for a field's value in `width` columns: the text around the
/// cursor, or the placeholder while it is empty.
fn value_spans(view: &FieldView<'_>, width: usize) -> Vec<Span<'static>> {
    let style = value_style(view.active);
    if view.input.is_empty()
        && let Some(placeholder) = view.placeholder
    {
        let room = width.saturating_sub(usize::from(view.active));
        let shown: String = wrap(placeholder, room.max(1))
            .into_iter()
            .next()
            .unwrap_or_default();
        let mut spans = Vec::new();
        if view.active {
            spans.push(Span::styled("\u{2588}", style));
        }
        spans.push(Span::styled(shown, muted()));
        return spans;
    }
    view.input
        .spans(width, view.active, view.masked.then_some(MASK), style)
}

/// Draw a text field on one row: label, value, and its tag when the row
/// has room for it next to the value.
pub(crate) fn render_input_row(frame: &mut Frame, area: Rect, view: &FieldView<'_>) {
    if area.is_empty() {
        return;
    }
    let value = value_area(area);
    let tag = view
        .tag
        .map(|tag| format!(" {tag}"))
        .filter(|tag| Span::raw(tag.as_str()).width() + 16 <= usize::from(value.width));
    let tag_width = tag.as_deref().map_or(0, |tag| Span::raw(tag).width());
    let value_width = usize::from(value.width).saturating_sub(tag_width);

    let mut spans = row_label(view.label, view.active);
    let drawn = value_spans(view, value_width);
    let used: usize = drawn.iter().map(Span::width).sum();
    spans.extend(drawn);
    if let Some(tag) = tag {
        spans.push(Span::raw(" ".repeat(value_width.saturating_sub(used))));
        spans.push(Span::styled(tag, muted()));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Rows a boxed field takes: its label, then a three-row box.
pub(crate) const BOX_ROWS: u16 = 4;

/// Draw a text field as a label over a bordered box, for a form with room
/// to spare. Falls back to one row when `area` is shorter than the box.
pub(crate) fn render_input_box(frame: &mut Frame, area: Rect, view: &FieldView<'_>) {
    if area.height < BOX_ROWS {
        render_input_row(frame, Rect { height: 1, ..area }.intersection(area), view);
        return;
    }

    let mut label = vec![Span::styled(
        format!("  {}", view.label),
        label_style(view.active),
    )];
    if let Some(tag) = view.tag {
        label.push(Span::styled(format!("  {tag}"), muted()));
    }
    frame.render_widget(
        Paragraph::new(Line::from(label)),
        Rect { height: 1, ..area },
    );

    let border = if view.active {
        theme::accent_primary()
    } else {
        theme::border_unfocused()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border));
    let block_area = Rect {
        y: area.y + 1,
        height: 3,
        ..area
    };
    let inner = block.inner(block_area);
    frame.render_widget(block, block_area);
    frame.render_widget(
        Paragraph::new(Line::from(value_spans(view, usize::from(inner.width)))),
        inner,
    );
}

/// Draw a checkbox row: label, box, and what the box means.
pub(crate) fn render_toggle_row(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: bool,
    active: bool,
    note: &str,
) {
    if area.is_empty() {
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
    let mut spans = row_label(label, active);
    spans.push(Span::styled(marker, marker_style));
    spans.push(Span::styled(format!(" {note}"), muted()));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Where a picker row drew its two arrows, for clicks.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ChoiceArrows {
    pub(crate) previous: Rect,
    pub(crate) next: Rect,
}

/// Draw a picker row, `Label  ◂ value ▸ note`, and report where its
/// arrows landed.
pub(crate) fn render_choice_row(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    active: bool,
    note: &str,
) -> ChoiceArrows {
    if area.is_empty() {
        return ChoiceArrows::default();
    }
    let arrow_style = if active {
        Style::default().fg(theme::accent_primary())
    } else {
        Style::default().fg(theme::border_unfocused())
    };
    let values = value_area(area);
    let room = usize::from(values.width).saturating_sub(4);
    let shown = fit(value, room);
    let shown_width = u16::try_from(Span::raw(shown.as_str()).width()).unwrap_or(u16::MAX);

    let mut spans = row_label(label, active);
    spans.push(Span::styled("\u{25C2} ", arrow_style));
    spans.push(Span::styled(shown, value_style(active)));
    spans.push(Span::styled(" \u{25B8}", arrow_style));
    spans.push(Span::styled(format!(" {note}"), muted()));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);

    let arrow = |x: u16| {
        Rect {
            x,
            width: 2,
            height: 1,
            ..values
        }
        .intersection(values)
    };
    ChoiceArrows {
        previous: arrow(values.x),
        next: arrow(values.x.saturating_add(2).saturating_add(shown_width)),
    }
}

/// Draw a row that shows a value without editing it in place: a label,
/// the value, and a dimmed note after it.
pub(crate) fn render_value_row(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    active: bool,
    note: &str,
) {
    if area.is_empty() {
        return;
    }
    let room = usize::from(value_area(area).width);
    let mut spans = row_label(label, active);
    spans.push(Span::styled(fit(value, room), value_style(active)));
    spans.push(Span::styled(format!(" {note}"), muted()));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Draw a read-only row: a label and what it says, dimmed.
pub(crate) fn render_info_row(frame: &mut Frame, area: Rect, label: &str, value: &str) {
    if area.is_empty() {
        return;
    }
    let mut spans = row_label(label, false);
    spans.push(Span::styled(
        fit(value, usize::from(value_area(area).width)),
        muted(),
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Draw the spinner a form shows while it waits: a label, and under it
/// what it is waiting on.
pub(crate) fn render_waiting(
    frame: &mut Frame,
    area: Rect,
    throbber: &throbber_widgets_tui::ThrobberState,
    label: &str,
    detail: &str,
) {
    if area.is_empty() {
        return;
    }
    let top = area.y + area.height.saturating_sub(2) / 2;
    let spinner = throbber_widgets_tui::Throbber::default()
        .label(format!(" {label}"))
        .style(Style::default().fg(theme::accent_secondary()))
        .throbber_style(Style::default().fg(theme::accent_primary()));
    let row = |y: u16| {
        Rect {
            x: area.x + 2,
            y,
            width: area.width.saturating_sub(4),
            height: 1,
        }
        .intersection(area)
    };
    frame.render_stateful_widget(spinner, row(top), &mut throbber.clone());
    frame.render_widget(
        Paragraph::new(Span::styled(
            fit(detail, usize::from(area.width.saturating_sub(6))),
            muted(),
        )),
        row(top + 1),
    );
}

/// Draw a section rule: ` Label ─────`.
pub(crate) fn render_section(frame: &mut Frame, area: Rect, label: &str) {
    if area.is_empty() {
        return;
    }
    let title = format!(" {label} ");
    let rule = usize::from(area.width).saturating_sub(Span::raw(title.as_str()).width() + 1);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                title,
                Style::default()
                    .fg(theme::accent_secondary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "\u{2500}".repeat(rule),
                Style::default().fg(theme::border_unfocused()),
            ),
        ])),
        area,
    );
}

/// `text` cut to `width` columns, with an ellipsis when it was cut.
pub(crate) fn fit(text: &str, width: usize) -> String {
    if Span::raw(text).width() <= width {
        return text.to_owned();
    }
    let mut out = String::new();
    for ch in text.chars() {
        let mut next = out.clone();
        next.push(ch);
        if Span::raw(next.as_str()).width() + 1 > width {
            break;
        }
        out = next;
    }
    out.push('\u{2026}');
    out
}

fn tone_style(tone: Tone) -> Style {
    match tone {
        Tone::Error => Style::default().fg(theme::error()),
        Tone::Advice => Style::default().fg(theme::text_primary()),
        Tone::Detail => muted(),
        Tone::Notice => Style::default().fg(theme::warning()),
    }
}

/// Paragraphs wrapped to `width` columns and cut to `max_lines`, the last
/// kept line ending in an ellipsis when something was cut.
pub(crate) fn message_lines(
    paragraphs: &[(Tone, &str)],
    width: usize,
    max_lines: usize,
) -> Vec<Line<'static>> {
    let mut lines: Vec<(Tone, String)> = paragraphs
        .iter()
        .flat_map(|(tone, text)| wrap(text, width).into_iter().map(|line| (*tone, line)))
        .collect();
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some((_, last)) = lines.last_mut() {
            *last = fit(&format!("{last}\u{2026}"), width);
        }
    }
    lines
        .into_iter()
        .map(|(tone, line)| Line::from(Span::styled(line, tone_style(tone))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::table::render_lines;

    fn view<'a>(input: &'a LineInput, label: &'a str) -> FieldView<'a> {
        FieldView {
            label,
            input,
            active: true,
            masked: false,
            placeholder: None,
            tag: None,
        }
    }

    /// A value longer than its field keeps the end being typed and the
    /// cursor in view, in both layouts.
    #[test]
    fn a_long_value_scrolls_to_its_cursor() {
        let host = "70A7419783ED0000000006797F060000000006C722C700000000624D3F9C:1234567";
        let input = LineInput::new(host);

        let row = render_lines(60, 1, |frame, area| {
            render_input_row(frame, area, &view(&input, "Host ID"));
        });
        assert!(row[0].ends_with("1234567\u{2588}"), "{row:?}");
        assert!(row[0].contains("\u{25B8} Host ID"), "{row:?}");

        let boxed = render_lines(40, 4, |frame, area| {
            render_input_box(frame, area, &view(&input, "Host ID"));
        });
        assert!(boxed[2].contains("1234567\u{2588}"), "{boxed:?}");
    }

    /// One dot per character: a mask built from the byte length shows a
    /// password with accents as longer than it is.
    #[test]
    fn a_masked_value_shows_one_dot_per_character() {
        let input = LineInput::new("pässwörd");
        let mut field = view(&input, "Password");
        field.masked = true;
        field.active = false;

        let row = render_lines(60, 1, |frame, area| render_input_row(frame, area, &field));

        assert_eq!(row[0].matches(MASK).count(), 8, "{row:?}");
        assert!(!row[0].contains('p'), "{row:?}");
    }

    #[test]
    fn an_empty_field_shows_its_placeholder_and_its_tag() {
        let input = LineInput::default();
        let mut field = view(&input, "API key");
        field.placeholder = Some("stored in keyring; type to replace");
        field.tag = Some("--api-key");

        let row = render_lines(76, 1, |frame, area| render_input_row(frame, area, &field));
        assert!(row[0].contains("\u{2588}stored in keyring"), "{row:?}");
        assert!(row[0].trim_end().ends_with("--api-key"), "{row:?}");

        // Too narrow for both: the value keeps the room.
        let row = render_lines(40, 1, |frame, area| render_input_row(frame, area, &field));
        assert!(!row[0].contains("--api-key"), "{row:?}");
    }

    #[test]
    fn a_picker_reports_where_its_arrows_are() {
        let mut arrows = ChoiceArrows::default();
        let row = render_lines(60, 1, |frame, area| {
            arrows = render_choice_row(frame, area, "Profile", "home", true, "1 of 2");
        });

        let chars: Vec<char> = row[0].chars().collect();
        let column = |index: Option<usize>| u16::try_from(index.expect("arrow")).expect("column");
        let previous = column(chars.iter().position(|ch| *ch == '\u{25C2}'));
        let next = column(chars.iter().rposition(|ch| *ch == '\u{25B8}'));
        assert_eq!(arrows.previous.x, previous);
        assert_eq!(arrows.previous.width, 2);
        assert_eq!(arrows.next.x + 1, next, "the arrow follows a space");
        assert!(row[0].contains("\u{25C2} home \u{25B8} 1 of 2"), "{row:?}");
    }

    #[test]
    fn a_message_wraps_and_marks_what_it_cut() {
        let paragraphs = [
            (Tone::Error, "one two three four five six seven"),
            (Tone::Advice, "eight nine ten"),
        ];

        let all = message_lines(&paragraphs, 12, 10);
        let text: Vec<String> = all.iter().map(ToString::to_string).collect();
        assert_eq!(
            text,
            [
                "one two",
                "three four",
                "five six",
                "seven",
                "eight nine",
                "ten"
            ]
        );

        let cut = message_lines(&paragraphs, 12, 2);
        assert_eq!(cut.len(), 2);
        assert!(cut[1].to_string().ends_with('\u{2026}'));
    }
}
