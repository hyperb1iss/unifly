use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use opaline::ThemeSelectorAction;

use super::{FieldHit, Leave, SettingsField, SettingsScreen, SettingsState};
use crate::tui::action::Action;
use crate::tui::forms::controller_profile::DraftField;
use crate::tui::widgets::click::{self, Click};
use crate::tui::widgets::line_input::Edit;

/// Whether `key` is Ctrl+`ch`, the form's own chords.
fn is_ctrl(key: KeyEvent, ch: char) -> bool {
    key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char(ch)
}

/// A plain letter, as the non-text fields read their shortcuts.
fn plain_char(key: KeyEvent) -> Option<char> {
    match key.code {
        KeyCode::Char(ch)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            Some(ch)
        }
        _ => None,
    }
}

// ── Keyboard input ──────────────────────────────────────────────────

impl SettingsScreen {
    pub(super) fn handle_key_input(&mut self, key: KeyEvent) -> Option<Action> {
        // Theme selector overlay captures all input while open.
        if self.handle_theme_selector_key(key) {
            return None;
        }

        match self.state.clone() {
            SettingsState::Testing => {
                if key.code == KeyCode::Esc {
                    self.cancel_test();
                }
                None
            }
            // A save cannot be taken back halfway, so it runs to its end.
            SettingsState::Saving => None,
            SettingsState::Confirming(to) => match (key.code, plain_char(key)) {
                (KeyCode::Enter, _) | (_, Some('y' | 'Y')) => self.leave_now(to),
                (KeyCode::Esc, _) | (_, Some('n' | 'N')) => {
                    self.state = SettingsState::Editing;
                    None
                }
                _ => None,
            },
            SettingsState::Naming => self.handle_naming_key(key),
            SettingsState::Editing => self.handle_editing_key(key),
        }
    }

    fn handle_theme_selector_key(&mut self, key: KeyEvent) -> bool {
        let picked = {
            let mut selector = self.theme_selector.borrow_mut();
            let Some(ref mut theme_selector) = *selector else {
                return false;
            };
            match theme_selector.handle_key(key) {
                ThemeSelectorAction::Select(theme_id) => {
                    *selector = None;
                    Some(theme_id)
                }
                ThemeSelectorAction::Cancel => {
                    *selector = None;
                    None
                }
                _ => None,
            }
        };
        if let Some(theme_id) = picked {
            self.save_theme_preference(&theme_id);
        }
        true
    }

    fn handle_naming_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc => {
                self.state = SettingsState::Editing;
                self.message = None;
                None
            }
            KeyCode::Enter => self.finish_naming(),
            _ => {
                if self
                    .new_profile
                    .handle_key_accepting(key, |ch| !ch.is_whitespace())
                    == Edit::Changed
                {
                    self.message = None;
                }
                None
            }
        }
    }

    fn handle_editing_key(&mut self, key: KeyEvent) -> Option<Action> {
        self.follow_focus.set(true);
        match key.code {
            KeyCode::Tab | KeyCode::Down => self.focus_next(),
            KeyCode::BackTab | KeyCode::Up => self.focus_prev(),
            KeyCode::Esc => return self.leave(Leave::Close),
            KeyCode::Enter if self.active_field == SettingsField::Theme => {
                self.open_theme_selector();
            }
            KeyCode::Enter => self.submit(),
            _ if is_ctrl(key, 't') => self.toggle_reveal(),
            _ => return self.handle_field_key(key),
        }
        None
    }

    /// Keys that belong to the focused field: typing in a text field, or
    /// the shortcuts of a picker or checkbox.
    fn handle_field_key(&mut self, key: KeyEvent) -> Option<Action> {
        if let Some(field) = self.active_text_field() {
            self.edit_text(field, key);
            return None;
        }

        // The site picker keeps the letters for typing a name by hand, for
        // a site the list does not show; only the arrows step it.
        if self.active_field == SettingsField::Site {
            match (key.code, plain_char(key)) {
                (KeyCode::Left, _) => self.step_site(-1),
                (KeyCode::Right, _) => self.step_site(1),
                (_, Some(ch)) if DraftField::Site.accepts(ch) => {
                    self.site_typed = true;
                    self.draft.site.clear();
                    self.draft.site.insert(ch);
                    self.message = None;
                }
                _ => {}
            }
            return None;
        }

        let step = match (key.code, plain_char(key)) {
            (KeyCode::Left, _) | (_, Some('h')) => Some(-1),
            (KeyCode::Right, _) | (_, Some('l')) => Some(1),
            _ => None,
        };
        match (self.active_field, plain_char(key)) {
            (_, Some('j')) => self.focus_next(),
            (_, Some('k')) => self.focus_prev(),
            (SettingsField::Profile, Some('n')) => self.begin_naming(),
            (SettingsField::Insecure, Some(' ')) => self.toggle(SettingsField::Insecure),
            (SettingsField::ShowDonate, Some(' ')) => self.toggle(SettingsField::ShowDonate),
            (SettingsField::Theme, Some(' ')) => self.open_theme_selector(),
            (field, _) => {
                if let Some(by) = step {
                    return self.step(field, by);
                }
            }
        }
        None
    }

    fn edit_text(&mut self, field: DraftField, key: KeyEvent) {
        let Some(input) = self.draft.input_mut(field) else {
            return;
        };
        if input.handle_key_accepting(key, |ch| field.accepts(ch)) == Edit::Changed {
            self.message = None;
        }
    }

    /// Step the picker `field` shows one choice back or forward.
    fn step(&mut self, field: SettingsField, by: isize) -> Option<Action> {
        match field {
            SettingsField::AuthMode => self.step_auth_mode(by),
            SettingsField::Site if self.picks_site() => self.step_site(by),
            SettingsField::Profile => {
                if let Some(name) = self.neighbor_profile(by) {
                    return self.leave(Leave::Profile(name));
                }
            }
            _ => {}
        }
        None
    }

    fn toggle(&mut self, field: SettingsField) {
        self.message = None;
        match field {
            SettingsField::Insecure => self.draft.toggle_insecure(),
            SettingsField::ShowDonate => self.toggle_show_donate(),
            _ => {}
        }
    }

    /// Show or hide the secret in the focused field. A session that is
    /// redacting what it shows never reveals one.
    fn toggle_reveal(&mut self) {
        if self.private {
            return;
        }
        if let Some(field) = self.active_field.draft_field() {
            self.reveal.toggle(field);
        }
    }

    fn open_theme_selector(&mut self) {
        *self.theme_selector.borrow_mut() = Some(
            opaline::ThemeSelectorState::with_current_selected()
                .with_derive(crate::tui::theme::derive_tokens),
        );
    }

    /// Insert pasted text into the field being typed in.
    pub(super) fn paste(&mut self, text: &str) {
        let changed = match self.state {
            SettingsState::Naming => self.new_profile.paste(text, |ch| !ch.is_whitespace()),
            SettingsState::Editing => {
                let Some(field) = self.active_text_field() else {
                    return;
                };
                let Some(input) = self.draft.input_mut(field) else {
                    return;
                };
                input.paste(text, |ch| field.accepts(ch))
            }
            _ => return,
        };
        if changed == Edit::Changed {
            self.message = None;
        }
    }
}

// ── Action dispatch ─────────────────────────────────────────────────

impl SettingsScreen {
    pub(super) fn apply_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::SetupProbeFinished { generation, result } => {
                self.finish_test(*generation, result);
                Some(Action::Invalidate)
            }
            Action::SetupSaveFinished { generation, result } => Some(
                self.finish_save(*generation, result)
                    .unwrap_or(Action::Invalidate),
            ),
            Action::SetupKeyringChecked {
                profile,
                api_key,
                password,
            } if *profile == self.profile_name => {
                self.draft.note_keyring(*api_key, *password);
                Some(Action::Invalidate)
            }
            // The spinner only turns when a frame is drawn for it.
            Action::Tick
                if matches!(self.state, SettingsState::Testing | SettingsState::Saving) =>
            {
                self.throbber_state.calc_next();
                Some(Action::Invalidate)
            }
            _ => None,
        }
    }
}

// ── Mouse input ─────────────────────────────────────────────────────

impl SettingsScreen {
    pub(super) fn handle_mouse_input(&mut self, mouse: MouseEvent) -> Option<Action> {
        let picking_theme = self.theme_selector.borrow().is_some();
        if let Some(delta) = click::wheel_delta(mouse) {
            if picking_theme {
                // The wheel walks the theme picker, previewing each theme.
                let key = if delta < 0 {
                    KeyCode::Up
                } else {
                    KeyCode::Down
                };
                self.handle_theme_selector_key(key.into());
            } else if self.state == SettingsState::Editing {
                // The form scrolls on its own until a key moves the focus.
                self.follow_focus.set(false);
                self.scroll
                    .set(self.scroll.get().saturating_add_signed(delta));
            }
            return None;
        }

        let target = self.hits.borrow().clicked(mouse);
        match target {
            Some(Click::Key(key)) => return Some(Action::PressKey(key)),
            Some(_) => return None,
            None => {}
        }

        if self.state != SettingsState::Editing || picking_theme {
            return None;
        }
        // Only rows the last frame drew are targets, each over the columns
        // it was drawn in.
        let hit = self.field_hits.borrow().clicked(mouse)?;
        match hit {
            FieldHit::Step(field, by) => {
                self.focus(field);
                self.step(field, by)
            }
            FieldHit::Focus(field) => {
                self.focus(field);
                match field {
                    SettingsField::Insecure | SettingsField::ShowDonate => self.toggle(field),
                    SettingsField::Theme => self.open_theme_selector(),
                    _ => {}
                }
                None
            }
        }
    }
}
