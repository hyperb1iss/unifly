mod clients;
mod detail;
pub(in crate::tui::screens) mod grid;
mod ports;
mod table;

use ratatui::Frame;
use ratatui::layout::Rect;

use super::DevicesScreen;

impl DevicesScreen {
    pub(super) fn render_screen(&self, frame: &mut Frame, area: Rect) {
        table::render_screen(self, frame, area);
    }
}
