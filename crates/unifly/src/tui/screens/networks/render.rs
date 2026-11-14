mod detail;
mod edit;
mod table;

use ratatui::Frame;
use ratatui::layout::Rect;

use super::NetworksScreen;
use crate::tui::widgets::click::Hint;

impl NetworksScreen {
    pub(super) fn render_screen(&self, frame: &mut Frame, area: Rect) {
        table::render_screen(self, frame, area);
    }

    /// What the status bar offers for the selection.
    pub(super) fn status_hints(&self) -> Vec<(u8, Hint)> {
        table::status_hints(self)
    }
}
