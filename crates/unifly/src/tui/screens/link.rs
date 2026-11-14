//! Whether the controller is reachable, as a screen tracks it to say why
//! it has nothing to draw.

use crate::tui::action::Action;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::tui::screens) enum Link {
    /// The first connection, or a retry after losing it, is under way.
    #[default]
    Connecting,
    Up,
    /// Connected, but the controller's last refresh failed. What is loaded
    /// stays on screen and requests are still made: one missed refresh is
    /// not a lost controller.
    Unsteady,
    Down,
}

impl Link {
    /// Follow a connection action. Other actions leave the state alone.
    ///
    /// `Reconnecting` means two things. After a connect it reports a
    /// refresh that failed while the clients are still up; after a failed
    /// connect it is the next attempt.
    pub(in crate::tui::screens) fn follow(&mut self, action: &Action) {
        match action {
            Action::Connecting => *self = Self::Connecting,
            Action::Reconnecting { .. } => {
                *self = if self.reachable() {
                    Self::Unsteady
                } else {
                    Self::Connecting
                };
            }
            Action::Connected => *self = Self::Up,
            Action::Disconnected(_) => *self = Self::Down,
            _ => {}
        }
    }

    /// Whether the controller can be asked for data.
    pub(in crate::tui::screens) fn reachable(self) -> bool {
        matches!(self, Self::Up | Self::Unsteady)
    }

    /// What to show in place of data while the controller is unreachable.
    pub(in crate::tui::screens) fn waiting(self) -> Option<&'static str> {
        match self {
            Self::Connecting => Some("Connecting\u{2026}"),
            Self::Down => Some("Not connected"),
            Self::Up | Self::Unsteady => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_refresh_after_a_connect_keeps_the_controller_reachable() {
        let mut link = Link::default();
        link.follow(&Action::Connected);
        link.follow(&Action::Reconnecting { attempt: 1 });
        assert_eq!(link, Link::Unsteady);
        assert!(link.reachable());
        assert_eq!(link.waiting(), None);

        link.follow(&Action::Reconnecting { attempt: 2 });
        assert_eq!(link, Link::Unsteady, "a second failure changes nothing");
        link.follow(&Action::Connected);
        assert_eq!(link, Link::Up);
    }

    #[test]
    fn a_retry_after_a_failed_connect_is_still_connecting() {
        let mut link = Link::default();
        link.follow(&Action::Disconnected("refused".into()));
        assert_eq!(link.waiting(), Some("Not connected"));
        link.follow(&Action::Reconnecting { attempt: 2 });
        assert_eq!(link, Link::Connecting);
        assert!(!link.reachable());
    }
}
