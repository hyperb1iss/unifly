use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};

use super::{OnboardingScreen, TestPhase, WizardField, WizardHit, WizardStep};
use crate::tui::action::Action;
use crate::tui::forms::controller_profile::AuthMode;
use crate::tui::widgets::click::Click;
use crate::tui::widgets::line_input::Edit;

/// A plain letter: lists and the Testing step read their shortcuts as
/// these, never as a Ctrl or Alt chord.
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

/// `index` moved one step through a list of `len`, stopping at its ends.
fn moved(index: usize, len: usize, forward: bool) -> usize {
    if forward {
        (index + 1).min(len.saturating_sub(1))
    } else {
        index.saturating_sub(1)
    }
}

impl OnboardingScreen {
    pub(super) fn handle_key_input(&mut self, key: KeyEvent) -> Option<Action> {
        match self.step {
            WizardStep::Welcome | WizardStep::Done => match key.code {
                KeyCode::Enter => self.advance(),
                KeyCode::Esc => self.go_back(),
                _ => None,
            },
            WizardStep::AuthMode => {
                let len = AuthMode::ALL.len();
                self.handle_list_key(key, |screen, forward| {
                    screen.auth_mode_index = moved(screen.auth_mode_index, len, forward);
                })
            }
            WizardStep::Site if self.picks_site() => {
                let len = self.known_sites.len();
                self.handle_list_key(key, |screen, forward| {
                    screen.site_index = moved(screen.site_index, len, forward);
                })
            }
            WizardStep::Url | WizardStep::Credentials | WizardStep::Site => {
                self.handle_form_key(key)
            }
            WizardStep::Testing => self.handle_testing_key(key),
        }
    }

    fn handle_list_key(
        &mut self,
        key: KeyEvent,
        step: impl FnOnce(&mut Self, bool),
    ) -> Option<Action> {
        match (key.code, plain_char(key)) {
            (KeyCode::Up, _) | (_, Some('k')) => step(self, false),
            (KeyCode::Down, _) | (_, Some('j')) => step(self, true),
            (KeyCode::Enter, _) => return self.advance(),
            (KeyCode::Esc, _) => return self.go_back(),
            _ => return None,
        }
        self.error = None;
        None
    }

    fn handle_form_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Enter => return self.advance(),
            KeyCode::Esc => return self.go_back(),
            KeyCode::Tab | KeyCode::Down => self.step_field(true),
            KeyCode::BackTab | KeyCode::Up => self.step_field(false),
            KeyCode::Char('t') if key.modifiers == KeyModifiers::CONTROL => self.toggle_reveal(),
            _ => {
                self.handle_field_key(key);
                return None;
            }
        }
        self.error = None;
        self.notice = None;
        None
    }

    fn handle_field_key(&mut self, key: KeyEvent) {
        let changed = match self.active_field() {
            Some(WizardField::Insecure) if plain_char(key) == Some(' ') => {
                self.draft.toggle_insecure();
                true
            }
            Some(WizardField::Text(field)) => self.draft.input_mut(field).is_some_and(|input| {
                input.handle_key_accepting(key, |ch| field.accepts(ch)) == Edit::Changed
            }),
            _ => false,
        };
        if changed {
            self.error = None;
            self.notice = None;
        }
    }

    fn handle_testing_key(&mut self, key: KeyEvent) -> Option<Action> {
        match (key.code, plain_char(key)) {
            (KeyCode::Esc, _) => return self.go_back(),
            (KeyCode::Enter, _) | (_, Some('r')) if self.phase == TestPhase::Idle => {
                self.start_test();
            }
            (_, Some('t')) => self.skip_verification_and_retry(),
            _ => {}
        }
        None
    }

    /// Show or hide the secret in the focused field. A session that is
    /// redacting what it shows never reveals one.
    fn toggle_reveal(&mut self) {
        if self.private {
            return;
        }
        if let Some(field) = self.active_text_field() {
            self.reveal.toggle(field);
        }
    }

    /// Insert pasted text into the field being typed in.
    pub(super) fn paste(&mut self, text: &str) {
        let Some(field) = self.active_text_field() else {
            return;
        };
        if let Some(input) = self.draft.input_mut(field)
            && input.paste(text, |ch| field.accepts(ch)) == Edit::Changed
        {
            self.error = None;
            self.notice = None;
        }
    }

    pub(super) fn apply_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::SetupProbeFinished { generation, result } => {
                self.finish_test(*generation, result);
                Some(Action::Invalidate)
            }
            Action::SetupSaveFinished { generation, result } => {
                self.finish_save(*generation, result);
                Some(Action::Invalidate)
            }
            // The spinner only turns when a frame is drawn for it.
            Action::Tick if self.phase != TestPhase::Idle => {
                self.throbber_state.calc_next();
                Some(Action::Invalidate)
            }
            _ => None,
        }
    }

    pub(super) fn handle_mouse_input(&mut self, mouse: MouseEvent) -> Option<Action> {
        if let Some(Click::Key(key)) = self.hits.borrow().clicked(mouse) {
            return Some(Action::PressKey(key));
        }
        let hit = self.targets.borrow().clicked(mouse)?;
        match (hit, self.step) {
            (WizardHit::Choice(index), WizardStep::AuthMode) => self.auth_mode_index = index,
            (WizardHit::Choice(index), WizardStep::Site) => self.site_index = index,
            (WizardHit::Field(index), _) if index < self.fields().len() => {
                self.field = index;
                if self.active_field() == Some(WizardField::Insecure) {
                    self.draft.toggle_insecure();
                }
            }
            _ => {}
        }
        None
    }
}
