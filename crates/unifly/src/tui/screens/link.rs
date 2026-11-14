//! Whether the controller is reachable, as a screen tracks it to say why
//! it has nothing to draw.

use crate::tui::action::Action;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::tui::screens) enum Link {
    /// The first connection, or a retry, is under way.
    #[default]
    Connecting,
    Up,
    Down,
}

impl Link {
    /// Follow a connection action. Other actions leave the state alone.
    pub(in crate::tui::screens) fn follow(&mut self, action: &Action) {
        match action {
            Action::Connecting | Action::Reconnecting { .. } => *self = Self::Connecting,
            Action::Connected => *self = Self::Up,
            Action::Disconnected(_) => *self = Self::Down,
            _ => {}
        }
    }

    /// What to show in place of data while the controller is unreachable.
    pub(in crate::tui::screens) fn waiting(self) -> Option<&'static str> {
        match self {
            Self::Connecting => Some("Connecting\u{2026}"),
            Self::Down => Some("Not connected"),
            Self::Up => None,
        }
    }
}
