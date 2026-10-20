use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};

use super::TopologyScreen;
use crate::tui::action::Action;
use crate::tui::widgets::click::Click;

impl TopologyScreen {
    pub(super) fn handle_key_input(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Left => Some(self.pan(-5.0, 0.0)),
            KeyCode::Right => Some(self.pan(5.0, 0.0)),
            KeyCode::Up => Some(self.pan(0.0, 5.0)),
            KeyCode::Down => Some(self.pan(0.0, -5.0)),
            KeyCode::Char('+' | '=') => Some(self.zoom_in()),
            KeyCode::Char('-') => Some(self.zoom_out()),
            KeyCode::Char('f') => Some(self.reset_view(Action::TopologyFit)),
            KeyCode::Char('r') => Some(self.reset_view(Action::TopologyReset)),
            _ => None,
        }
    }

    /// Mouse and touch input: key hints press their key, a drag pans the
    /// map under the pointer, and the wheel zooms.
    pub(super) fn handle_mouse_input(&mut self, mouse: MouseEvent) -> Option<Action> {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let target = self.hits.borrow().clicked(mouse);
                if let Some(Click::Key(key)) = target {
                    return Some(Action::PressKey(key));
                }
                self.drag_from = Some((mouse.column, mouse.row));
                None
            }
            // Only a drag that began on the map pans it; one that began on a
            // hint button never armed a drag.
            MouseEventKind::Drag(MouseButton::Left) => {
                let (column, row) = self.drag_from?;
                self.drag_from = Some((mouse.column, mouse.row));
                self.drag_by(
                    f64::from(mouse.column) - f64::from(column),
                    f64::from(mouse.row) - f64::from(row),
                );
                None
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.drag_from = None;
                None
            }
            MouseEventKind::ScrollUp => Some(self.zoom_in()),
            MouseEventKind::ScrollDown => Some(self.zoom_out()),
            _ => None,
        }
    }

    /// Move the map with a drag of `columns` across and `rows` down, so
    /// the point under the pointer stays under it. One cell is `1 / zoom`
    /// layout units, and panning moves the viewport, so the map moves the
    /// opposite way.
    fn drag_by(&mut self, columns: f64, rows: f64) {
        self.pan_x -= columns / self.zoom;
        self.pan_y += rows / self.zoom;
    }

    fn pan(&mut self, delta_x: f64, delta_y: f64) -> Action {
        self.pan_x += delta_x;
        self.pan_y += delta_y;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::as_conversions
        )]
        {
            Action::TopologyPan(delta_x as i16, delta_y as i16)
        }
    }

    fn zoom_in(&mut self) -> Action {
        self.zoom = (self.zoom * 1.2).min(5.0);
        Action::TopologyZoom(self.zoom)
    }

    fn zoom_out(&mut self) -> Action {
        self.zoom = (self.zoom / 1.2).max(0.2);
        Action::TopologyZoom(self.zoom)
    }

    fn reset_view(&mut self, action: Action) -> Action {
        self.pan_x = 0.0;
        self.pan_y = 0.0;
        self.zoom = 1.0;
        action
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{MouseButton, MouseEventKind};

    use crate::tui::action::Action;
    use crate::tui::component::Component;
    use crate::tui::screens::topology::TopologyScreen;
    use crate::tui::table::render_lines;
    use crate::tui::widgets::click::{self, click_label, mouse};

    #[test]
    fn a_drag_pans_the_map_with_the_pointer() {
        let mut screen = TopologyScreen::new();
        screen.handle_mouse_input(mouse(MouseEventKind::Down(MouseButton::Left), 10, 5));
        screen.handle_mouse_input(mouse(MouseEventKind::Drag(MouseButton::Left), 14, 7));

        // Dragging right and down moves the viewport left and up.
        assert!(
            (screen.pan_x + 4.0).abs() < f64::EPSILON,
            "{}",
            screen.pan_x
        );
        assert!(
            (screen.pan_y - 2.0).abs() < f64::EPSILON,
            "{}",
            screen.pan_y
        );

        screen.handle_mouse_input(mouse(MouseEventKind::Up(MouseButton::Left), 14, 7));
        screen.handle_mouse_input(mouse(MouseEventKind::Drag(MouseButton::Left), 30, 9));
        assert!(
            (screen.pan_x + 4.0).abs() < f64::EPSILON,
            "a released drag stops"
        );

        // At 200% a cell is half a layout unit.
        screen.zoom = 2.0;
        screen.handle_mouse_input(mouse(MouseEventKind::Down(MouseButton::Left), 10, 5));
        screen.handle_mouse_input(mouse(MouseEventKind::Drag(MouseButton::Left), 12, 5));
        assert!(
            (screen.pan_x + 5.0).abs() < f64::EPSILON,
            "{}",
            screen.pan_x
        );
    }

    #[test]
    fn a_drag_that_starts_on_a_hint_never_pans() {
        let mut screen = TopologyScreen::new();
        let rows = render_lines(120, 20, |frame, area| screen.render(frame, area));
        let press = click_label(&rows, "zoom out", "zoom out");
        screen.handle_mouse_input(press);

        for column in [press.column + 3, press.column + 9] {
            screen.handle_mouse_input(mouse(MouseEventKind::Drag(MouseButton::Left), column, 2));
        }

        assert!(screen.pan_x.abs() < f64::EPSILON, "{}", screen.pan_x);
        assert!(screen.pan_y.abs() < f64::EPSILON, "{}", screen.pan_y);
    }

    #[test]
    fn the_wheel_zooms_and_hints_win_over_a_drag() {
        let mut screen = TopologyScreen::new();
        let zoomed = screen.handle_mouse_input(mouse(MouseEventKind::ScrollUp, 10, 5));
        assert!(matches!(zoomed, Some(Action::TopologyZoom(zoom)) if zoom > 1.0));

        let rows = render_lines(120, 20, |frame, area| screen.render(frame, area));
        let action = screen.handle_mouse_input(click_label(&rows, "zoom out", "zoom out"));
        assert!(
            matches!(action, Some(Action::PressKey(key)) if key == click::char_key('-')),
            "{action:?}"
        );
        assert!(screen.drag_from.is_none(), "a hint tap starts no drag");
    }
}
