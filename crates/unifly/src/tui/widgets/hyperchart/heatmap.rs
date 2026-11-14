//! Compact channel heatmap widget for Wi-Fi spectrum views.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::tui::theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeatmapCell {
    pub label: String,
    pub your_count: usize,
    pub neighbor_count: usize,
    pub signal: Option<i32>,
    pub conflict: bool,
}

pub struct HyperHeatmap<'a> {
    cells: &'a [HeatmapCell],
    empty_message: &'a str,
}

impl<'a> HyperHeatmap<'a> {
    pub const fn new(cells: &'a [HeatmapCell]) -> Self {
        Self {
            cells,
            empty_message: "No channel data",
        }
    }

    #[must_use]
    pub fn empty_message(mut self, message: &'a str) -> Self {
        self.empty_message = message;
        self
    }
}

impl HyperHeatmap<'_> {
    /// Rows the map takes in an area `width` columns wide: its cells,
    /// wrapped, and the legend under them.
    pub fn rows(&self, width: u16) -> u16 {
        if self.cells.is_empty() {
            return 1;
        }
        u16::try_from(self.lines(width).len()).unwrap_or(u16::MAX)
    }

    /// The cells, as many to a line as fit `width`, then the legend.
    fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let mut spans = Vec::new();
        let mut line_width = 0usize;
        let max_width = usize::from(width.max(1));

        for cell in self.cells {
            let (width, mut cell_spans) = cell_spans(cell);
            if line_width > 0 && line_width + width > max_width {
                lines.push(Line::from(std::mem::take(&mut spans)));
                line_width = 0;
            }
            line_width += width;
            spans.append(&mut cell_spans);
        }

        if !spans.is_empty() {
            lines.push(Line::from(spans));
        }

        lines.push(Line::from(vec![
            Span::styled("  ░ clear  ", Style::default().fg(theme::text_muted())),
            Span::styled("▒ light  ", Style::default().fg(theme::wifi_neighbor())),
            Span::styled("▓ busy  ", Style::default().fg(theme::wifi_your_ap())),
            Span::styled("█ conflict  ", Style::default().fg(theme::warning())),
            Span::styled(
                "▲ your AP  · neighbors",
                Style::default().fg(theme::text_secondary()),
            ),
        ]));
        lines
    }
}

impl Widget for HyperHeatmap<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.cells.is_empty() {
            Paragraph::new(self.empty_message)
                .style(Style::default().fg(theme::text_muted()))
                .render(area, buf);
            return;
        }
        Paragraph::new(self.lines(area.width)).render(area, buf);
    }
}

fn cell_spans(cell: &HeatmapCell) -> (usize, Vec<Span<'static>>) {
    let intensity = cell
        .your_count
        .saturating_mul(2)
        .saturating_add(cell.neighbor_count)
        .min(4);
    let bar = match intensity {
        0 => "···",
        1 => "░░░",
        2 => "▒▒▒",
        3 => "▓▓▓",
        _ => "███",
    };
    let bar_color = if cell.conflict {
        theme::warning()
    } else if cell.your_count > 0 {
        theme::wifi_your_ap()
    } else if cell.neighbor_count > 0 {
        theme::wifi_neighbor()
    } else {
        theme::text_muted()
    };
    let label_style = if cell.conflict {
        Style::default()
            .fg(theme::warning())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::accent_secondary())
    };
    let mut spans = vec![
        Span::styled(format!("{:>4}", cell.label), label_style),
        Span::styled("▕", Style::default().fg(theme::border_unfocused())),
        Span::styled(bar.to_string(), Style::default().fg(bar_color)),
        Span::styled("▏", Style::default().fg(theme::border_unfocused())),
    ];

    let mut width = cell.label.chars().count().max(4) + 5;
    if cell.your_count > 0 {
        spans.push(Span::styled(
            "▲",
            Style::default().fg(theme::wifi_your_ap()),
        ));
        width += 1;
    }
    if cell.neighbor_count > 0 {
        let marker = format!("·{}", cell.neighbor_count.min(9));
        width += marker.chars().count();
        spans.push(Span::styled(
            marker,
            Style::default().fg(theme::wifi_neighbor()),
        ));
    }
    if let Some(signal) = cell.signal {
        let signal = format!("{signal}d");
        width += signal.chars().count() + 1;
        spans.push(Span::styled(
            format!(" {signal}"),
            Style::default().fg(theme::text_secondary()),
        ));
    }
    spans.push(Span::raw("  "));
    width += 2;

    (width, spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heatmap_cell_width_accounts_for_markers() {
        let cell = HeatmapCell {
            label: "ch6".into(),
            your_count: 1,
            neighbor_count: 3,
            signal: Some(-66),
            conflict: true,
        };

        let (width, spans) = cell_spans(&cell);

        assert!(width >= 15);
        assert!(spans.len() >= 7);
    }

    #[test]
    fn the_map_reports_the_rows_its_cells_wrap_to() {
        let cells: Vec<HeatmapCell> = (1..=11)
            .map(|channel| HeatmapCell {
                label: channel.to_string(),
                your_count: 0,
                neighbor_count: 0,
                signal: None,
                conflict: false,
            })
            .collect();
        let map = HyperHeatmap::new(&cells);
        // Each idle cell is eleven columns: one line of cells and the legend.
        assert_eq!(map.rows(121), 2);
        assert_eq!(map.rows(120), 3);
        assert_eq!(map.rows(11), 12);
        assert_eq!(HyperHeatmap::new(&[]).rows(40), 1);
    }
}
