//! Component trait — the building block for every UI element.

use color_eyre::eyre::Result;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::{Frame, layout::Rect};
use tokio::sync::mpsc::UnboundedSender;

use crate::tui::action::Action;

/// Every UI element implements Component.
///
/// Lifecycle: `init` → (`handle_key_event` | `handle_mouse_event` | `update` | `render`)*
pub trait Component: Send {
    /// Called once when the component is mounted.
    /// Receives the action sender for dispatching actions to the app loop.
    fn init(&mut self, _action_tx: UnboundedSender<Action>) -> Result<()> {
        Ok(())
    }

    /// Handle a keyboard event. Return an Action to dispatch, or None.
    fn handle_key_event(&mut self, _key: KeyEvent) -> Result<Option<Action>> {
        Ok(None)
    }

    /// Handle a mouse event. Return an Action to dispatch, or None.
    fn handle_mouse_event(&mut self, _mouse: MouseEvent) -> Result<Option<Action>> {
        Ok(None)
    }

    /// Process a dispatched action. May return a follow-up action.
    fn update(&mut self, _action: &Action) -> Result<Option<Action>> {
        Ok(None)
    }

    /// Render into the provided frame area.
    fn render(&self, frame: &mut Frame, area: Rect);

    /// Whether the component is taking typed input, such as a form field.
    /// Its keys then skip the global shortcuts, so typing `q` or a digit
    /// edits the field instead of quitting or switching screens.
    fn captures_input(&self) -> bool {
        false
    }

    /// The filter this component's search applies, or `None` when search
    /// filters nothing here. Search only opens where it filters.
    fn search_query(&self) -> Option<&str> {
        None
    }

    /// Whether this component currently holds input focus.
    #[allow(dead_code)]
    fn focused(&self) -> bool {
        false
    }

    /// Set focus state.
    fn set_focused(&mut self, _focused: bool) {}

    /// Unique identifier for this component (for focus management).
    #[allow(dead_code)]
    fn id(&self) -> &str;
}
