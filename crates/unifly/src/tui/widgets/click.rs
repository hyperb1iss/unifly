//! Click targets for mouse and touch input.
//!
//! Each frame, whatever draws a clickable thing registers its rect in a
//! [`HitMap`], and a later click resolves against what was actually drawn.
//! [`ClickLine`] builds one row of spans (tabs, key hints) and registers the
//! columns of every span that carries a target.

use std::ops::Range;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::TableState;

use crate::tui::action::Action;
use crate::tui::theme;

/// What a click on a registered target does.
#[derive(Debug, Clone)]
pub enum Click {
    /// A tab in a tab strip, by position.
    Tab(usize),
    /// A table row, by its index in the table's data.
    Row(usize),
    /// A key hint: the click presses this key.
    Key(KeyEvent),
    /// Dispatch this action.
    Action(Action),
    /// Swallow the click, for the body of a dialog or the backdrop behind it.
    Absorb,
}

/// The clickable rects of the last frame. Later targets sit on top of
/// earlier ones, so an overlay registered after the screen beneath it wins.
#[derive(Debug, Clone)]
pub struct HitMap<T> {
    targets: Vec<(Rect, T)>,
}

impl<T> Default for HitMap<T> {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
        }
    }
}

impl<T: Clone> HitMap<T> {
    /// Forget the previous frame's targets.
    pub fn clear(&mut self) {
        self.targets.clear();
    }

    pub fn push(&mut self, area: Rect, target: T) {
        if !area.is_empty() {
            self.targets.push((area, target));
        }
    }

    /// The topmost target at a cell.
    pub fn at(&self, column: u16, row: u16) -> Option<T> {
        let cell = Position::new(column, row);
        self.targets
            .iter()
            .rev()
            .find(|(area, _)| area.contains(cell))
            .map(|(_, target)| target.clone())
    }

    /// The topmost target under a left press, or `None` for any other
    /// mouse event.
    pub fn clicked(&self, mouse: MouseEvent) -> Option<T> {
        is_left_press(mouse)
            .then(|| self.at(mouse.column, mouse.row))
            .flatten()
    }
}

impl HitMap<Click> {
    /// Register each visible body row of a table drawn in `area` under a
    /// one-row header. `state` is the copy the table was rendered with, so
    /// its offset is the scroll position on screen.
    pub fn push_table_rows(&mut self, area: Rect, state: &TableState, len: usize) {
        let body = Rect {
            y: area.y.saturating_add(1),
            height: area.height.saturating_sub(1),
            ..area
        };
        for (row, index) in (body.y..body.bottom()).zip(state.offset()..len) {
            self.push(
                Rect {
                    y: row,
                    height: 1,
                    ..body
                },
                Click::Row(index),
            );
        }
    }
}

/// One row of spans, each optionally carrying a click target.
#[derive(Debug)]
pub struct ClickLine<'a, T> {
    spans: Vec<(Span<'a>, Option<T>)>,
}

impl<T> Default for ClickLine<'_, T> {
    fn default() -> Self {
        Self { spans: Vec::new() }
    }
}

impl<'a, T: Clone> ClickLine<'a, T> {
    /// Append a span that a click on resolves to `target`.
    pub fn push(&mut self, target: T, span: Span<'a>) {
        self.spans.push((span, Some(target)));
    }

    /// Append a span that is not clickable, such as a divider.
    pub fn push_gap(&mut self, span: Span<'a>) {
        self.spans.push((span, None));
    }

    fn push_span(&mut self, span: Span<'a>, target: Option<T>) {
        self.spans.push((span, target));
    }

    /// Columns the line takes when drawn.
    pub fn width(&self) -> usize {
        self.spans.iter().map(|(span, _)| span.width()).sum()
    }

    /// Register the columns each target lands on when the line is drawn
    /// left-aligned on the first row of `area`, clipped as the line is.
    pub fn register(&self, area: Rect, hits: &mut HitMap<T>) {
        for (columns, target) in self.columns(area) {
            hits.push(
                Rect::new(columns.start, area.y, columns.end - columns.start, 1),
                target,
            );
        }
    }

    fn columns(&self, area: Rect) -> Vec<(Range<u16>, T)> {
        if area.is_empty() {
            return Vec::new();
        }
        let mut x = area.x;
        let mut columns: Vec<(Range<u16>, T)> = Vec::new();
        for (span, target) in &self.spans {
            let width = u16::try_from(span.width()).unwrap_or(u16::MAX);
            let end = x.saturating_add(width).min(area.right());
            if let Some(target) = target
                && end > x
            {
                columns.push((x..end, target.clone()));
            }
            x = end;
        }
        columns
    }

    /// Register the columns each target lands on when a paragraph centers
    /// the line in the first row of `area`, using the same offset ratatui
    /// does.
    pub fn register_centered(&self, area: Rect, hits: &mut HitMap<T>) {
        let width = u16::try_from(self.width()).unwrap_or(u16::MAX);
        let offset = (area.width / 2).saturating_sub(width / 2);
        let shifted = Rect {
            x: area.x.saturating_add(offset),
            width: area.width.saturating_sub(offset),
            ..area
        };
        self.register(shifted, hits);
    }

    /// Append another line's spans and targets.
    pub fn append(&mut self, other: ClickLine<'a, T>) {
        self.spans.extend(other.spans);
    }

    /// Register the targets in `area` and return the line to draw there.
    pub fn draw(self, area: Rect, hits: &mut HitMap<T>) -> Line<'a> {
        self.register(area, hits);
        self.into_line()
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

/// One key hint: the key as shown, what it does, and the key a click on it
/// presses. Hints for keys a click cannot stand in for (`j/k navigate`)
/// press nothing.
#[derive(Debug, Clone, Copy)]
pub struct Hint {
    key: &'static str,
    label: &'static str,
    press: Option<KeyEvent>,
}

impl Hint {
    /// A hint whose click presses `press`.
    pub fn button(key: &'static str, label: &'static str, press: impl Into<KeyEvent>) -> Self {
        Self {
            key,
            label,
            press: Some(press.into()),
        }
    }

    /// A hint a click does nothing for.
    pub fn info(key: &'static str, label: &'static str) -> Self {
        Self {
            key,
            label,
            press: None,
        }
    }
}

/// A plain character key press.
pub fn char_key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

/// A key hint bar in the standard hint styles: `  key label  key label`.
pub fn key_hints(hints: &[Hint]) -> ClickLine<'static, Click> {
    key_hints_styled(hints, theme::key_hint_key(), theme::key_hint())
}

/// A key hint bar with its own key and label styles.
pub fn key_hints_styled(
    hints: &[Hint],
    key_style: Style,
    label_style: Style,
) -> ClickLine<'static, Click> {
    let mut line = ClickLine::default();
    line.push_gap(Span::raw("  "));
    line.push_hints(hints, key_style, label_style);
    line
}

impl ClickLine<'static, Click> {
    /// Append key hints two spaces apart, with no leading gap.
    pub fn push_hints(&mut self, hints: &[Hint], key_style: Style, label_style: Style) {
        for (index, hint) in hints.iter().enumerate() {
            if index > 0 {
                self.push_gap(Span::raw("  "));
            }
            let key = Span::styled(format!("{} ", hint.key), key_style);
            let label = Span::styled(hint.label, label_style);
            let target = hint.press.map(Click::Key);
            self.push_span(key, target.clone());
            self.push_span(label, target);
        }
    }
}

/// The row that line `index` of a paragraph drawn in `area` lands on,
/// clipped to `area` (empty once the paragraph runs out of room).
pub fn paragraph_row(area: Rect, index: usize) -> Rect {
    Rect {
        y: area
            .y
            .saturating_add(u16::try_from(index).unwrap_or(u16::MAX)),
        height: 1,
        ..area
    }
    .intersection(area)
}

pub fn is_left_press(mouse: MouseEvent) -> bool {
    mouse.kind == MouseEventKind::Down(MouseButton::Left)
}

/// The rows a wheel turn moves through a list: one down, or one back up.
pub fn wheel_delta(mouse: MouseEvent) -> Option<isize> {
    match mouse.kind {
        MouseEventKind::ScrollDown => Some(1),
        MouseEventKind::ScrollUp => Some(-1),
        _ => None,
    }
}

/// A mouse event of `kind` at (`column`, `row`), for click tests.
#[cfg(test)]
pub(crate) fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

/// A left press at (`column`, `row`), for click tests.
#[cfg(test)]
pub(crate) fn left_click(column: u16, row: u16) -> MouseEvent {
    mouse(MouseEventKind::Down(MouseButton::Left), column, row)
}

/// A left press on the first `label` in the first rendered row containing
/// `row_marker`, for aiming test clicks.
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

    fn line() -> ClickLine<'static, usize> {
        let mut line = ClickLine::default();
        line.push(0, Span::raw("[One]"));
        line.push_gap(Span::raw("  "));
        line.push(1, Span::raw("Two"));
        line.push_gap(Span::raw("  tail"));
        line
    }

    fn hits(area: Rect) -> HitMap<usize> {
        let mut hits = HitMap::default();
        line().register(area, &mut hits);
        hits
    }

    #[test]
    fn clicks_resolve_to_the_span_drawn_under_them() {
        let hits = hits(Rect::new(10, 4, 40, 1));

        assert_eq!(hits.clicked(left_click(10, 4)), Some(0));
        assert_eq!(hits.clicked(left_click(14, 4)), Some(0));
        assert_eq!(hits.clicked(left_click(15, 4)), None, "gap");
        assert_eq!(hits.clicked(left_click(17, 4)), Some(1));
        assert_eq!(hits.clicked(left_click(19, 4)), Some(1));
        assert_eq!(hits.clicked(left_click(20, 4)), None, "trailing gap");
        assert_eq!(hits.clicked(left_click(9, 4)), None, "left of the line");
        assert_eq!(hits.clicked(left_click(12, 5)), None, "other row");
    }

    #[test]
    fn only_a_left_press_counts_as_a_click() {
        let hits = hits(Rect::new(0, 0, 40, 1));

        for kind in [
            MouseEventKind::Down(MouseButton::Right),
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Moved,
            MouseEventKind::ScrollDown,
        ] {
            assert_eq!(hits.clicked(mouse(kind, 1, 0)), None, "{kind:?}");
        }
    }

    #[test]
    fn spans_past_the_right_edge_are_clipped_like_the_line() {
        let hits = hits(Rect::new(0, 0, 8, 1));

        assert_eq!(hits.clicked(left_click(7, 0)), Some(1));
        assert_eq!(hits.clicked(left_click(8, 0)), None);
    }

    #[test]
    fn later_targets_sit_on_top() {
        let mut hits = HitMap::default();
        hits.push(Rect::new(0, 0, 20, 10), "backdrop");
        hits.push(Rect::new(5, 5, 4, 1), "button");

        assert_eq!(hits.at(6, 5), Some("button"));
        assert_eq!(hits.at(6, 6), Some("backdrop"));
        assert_eq!(hits.at(20, 0), None);

        hits.clear();
        assert_eq!(hits.at(6, 5), None);
    }

    #[test]
    fn table_rows_follow_the_scroll_offset() {
        let mut hits = HitMap::default();
        let state = TableState::default().with_offset(3);
        // A header row, then room for four body rows but only two left.
        hits.push_table_rows(Rect::new(0, 10, 30, 5), &state, 5);

        assert!(hits.at(0, 10).is_none(), "header");
        assert!(matches!(hits.at(0, 11), Some(Click::Row(3))));
        assert!(matches!(hits.at(29, 12), Some(Click::Row(4))));
        assert!(hits.at(0, 13).is_none(), "past the last row");
    }

    #[test]
    fn key_hints_press_their_key_and_info_hints_press_nothing() {
        let line = key_hints(&[
            Hint::info("j/k", "navigate"),
            Hint::button("R", "restart", char_key('R')),
        ]);
        let mut hits = HitMap::default();
        let text = line.draw(Rect::new(0, 0, 40, 1), &mut hits).to_string();
        assert_eq!(text, "  j/k navigate  R restart");

        assert!(hits.at(2, 0).is_none());
        for column in 16..25 {
            assert!(
                matches!(hits.at(column, 0), Some(Click::Key(key)) if key == char_key('R')),
                "column {column}"
            );
        }
        assert!(hits.at(15, 0).is_none(), "gap before the button");
    }
}
