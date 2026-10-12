//! Shared plumbing for the list screens' tables: column widths planned from
//! the cell contents, and selection that follows an entity across refreshes.

mod columns;
mod selection;

pub use columns::{Column, plan_columns};
pub use selection::reselect;

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
