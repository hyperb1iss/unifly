//! Settings screen: one scrolling form for the profile the session runs on.
//!
//! Opened with `,`, not in the tab bar. Fields are grouped under section
//! rules (Profile, Connection, Appearance), one row each, and the form
//! scrolls to keep the focused row in view.
//!
//! `Enter` tests the connection with what the form holds. A test that
//! passes saves that profile and emits `ApplyConnection` so the app
//! reconnects with it; `Esc` cancels a running test, and on the form asks
//! before dropping edits. Theme and sponsor-button changes apply at once.

mod input;
mod render;
mod state;
#[cfg(test)]
mod tests;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use color_eyre::eyre::Result;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use tokio::sync::mpsc::UnboundedSender;
use unifly_api::SiteHint;

use crate::tui::action::Action;
use crate::tui::component::Component;
use crate::tui::forms::controller_profile::{ControllerProfileDraft, DraftField, Reveal};
use crate::tui::forms::probe::{ProbeFailure, ProbeSlot};
use crate::tui::forms::session::Session;
use crate::tui::widgets::click::{Click, HitMap};
use crate::tui::widgets::line_input::LineInput;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SettingsState {
    Editing,
    /// A connection test is running.
    Testing,
    /// The test passed and the profile is being saved.
    Saving,
    /// Asking whether to drop unsaved edits before leaving.
    Confirming(Leave),
    /// Typing the name of a new profile.
    Naming,
}

/// Where the form goes once it may drop its edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Leave {
    Close,
    /// Show another saved profile.
    Profile(String),
    /// Set up a new profile with this name.
    NewProfile(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SettingsField {
    Profile,
    Url,
    AuthMode,
    ApiKey,
    HostId,
    Username,
    Password,
    Totp,
    Site,
    Insecure,
    CaCert,
    Timeout,
    Theme,
    ShowDonate,
}

/// A row of the form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FormEntry {
    Section(&'static str),
    Field(SettingsField),
    /// A read-only row: its label and what it says.
    Info(&'static str, String),
}

/// What a click on the form does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FieldHit {
    /// Focus the field, and act on it where a click is the whole action.
    Focus(SettingsField),
    /// Step a picker one choice back or forward.
    Step(SettingsField, isize),
}

/// What the area under the form is saying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Message {
    Failure(ProbeFailure),
    Error(String),
    Notice(String),
}

#[allow(clippy::struct_excessive_bools)]
pub struct SettingsScreen {
    focused: bool,
    action_tx: Option<UnboundedSender<Action>>,
    state: SettingsState,
    active_field: SettingsField,
    draft: ControllerProfileDraft,
    /// The saved form of the profile as loaded, to tell edits from none.
    pristine: String,
    reveal: Reveal,
    /// The profile the form shows and saves.
    profile_name: String,
    /// Every profile in the config, in the order the picker walks them.
    profiles: Vec<String>,
    /// The name being typed for a new profile.
    new_profile: LineInput,
    /// The session this screen opened in: its profile and launch flags.
    session: Session,
    /// `[defaults].timeout`, shown where the profile sets none.
    default_timeout: u64,
    /// The config file the form reads profiles from and writes preferences
    /// to. `None` for a screen that is not backed by one.
    config_path: Option<PathBuf>,
    message: Option<Message>,
    probe: ProbeSlot,
    /// The connection a passed test produced, held while the save runs.
    tested: Option<Box<unifly_api::ControllerConfig>>,
    /// The controller's sites, and which controller they were read from.
    known_sites: Vec<SiteHint>,
    sites_for: String,
    /// Set once the user types a site by hand instead of picking one.
    site_typed: bool,
    /// Draw every value masked: the session is redacting what it shows.
    private: bool,
    throbber_state: throbber_widgets_tui::ThrobberState,
    /// First form row drawn, and whether it follows the focused field.
    scroll: Cell<usize>,
    follow_focus: Cell<bool>,
    theme_selector: RefCell<Option<opaline::ThemeSelectorState>>,
    /// What the last frame drew where: key hints and overlays, then the
    /// form's own rows.
    hits: RefCell<HitMap<Click>>,
    field_hits: RefCell<HitMap<FieldHit>>,
    show_donate: bool,
}

impl Default for SettingsScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsField {
    /// The draft field behind this form field, when it edits the profile.
    pub(super) fn draft_field(self) -> Option<DraftField> {
        match self {
            Self::Url => Some(DraftField::Url),
            Self::AuthMode => Some(DraftField::AuthMode),
            Self::ApiKey => Some(DraftField::ApiKey),
            Self::HostId => Some(DraftField::HostId),
            Self::Username => Some(DraftField::Username),
            Self::Password => Some(DraftField::Password),
            Self::Totp => Some(DraftField::Totp),
            Self::Site => Some(DraftField::Site),
            Self::Insecure => Some(DraftField::Insecure),
            Self::CaCert => Some(DraftField::CaCert),
            Self::Timeout => Some(DraftField::Timeout),
            Self::Profile | Self::Theme | Self::ShowDonate => None,
        }
    }

    pub(super) fn for_draft(field: DraftField) -> Self {
        match field {
            DraftField::Url => Self::Url,
            DraftField::AuthMode => Self::AuthMode,
            DraftField::ApiKey => Self::ApiKey,
            DraftField::HostId => Self::HostId,
            DraftField::Username => Self::Username,
            DraftField::Password => Self::Password,
            DraftField::Totp => Self::Totp,
            DraftField::Site => Self::Site,
            DraftField::Insecure => Self::Insecure,
            DraftField::CaCert => Self::CaCert,
            DraftField::Timeout => Self::Timeout,
        }
    }
}

impl Component for SettingsScreen {
    fn init(&mut self, action_tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(action_tx);
        self.check_keyring();
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        Ok(self.handle_key_input(key))
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        Ok(self.handle_mouse_input(mouse))
    }

    fn handle_paste(&mut self, text: &str) -> Option<Action> {
        self.paste(text);
        None
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        Ok(self.apply_action(action))
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        self.render_screen(frame, area);
    }

    fn captures_input(&self) -> bool {
        self.typing()
    }

    fn help(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("Tab/\u{2193}  Shift+Tab/\u{2191}", "Next / previous field"),
            ("\u{2190}/\u{2192}", "Change a picker, move in a text field"),
            ("Space", "Toggle a checkbox"),
            ("Ctrl+T", "Show or hide the API key or password"),
            ("n", "On Profile: set up a new profile"),
            ("Enter", "Test the connection, save, and reconnect"),
            ("Esc", "Cancel a test, or close Settings"),
        ]
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
