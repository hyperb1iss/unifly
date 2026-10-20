use super::{FormEntry, SettingsField, SettingsScreen, SettingsState};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use opaline::ThemeSelectorAction;

use crate::tui::action::Action;
use crate::tui::widgets::click::{self, Click};

// ── Keyboard input ──────────────────────────────────────────────────

impl SettingsScreen {
    pub(super) fn handle_key_input(&mut self, key: KeyEvent) -> Option<Action> {
        // Theme selector overlay captures all input while open.
        if self.handle_theme_selector_key(key) {
            return None;
        }

        // Testing state: only Esc cancels.
        if self.state == SettingsState::Testing {
            if key.code == KeyCode::Esc {
                self.state = SettingsState::Editing;
                self.test_error = None;
            }
            return None;
        }

        self.test_error = None;

        // Delegate to active field's handler.
        match self.active_field {
            SettingsField::AuthMode => self.handle_auth_mode_key(key),
            SettingsField::Insecure => self.handle_toggle_key(key, |s| {
                s.draft.toggle_insecure();
            }),
            SettingsField::ShowDonate => self.handle_toggle_key(key, |s| {
                s.toggle_show_donate();
            }),
            SettingsField::Theme => self.handle_theme_key(key),
            _ => self.handle_text_field_key(key),
        }
    }

    // ── Theme selector overlay ──────────────────────────────────────

    fn handle_theme_selector_key(&mut self, key: KeyEvent) -> bool {
        let mut selector = self.theme_selector.borrow_mut();
        let Some(ref mut theme_selector) = *selector else {
            return false;
        };

        match theme_selector.handle_key(key) {
            ThemeSelectorAction::Select(theme_id) => {
                SettingsScreen::save_theme_preference(&theme_id);
                *selector = None;
            }
            ThemeSelectorAction::Cancel => {
                *selector = None;
            }
            _ => {}
        }

        true
    }

    // ── Field-specific key handlers ─────────────────────────────────

    fn handle_auth_mode_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Up | KeyCode::Left | KeyCode::Char('k' | 'h') => {
                self.cycle_auth_mode_previous();
                None
            }
            KeyCode::Down | KeyCode::Right | KeyCode::Char('j' | 'l') => {
                self.cycle_auth_mode_next();
                None
            }
            KeyCode::Tab => {
                self.focus_next();
                None
            }
            KeyCode::BackTab => {
                self.focus_prev();
                None
            }
            KeyCode::Enter => {
                self.submit_connection_test();
                None
            }
            KeyCode::Esc => Some(Action::CloseSettings),
            _ => None,
        }
    }

    /// Generic handler for checkbox / toggle fields.
    fn handle_toggle_key(&mut self, key: KeyEvent, toggle: fn(&mut Self)) -> Option<Action> {
        match key.code {
            KeyCode::Char(' ') => {
                toggle(self);
                None
            }
            KeyCode::Tab => {
                self.focus_next();
                None
            }
            KeyCode::BackTab => {
                self.focus_prev();
                None
            }
            KeyCode::Enter => {
                self.submit_connection_test();
                None
            }
            KeyCode::Esc => Some(Action::CloseSettings),
            _ => None,
        }
    }

    fn handle_theme_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Enter => {
                self.open_theme_selector();
                None
            }
            KeyCode::Tab => {
                self.focus_next();
                None
            }
            KeyCode::BackTab => {
                self.focus_prev();
                None
            }
            KeyCode::Esc => Some(Action::CloseSettings),
            _ => None,
        }
    }

    fn handle_text_field_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Tab => {
                self.focus_next();
                None
            }
            KeyCode::BackTab => {
                self.focus_prev();
                None
            }
            KeyCode::Enter => {
                self.submit_connection_test();
                None
            }
            KeyCode::Esc => Some(Action::CloseSettings),
            KeyCode::Backspace => {
                if let Some(input) = self.active_input_mut() {
                    input.pop();
                }
                None
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'u' {
                    self.show_password = !self.show_password;
                } else if let Some(input) = self.active_input_mut() {
                    input.push(c);
                }
                None
            }
            _ => None,
        }
    }

    fn open_theme_selector(&mut self) {
        *self.theme_selector.borrow_mut() = Some(
            opaline::ThemeSelectorState::with_current_selected()
                .with_derive(crate::tui::theme::derive_tokens),
        );
    }
}

// ── Action dispatch ─────────────────────────────────────────────────

impl SettingsScreen {
    pub(super) fn apply_action(&mut self, action: &Action) {
        match action {
            Action::SettingsTestResult(result) => match result {
                Ok(()) => self.send_apply(),
                Err(message) => {
                    self.state = SettingsState::Editing;
                    self.test_error = Some(message.clone());
                }
            },
            Action::Tick if self.state == SettingsState::Testing => {
                self.throbber_state.calc_next();
            }
            _ => {}
        }
    }
}

// ── Mouse input ─────────────────────────────────────────────────────

impl SettingsScreen {
    pub(super) fn handle_mouse_input(&mut self, mouse: MouseEvent) -> Option<Action> {
        // The wheel walks the theme picker, previewing each theme.
        if let Some(delta) = click::wheel_delta(mouse)
            && self.theme_selector.borrow().is_some()
        {
            let key = if delta < 0 {
                KeyCode::Up
            } else {
                KeyCode::Down
            };
            self.handle_theme_selector_key(key.into());
            return None;
        }

        let target = self.hits.borrow().clicked(mouse);
        match target {
            Some(Click::Key(key)) => return Some(Action::PressKey(key)),
            Some(Click::Absorb) => return None,
            _ => {}
        }

        if self.state != SettingsState::Editing || self.theme_selector.borrow().is_some() {
            return None;
        }

        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
            let area = self.last_area.get();
            if area.width == 0 {
                return None;
            }

            let panel = super::render::panel_rect(area);
            // inner area starts 1 row below the border, then 1 row padding
            let content_y = panel.y + 2;
            let fields_x = panel.x + 2;
            let fields_w = panel.width.saturating_sub(4);

            let mut y = content_y;
            for entry in self.form_layout() {
                match entry {
                    FormEntry::Section(_) => {
                        y += 2; // section header height
                    }
                    FormEntry::Field(field, height) => {
                        if mouse.row >= y && mouse.row < y + height {
                            self.active_field = field;

                            match field {
                                SettingsField::Insecure => {
                                    self.draft.toggle_insecure();
                                }
                                SettingsField::ShowDonate => {
                                    self.toggle_show_donate();
                                }
                                SettingsField::Theme => {
                                    self.open_theme_selector();
                                }
                                SettingsField::AuthMode => {
                                    let mid_x = fields_x + fields_w / 2;
                                    if mouse.column < mid_x {
                                        self.cycle_auth_mode_previous();
                                    } else {
                                        self.cycle_auth_mode_next();
                                    }
                                }
                                _ => {}
                            }

                            break;
                        }
                        y += height;
                    }
                }
            }
        }

        None
    }
}
