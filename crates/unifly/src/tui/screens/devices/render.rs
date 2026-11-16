mod clients;
mod detail;
pub(in crate::tui::screens) mod grid;
mod ports;
mod table;

use ratatui::Frame;
use ratatui::layout::Rect;

use super::DevicesScreen;
use crate::tui::widgets::click::Hint;

impl DevicesScreen {
    pub(super) fn render_screen(&self, frame: &mut Frame, area: Rect) {
        table::render_screen(self, frame, area);
    }

    /// What the status bar offers for the selection.
    pub(super) fn status_hints(&self) -> Vec<(u8, Hint)> {
        table::status_hints(self)
    }
}
