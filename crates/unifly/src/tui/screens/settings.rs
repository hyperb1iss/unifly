//! Settings screen — single-panel configuration form.
//!
//! Opened with `,`, not in the tab bar. Fields are grouped under section
//! headers (Connection, Appearance). Press `a` to open the About overlay.
//!
//! Esc cancels without saving. On successful connection test the config is
//! persisted and `SettingsApply` is emitted so the app can reconnect.

mod input;
mod render;
mod state;

use std::cell::{Cell, RefCell};

use color_eyre::eyre::Result;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use tokio::sync::mpsc::UnboundedSender;

use crate::tui::widgets::click::{Click, HitMap};

use crate::tui::action::Action;
use crate::tui::component::Component;
pub(super) use crate::tui::forms::controller_profile::{AuthMode, ControllerProfileDraft};

// ── State enums ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SettingsState {
    Editing,
    Testing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SettingsField {
    // Connection section
    Url,
    AuthMode,
    ApiKey,
    HostId,
    Username,
    Password,
    Site,
    Insecure,
    // Appearance section
    Theme,
    ShowDonate,
}

/// An entry in the rendered form — either a section divider or a field row.
#[derive(Debug, Clone, Copy)]
pub(super) enum FormEntry {
    Section(&'static str),
    Field(SettingsField, u16),
}

// ── Screen struct ───────────────────────────────────────────────────

#[allow(clippy::struct_excessive_bools)]
pub struct SettingsScreen {
    focused: bool,
    action_tx: Option<UnboundedSender<Action>>,
    state: SettingsState,
    active_field: SettingsField,
    draft: ControllerProfileDraft,
    auth_mode_index: usize,
    show_password: bool,
    profile_name: String,
    test_error: Option<String>,
    throbber_state: throbber_widgets_tui::ThrobberState,
    last_area: Cell<Rect>,
    theme_selector: RefCell<Option<opaline::ThemeSelectorState>>,
    /// What the last frame drew where, for clicks on the key hints.
    hits: RefCell<HitMap<Click>>,
    show_donate: bool,
}

impl Default for SettingsScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for SettingsScreen {
    fn init(&mut self, action_tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(action_tx);
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        Ok(self.handle_key_input(key))
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        Ok(self.handle_mouse_input(mouse))
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        self.apply_action(action);
        Ok(None)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        self.render_screen(frame, area);
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn id(&self) -> &'static str {
        "settings"
    }
}
