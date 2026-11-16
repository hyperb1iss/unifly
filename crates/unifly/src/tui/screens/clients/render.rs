mod detail;
mod table;

use super::ClientsScreen;

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::tui::widgets::click::Hint;

impl ClientsScreen {
    pub(super) fn render_screen(&self, frame: &mut Frame, area: Rect) {
        table::render_screen(self, frame, area);
    }

    /// What the status bar offers for the selection.
    pub(super) fn status_hints(&self) -> Vec<(u8, Hint)> {
        table::status_hints(self)
    }
}
