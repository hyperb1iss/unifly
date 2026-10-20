//! One row of tabs that remembers which tab owns each column, so a click
//! resolves against exactly what was drawn.
//!
//! Screens build a [`TabStrip`], keep its [`TabTargets`] from render, and
//! ask the targets which tab a later click landed on.

use std::ops::Range;

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

/// Tab labels and the gaps between them, built span by span.
#[derive(Debug, Default)]
pub struct TabStrip<'a> {
    spans: Vec<(Span<'a>, Option<usize>)>,
}

impl<'a> TabStrip<'a> {
    /// Append a span that belongs to the tab at `index`.
    pub fn push_tab(&mut self, index: usize, span: Span<'a>) {
        self.spans.push((span, Some(index)));
    }

    /// Append a span that belongs to no tab, such as a divider or trailing
    /// context after the tabs.
    pub fn push_gap(&mut self, span: Span<'a>) {
        self.spans.push((span, None));
    }

    /// Columns the strip takes when drawn.
    pub fn width(&self) -> usize {
        self.spans.iter().map(|(span, _)| span.width()).sum()
    }

    /// Where each tab lands when the strip is drawn left-aligned on the
    /// first row of `area`. Columns past the right edge are clipped, as
    /// the drawn line is.
    pub fn targets(&self, area: Rect) -> TabTargets {
        if area.is_empty() {
            return TabTargets::default();
        }

        let right = area.right();
        let mut x = area.x;
        let mut ranges = Vec::new();
        for (span, owner) in &self.spans {
            let width = u16::try_from(span.width()).unwrap_or(u16::MAX);
            let end = x.saturating_add(width).min(right);
            if let Some(index) = owner
                && end > x
            {
                ranges.push((x..end, *index));
            }
            x = end;
        }

        TabTargets {
            row: area.y,
            ranges,
        }
    }

    pub fn into_line(self) -> Line<'a> {
        Line::from(
            self.spans
                .into_iter()
                .map(|(span, _)| span)
                .collect::<Vec<_>>(),
        )
    }
}

/// The on-screen columns of each tab in a drawn [`TabStrip`]. The default
/// value has no targets, for a strip that is not on screen.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TabTargets {
    row: u16,
    ranges: Vec<(Range<u16>, usize)>,
}

impl TabTargets {
    /// Index of the tab under a left click, or `None` for any other mouse
    /// event or a click that misses every tab.
    pub fn clicked(&self, mouse: MouseEvent) -> Option<usize> {
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) || mouse.row != self.row {
            return None;
        }
        self.ranges
            .iter()
            .find(|(columns, _)| columns.contains(&mouse.column))
            .map(|(_, index)| *index)
    }
}

/// A left click at (`column`, `row`), for click tests across the TUI.
#[cfg(test)]
pub(crate) fn left_click(column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: crossterm::event::KeyModifiers::NONE,
    }
}

/// A left click on the first `label` in the first rendered row containing
/// `row_marker`, for aiming test clicks at a tab.
#[cfg(test)]
pub(crate) fn click_label(rows: &[String], row_marker: &str, label: &str) -> MouseEvent {
    let (row, text) = rows
        .iter()
        .enumerate()
        .find(|(_, text)| text.contains(row_marker))
        .unwrap_or_else(|| panic!("no row contains {row_marker:?}:\n{}", rows.join("\n")));
    let byte = text
        .find(label)
        .unwrap_or_else(|| panic!("{label:?} not in {text:?}"));
    left_click(
        u16::try_from(text[..byte].chars().count()).expect("column fits in u16"),
        u16::try_from(row).expect("row fits in u16"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip() -> TabStrip<'static> {
        let mut strip = TabStrip::default();
        strip.push_tab(0, Span::raw("[One]"));
        strip.push_gap(Span::raw("  "));
        strip.push_tab(1, Span::raw("Two"));
        strip.push_gap(Span::raw("  tail"));
        strip
    }

    #[test]
    fn clicks_resolve_to_the_tab_drawn_under_them() {
        let targets = strip().targets(Rect::new(10, 4, 40, 1));

        assert_eq!(targets.clicked(left_click(10, 4)), Some(0));
        assert_eq!(targets.clicked(left_click(14, 4)), Some(0));
        assert_eq!(targets.clicked(left_click(15, 4)), None, "gap");
        assert_eq!(targets.clicked(left_click(17, 4)), Some(1));
        assert_eq!(targets.clicked(left_click(19, 4)), Some(1));
        assert_eq!(targets.clicked(left_click(20, 4)), None, "trailing gap");
        assert_eq!(targets.clicked(left_click(9, 4)), None, "left of strip");
        assert_eq!(targets.clicked(left_click(12, 5)), None, "other row");
    }

    #[test]
    fn only_a_left_press_counts_as_a_click() {
        let targets = strip().targets(Rect::new(0, 0, 40, 1));
        let mut event = left_click(1, 0);

        for kind in [
            MouseEventKind::Down(MouseButton::Right),
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::Moved,
            MouseEventKind::ScrollDown,
        ] {
            event.kind = kind;
            assert_eq!(targets.clicked(event), None, "{kind:?}");
        }
    }

    #[test]
    fn tabs_past_the_right_edge_are_clipped_like_the_line() {
        let targets = strip().targets(Rect::new(0, 0, 8, 1));

        assert_eq!(targets.clicked(left_click(7, 0)), Some(1));
        assert_eq!(targets.clicked(left_click(8, 0)), None);
    }

    #[test]
    fn an_undrawn_strip_has_no_targets() {
        assert_eq!(
            strip().targets(Rect::new(0, 3, 0, 1)),
            TabTargets::default()
        );
        assert_eq!(TabTargets::default().clicked(left_click(0, 0)), None);
    }
}
