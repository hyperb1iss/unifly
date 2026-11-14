use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use crate::tui::screens::networks::edit::{Field, NetworkForm, Target};
use crate::tui::theme;
use crate::tui::widgets::click::{self, Click, Hint, HitMap};
use crate::tui::widgets::wrap::wrap_clamped;

/// Columns the form takes when the screen has them.
const FORM_WIDTH: u16 = 62;
/// Columns for a field's label, the DHCP settings' indent included.
const LABEL_WIDTH: usize = 17;
/// Columns left of the label: the focus arrow and the changed mark.
const MARKER_WIDTH: usize = 2;
/// Rows a failure the controller reported may wrap to.
const ERROR_ROWS: usize = 3;

/// One row of the form's body.
enum FormRow {
    /// A field, by its position in the form.
    Field(usize),
    /// The reason the field above cannot be saved.
    Error(String),
    Gap,
}

/// The form's body rows: its fields in order, a gap before each group
/// when `roomy`, and the validation error under the field it is about.
fn body_rows(form: &NetworkForm, text_width: usize, roomy: bool) -> Vec<FormRow> {
    let mut rows = Vec::new();
    for (index, field) in form.fields().iter().enumerate() {
        if roomy && field.starts_group() {
            rows.push(FormRow::Gap);
        }
        rows.push(FormRow::Field(index));
        if let Some(error) = form.error.as_ref().filter(|e| e.field == Some(*field)) {
            rows.extend(
                wrap_clamped(&error.message, text_width, ERROR_ROWS)
                    .into_iter()
                    .map(FormRow::Error),
            );
        }
    }
    rows
}

/// The lines under the fields: that a save is running, or why the
/// controller refused one.
fn status_lines(form: &NetworkForm, text_width: usize) -> Vec<Line<'static>> {
    if form.saving {
        return vec![Line::from(Span::styled(
            "  Saving\u{2026}",
            Style::default()
                .fg(theme::accent_primary())
                .add_modifier(Modifier::BOLD),
        ))];
    }
    let Some(error) = form.error.as_ref().filter(|error| error.field.is_none()) else {
        return Vec::new();
    };
    wrap_clamped(&error.message, text_width, ERROR_ROWS)
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let lead = if index == 0 { "  \u{2717} " } else { "    " };
            Line::from(Span::styled(
                format!("{lead}{line}"),
                Style::default().fg(theme::error()),
            ))
        })
        .collect()
}

fn field_line(form: &NetworkForm, field: Field, width: usize) -> Line<'static> {
    let focused = form.focused() == field && !form.saving;
    let failed = form
        .error
        .as_ref()
        .is_some_and(|error| error.field == Some(field));
    let label_style = if failed {
        Style::default()
            .fg(theme::error())
            .add_modifier(Modifier::BOLD)
    } else if focused {
        Style::default()
            .fg(theme::warning())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::text_secondary())
    };
    // Everything in a new network is new, so only an edit marks changes.
    let changed = form.target != Target::Create && form.changed(field);
    let marker = format!(
        "{}{}",
        if focused { "\u{25B8}" } else { " " },
        if changed { "\u{2022}" } else { " " }
    );
    let indent = if field.is_dhcp_setting() { "  " } else { "" };
    let label = format!("{indent}{}", field.label());
    let mut spans = vec![
        Span::styled(marker, label_style),
        Span::styled(format!("{label:<LABEL_WIDTH$}"), label_style),
    ];

    let muted = Style::default().fg(theme::text_muted());
    let Some(input) = form.input(field) else {
        let (glyph, style) = if form.flag(field) {
            ("\u{25CF} on", Style::default().fg(theme::success()))
        } else {
            ("\u{25CB} off", muted)
        };
        spans.push(Span::styled(glyph, style));
        if let Some(note) = form.note(field) {
            spans.push(Span::styled(format!("  {note}"), muted));
        }
        return Line::from(spans);
    };
    // A DHCP setting means nothing while the server is off.
    let value_style = if field.is_dhcp_setting() && !form.flag(Field::Dhcp) {
        muted
    } else {
        Style::default().fg(theme::accent_secondary())
    };
    let value_width = width.saturating_sub(MARKER_WIDTH + LABEL_WIDTH).max(1);
    if input.is_empty() && !focused {
        spans.push(Span::styled(field.placeholder(), muted));
    } else {
        spans.extend(input.spans(value_width, focused, None, value_style));
    }
    Line::from(spans)
}

fn title(form: &NetworkForm) -> String {
    let modified = if form.is_dirty() { " (modified)" } else { "" };
    match &form.target {
        Target::Edit { name, .. } => format!(" Edit network: {name}{modified} "),
        Target::Create => " New network ".to_owned(),
    }
}

/// The form's own buttons. It holds the keyboard, so the status bar has
/// none for it; moving between fields is in the help overlay.
fn hint_bar(form: &NetworkForm) -> click::ClickLine<'static, Click> {
    // Space only flips a yes/no field; on a text field it types a space,
    // so the toggle button shows only where it toggles.
    let mut buttons = Vec::new();
    if form.focused().is_toggle() {
        buttons.push(Hint::button("Space", "toggle", click::char_key(' ')));
    }
    let save = match form.target {
        Target::Edit { .. } => "save",
        Target::Create => "create",
    };
    buttons.extend([
        Hint::button("Enter", save, KeyCode::Enter),
        Hint::button("Esc", "cancel", KeyCode::Esc),
    ]);
    click::key_hints(&buttons)
}

pub(super) fn render_form(
    frame: &mut Frame,
    area: Rect,
    form: &NetworkForm,
    hits: &mut HitMap<Click>,
) {
    // The form holds the input while it is open, so it swallows taps on
    // the rows behind it.
    hits.push(area, Click::Absorb);

    let width = FORM_WIDTH.min(area.width.saturating_sub(4));
    let inner_width = usize::from(width.saturating_sub(2));
    let text_width = inner_width.saturating_sub(4).max(1);
    let status = status_lines(form, text_width);
    let max_height = area.height.saturating_sub(2);
    let height_with = |rows: &[FormRow], gap: usize| {
        u16::try_from(rows.len() + gap + status.len() + 3).unwrap_or(u16::MAX)
    };

    // Short terminals drop the gaps between groups before they scroll.
    let roomy = body_rows(form, text_width, true);
    let (rows, gap) = if height_with(&roomy, 1) <= max_height {
        (roomy, 1)
    } else {
        (body_rows(form, text_width, false), 0)
    };
    let height = height_with(&rows, gap).min(max_height);
    let overlay = Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    );

    frame.render_widget(Clear, overlay);
    let block = Block::default()
        .title(title(form))
        .title_style(theme::title_style())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border_focused())
        .style(Style::default().bg(theme::bg_base()));
    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    // The hint bar and the status above it always show; the fields scroll
    // in what is left, keeping the focused one and its error in view.
    let footer = status.len() + 1;
    let body_height = usize::from(inner.height).saturating_sub(footer + gap);
    let focus_row = rows
        .iter()
        .position(|row| matches!(row, FormRow::Field(index) if form.fields().get(*index) == Some(&form.focused())))
        .unwrap_or(0);
    let focus_end = rows
        .iter()
        .skip(focus_row + 1)
        .take_while(|row| matches!(row, FormRow::Error(_)))
        .count()
        + focus_row
        + 1;
    let first = focus_end.saturating_sub(body_height).min(focus_row);

    let mut lines = Vec::new();
    for row in rows.iter().skip(first).take(body_height) {
        let line = match row {
            FormRow::Gap => Line::from(""),
            FormRow::Error(text) => Line::from(Span::styled(
                format!("    {text}"),
                Style::default().fg(theme::error()),
            )),
            FormRow::Field(index) => {
                hits.push(click::paragraph_row(inner, lines.len()), Click::Row(*index));
                form.fields().get(*index).map_or_else(
                    || Line::from(""),
                    |field| field_line(form, *field, inner_width),
                )
            }
        };
        lines.push(line);
    }
    lines.resize_with(body_height + gap, || Line::from(""));
    lines.extend(status);
    if form.saving {
        lines.push(Line::from(""));
    } else {
        let hint_row = click::paragraph_row(inner, lines.len());
        lines.push(hint_bar(form).draw(hint_row, hits));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}
