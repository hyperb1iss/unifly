//! Detail panels under a list: a grid of labelled values sized to its
//! content, so a panel takes the rows it needs and the list keeps the rest.
//!
//! Short fields flow side by side in as many columns as the width holds,
//! with every label and value column aligned. A wide field takes a whole
//! row and wraps.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use super::wrap::wrap;
use crate::tui::theme;

/// The glyph every table and panel shows for a value that is not set.
pub const NONE: &str = "\u{2500}";

/// Cells between a label and its value.
const LABEL_GAP: usize = 2;
/// Cells between two columns of fields.
const COLUMN_GAP: usize = 3;
/// Most columns short fields flow into.
const MAX_COLUMNS: usize = 3;

#[derive(Debug, Clone)]
struct Field {
    label: String,
    value: String,
    style: Style,
    wide: bool,
}

/// Labelled values for one detail panel.
#[derive(Debug, Clone, Default)]
pub struct DetailGrid {
    fields: Vec<Field>,
}

fn width_of(text: &str) -> usize {
    Span::raw(text).width()
}

impl DetailGrid {
    /// A short value, laid out beside its neighbours.
    pub fn field(&mut self, label: &str, value: impl Into<String>, style: Style) -> &mut Self {
        self.push(label, value.into(), style, false)
    }

    /// A value that takes a whole row and wraps.
    pub fn wide(&mut self, label: &str, value: impl Into<String>, style: Style) -> &mut Self {
        self.push(label, value.into(), style, true)
    }

    fn push(&mut self, label: &str, value: String, style: Style, wide: bool) -> &mut Self {
        self.fields.push(Field {
            label: label.to_owned(),
            value,
            style,
            wide,
        });
        self
    }

    fn label_width(&self) -> usize {
        self.fields
            .iter()
            .map(|field| width_of(&field.label))
            .max()
            .unwrap_or(0)
            + LABEL_GAP
    }

    /// Runs of short fields, split where a wide field interrupts them.
    fn runs(&self) -> Vec<&[Field]> {
        self.fields
            .chunk_by(|a, b| !a.wide && !b.wide)
            .collect::<Vec<_>>()
    }

    /// The value width of each column when short fields flow `columns`
    /// wide, or `None` when that does not fit `width`.
    fn column_widths(&self, columns: usize, width: usize) -> Option<Vec<usize>> {
        let mut widths = vec![0; columns];
        for run in self.runs() {
            if run.first().is_some_and(|field| field.wide) {
                continue;
            }
            for (index, field) in run.iter().enumerate() {
                let column = index % columns;
                widths[column] = widths[column].max(width_of(&field.value));
            }
        }
        let total = 1
            + columns * self.label_width()
            + widths.iter().sum::<usize>()
            + COLUMN_GAP * (columns - 1);
        (total <= width || columns == 1).then_some(widths)
    }

    /// The panel's lines for a `width`-cell interior.
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let width = usize::from(width);
        let label_width = self.label_width();
        let (columns, widths) = (1..=MAX_COLUMNS)
            .rev()
            .find_map(|columns| Some((columns, self.column_widths(columns, width)?)))
            .unwrap_or((1, vec![0]));
        let label_style = Style::default().fg(theme::text_secondary());
        let label = |text: &str| Span::styled(format!("{text:<label_width$}"), label_style);
        let value_width = width.saturating_sub(1 + label_width).max(1);

        let mut lines = Vec::new();
        for run in self.runs() {
            // A lone column has nothing beside it to align with, so its
            // values wrap like wide ones instead of being cut.
            if columns == 1 || run.first().is_some_and(|field| field.wide) {
                for field in run {
                    for (index, text) in wrap(&field.value, value_width).into_iter().enumerate() {
                        let head = if index == 0 { field.label.as_str() } else { "" };
                        lines.push(Line::from(vec![
                            Span::raw(" "),
                            label(head),
                            Span::styled(text, field.style),
                        ]));
                    }
                }
                continue;
            }
            for row in run.chunks(columns) {
                let mut spans = vec![Span::raw(" ")];
                for (column, field) in row.iter().enumerate() {
                    if column > 0 {
                        spans.push(Span::raw(" ".repeat(COLUMN_GAP)));
                    }
                    let cell = widths[column];
                    spans.push(label(&field.label));
                    spans.push(Span::styled(format!("{:<cell$}", field.value), field.style));
                }
                lines.push(Line::from(spans));
            }
        }
        lines
    }
}

/// Split `area` into a list above and a panel below that holds `lines`
/// rows of detail, leaving the list at least `min_list` rows. The panel
/// is empty when there is no room for it.
pub fn split_detail(area: Rect, lines: usize, min_list: u16) -> (Rect, Rect) {
    let wanted = u16::try_from(lines).unwrap_or(u16::MAX).saturating_add(2);
    let height = wanted.min(area.height.saturating_sub(min_list));
    // A border with nothing inside it says nothing.
    let height = if height < 3 { 0 } else { height };
    let list = Rect {
        height: area.height - height,
        ..area
    };
    let panel = Rect {
        y: area.y + list.height,
        height,
        ..area
    };
    (list, panel)
}

/// Draw a titled panel holding `lines`. When they do not all fit, the last
/// row says how many are hidden instead of cutting silently.
pub fn render_panel(frame: &mut Frame, area: Rect, title: &str, mut lines: Vec<Line<'static>>) {
    if area.is_empty() {
        return;
    }
    let block = Block::default()
        .title(format!(" {title} "))
        .title_style(theme::title_style())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border_focused());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = usize::from(inner.height);
    if lines.len() > rows && rows > 0 {
        let hidden = lines.len() - (rows - 1);
        lines.truncate(rows - 1);
        lines.push(Line::from(Span::styled(
            format!(" \u{22EF} {hidden} more lines; enlarge the window to see them"),
            Style::default().fg(theme::text_muted()),
        )));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// One centered, muted line in the middle of `area`, for a list with
/// nothing in it: it says why.
pub fn render_empty(frame: &mut Frame, area: Rect, text: &str) {
    if area.is_empty() {
        return;
    }
    let lines = wrap(text, usize::from(area.width.saturating_sub(4)).max(1));
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .min(area.height);
    let rows = Rect {
        y: area.y + (area.height - height) / 2,
        height,
        ..area
    };
    let lines: Vec<Line> = lines
        .into_iter()
        .map(|line| Line::from(Span::styled(line, Style::default().fg(theme::text_muted()))))
        .collect();
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), rows);
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    use super::{DetailGrid, split_detail};

    fn text(grid: &DetailGrid, width: u16) -> Vec<String> {
        grid.lines(width).iter().map(ToString::to_string).collect()
    }

    fn grid() -> DetailGrid {
        let mut grid = DetailGrid::default();
        grid.field("Status", "Enabled", Style::default())
            .field("VLAN", "10", Style::default())
            .field("Gateway", "10.40.10.1", Style::default())
            .wide("DNS", "1.1.1.1, 9.9.9.9", Style::default())
            .field("IPv6", "Off", Style::default());
        grid
    }

    #[test]
    fn short_fields_sit_side_by_side_with_their_columns_aligned() {
        let rows = text(&grid(), 40);
        assert_eq!(rows.len(), 4, "{rows:#?}");
        assert!(rows[0].contains("Status") && rows[0].contains("VLAN"));
        let column = |row: &str, label: &str| row.find(label);
        assert_eq!(column(&rows[0], "Status"), column(&rows[1], "Gateway"));
        assert_eq!(
            column(&rows[0], "Enabled"),
            column(&rows[1], "10.40.10.1"),
            "values start in the same column: {rows:#?}"
        );
        assert_eq!(column(&rows[0], "Enabled"), column(&rows[2], "1.1.1.1"));
    }

    #[test]
    fn a_narrow_panel_stacks_fields_and_wraps_long_values() {
        let rows = text(&grid(), 22);
        assert_eq!(rows.len(), 6, "{rows:#?}");
        assert!(rows[3].contains("1.1.1.1,"), "{rows:#?}");
        assert!(rows[4].trim_start().starts_with("9.9.9.9"), "{rows:#?}");
    }

    #[test]
    fn the_panel_takes_only_the_rows_it_needs_and_leaves_the_list_its_share() {
        let area = Rect::new(0, 0, 80, 20);
        let (list, panel) = split_detail(area, 6, 4);
        assert_eq!((list.height, panel.height), (12, 8));

        let (list, panel) = split_detail(area, 40, 4);
        assert_eq!((list.height, panel.height), (4, 16));

        let (list, panel) = split_detail(Rect::new(0, 0, 80, 5), 6, 4);
        assert_eq!((list.height, panel.height), (5, 0));
    }
}
