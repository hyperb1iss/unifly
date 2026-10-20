use crossterm::event::{KeyCode, KeyEvent, MouseEvent};

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

    /// Mouse and touch input: key hints press their key.
    pub(super) fn handle_mouse_input(&mut self, mouse: MouseEvent) -> Option<Action> {
        let target = self.hits.borrow().clicked(mouse)?;
        match target {
            Click::Key(key) => Some(Action::PressKey(key)),
            _ => None,
        }
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
