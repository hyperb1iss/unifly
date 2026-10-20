//! Shared plumbing for the list screens' tables: column widths planned from
//! the cell contents, selection that follows an entity across refreshes, and
//! a scroll position that lasts from frame to frame.

mod columns;
mod selection;

use std::cell::Cell;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Table, TableState};

pub use columns::{Column, plan_columns};
pub use selection::reselect;

/// Where a table scrolled to on the last frame.
///
/// Screens render from `&self`, so each frame draws a copy of the table's
/// state. Ratatui scrolls only as far as the selection needs from the
/// offset it is handed, so a copy that always starts at offset 0 pins the
/// selected row to the bottom of the view once the list scrolls, and
/// moving back up never shows the rows below the cursor. The offset each
/// frame scrolled to is kept here and handed to the next frame.
#[derive(Debug, Default)]
pub struct TableScroll(Cell<usize>);

impl TableScroll {
    /// Draw `table`, holding `len` one-line rows under a one-line header,
    /// with the selection in `state`, starting from where the last frame
    /// scrolled to, and keep this frame's scroll position. Returns the
    /// state as drawn, whose offset is the first row on screen.
    ///
    /// The kept offset never passes the last full page, so a list that
    /// shrinks fills the view from the bottom instead of hiding rows above
    /// blank space. Ratatui then scrolls the selected row into view.
    pub fn render(
        &self,
        frame: &mut Frame,
        table: Table<'_>,
        area: Rect,
        state: &TableState,
        len: usize,
    ) -> TableState {
        let body = usize::from(area.height.saturating_sub(1));
        let last_page = len.saturating_sub(body);
        let mut drawn = state.with_offset(self.0.get().min(last_page));
        frame.render_stateful_widget(table, area, &mut drawn);
        self.0.set(drawn.offset());
        drawn
    }
}

/// Draw with `draw` into a `width` x `height` test terminal and return the
/// rendered rows as text, for render assertions in screen tests.
#[cfg(test)]
pub(crate) fn render_lines(
    width: u16,
    height: u16,
    draw: impl FnOnce(&mut ratatui::Frame, ratatui::layout::Rect),
) -> Vec<String> {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| draw(frame, frame.area()))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Constraint;
    use ratatui::widgets::{Row, Table, TableState};

    use super::{TableScroll, render_lines};

    /// Draw `len` numbered rows under a header in a 10-row area (9 body
    /// rows) with `selected` selected, returning the body rows on screen
    /// and the state as drawn.
    fn draw(scroll: &TableScroll, len: usize, selected: usize) -> (Vec<String>, TableState) {
        let state = TableState::default().with_selected(Some(selected));
        let mut drawn = TableState::default();
        let rows = render_lines(20, 10, |frame, area| {
            let rows = (0..len).map(|index| Row::new([format!("row-{index:02}")]));
            let table = Table::new(rows, [Constraint::Length(10)]).header(Row::new(["Name"]));
            drawn = scroll.render(frame, table, area, &state, len);
        });
        let body = rows.into_iter().skip(1).map(|row| row.trim().to_owned());
        (body.collect(), drawn)
    }

    #[test]
    fn the_view_stays_scrolled_while_the_cursor_moves_back_up() {
        let scroll = TableScroll::default();
        let (_, drawn) = draw(&scroll, 40, 39);
        assert_eq!(drawn.offset(), 31);

        let (rows, drawn) = draw(&scroll, 40, 33);
        assert_eq!(drawn.offset(), 31, "moving up inside the view keeps it");
        assert_eq!(rows.first().map(String::as_str), Some("row-31"));
        assert_eq!(rows.last().map(String::as_str), Some("row-39"));

        let (_, drawn) = draw(&scroll, 40, 25);
        assert_eq!(drawn.offset(), 25, "moving above the view scrolls up");
    }

    #[test]
    fn a_shrinking_list_fills_the_view_from_the_bottom() {
        let scroll = TableScroll::default();
        draw(&scroll, 40, 39);

        let (rows, drawn) = draw(&scroll, 35, 34);
        assert_eq!(drawn.offset(), 26, "{rows:?}");
        assert_eq!(rows.first().map(String::as_str), Some("row-26"));
        assert_eq!(rows.last().map(String::as_str), Some("row-34"));

        // Still scrolled, a cursor above the clamped view pulls it up.
        draw(&scroll, 40, 39);
        let (_, drawn) = draw(&scroll, 35, 20);
        assert_eq!(drawn.offset(), 20);
    }

    #[test]
    fn a_list_shorter_than_the_view_starts_at_the_top() {
        let scroll = TableScroll::default();
        draw(&scroll, 40, 39);

        let (rows, drawn) = draw(&scroll, 3, 2);
        assert_eq!(drawn.offset(), 0);
        assert_eq!(rows.first().map(String::as_str), Some("row-00"));
    }
}
