use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};

use unifly_api::EntityId;

use super::NetworksScreen;
use super::edit::{FormError, NetworkForm, Submit, Target};
use crate::tui::action::{Action, ConfirmRequest, Notification};
use crate::tui::widgets::click::{self, Click};

impl NetworksScreen {
    pub(super) fn handle_key_input(&mut self, key: KeyEvent) -> Option<Action> {
        if self.form.is_some() {
            return self.handle_form_key(key);
        }

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
            KeyCode::Char('g') => self.select(0),
            KeyCode::Char('G') => self.select(self.networks.len().saturating_sub(1)),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(10);
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(-10);
            }
            // With no network there is nothing to show a detail of.
            KeyCode::Enter if !self.networks.is_empty() => {
                self.detail_open = !self.detail_open;
            }
            // A consumed Esc still returns an action, or the App would go on
            // to its global Esc and jump back to the previous screen.
            KeyCode::Esc if self.detail_open => {
                self.detail_open = false;
                return Some(Action::Invalidate);
            }
            KeyCode::Char('e') => {
                let network = self.selected_network()?;
                self.form = Some(NetworkForm::edit(network));
                // The detail stays behind the form, to read while editing.
                self.detail_open = true;
            }
            KeyCode::Char('c') => return self.open_create_form(),
            KeyCode::Char('D') => return self.request_delete(),
            _ => {}
        }
        None
    }

    /// Open the form for a new network, where the controller can take one.
    /// Networks on screen prove the Integration API answers, whatever the
    /// access report says.
    fn open_create_form(&mut self) -> Option<Action> {
        if self.networks.is_empty() && self.access.is_some_and(|access| !access.integration) {
            return Some(Action::Notify(Notification::warning(
                "Creating a network needs the Integration API: add an API key to this profile",
            )));
        }
        self.form = Some(NetworkForm::create());
        None
    }

    fn request_delete(&self) -> Option<Action> {
        let network = self.selected_network()?;
        if network.is_default {
            return Some(Action::Notify(Notification::warning(format!(
                "{} is the default network and cannot be deleted",
                network.name
            ))));
        }
        Some(Action::RequestDeleteNetwork(network.id.clone()))
    }

    /// The check of what uses network `id` is back. Ask whether to
    /// delete it only if the cursor is still on it and no form is open:
    /// the answer is a single `y`, and the user may since have moved to
    /// another network or started typing into a form.
    pub(super) fn answer_delete_check(
        &self,
        id: &EntityId,
        name: &str,
        confirm: &ConfirmRequest,
    ) -> Action {
        let still_on_it = self.form.is_none()
            && self
                .selected_network()
                .is_some_and(|network| &network.id == id);
        if still_on_it {
            Action::ShowConfirm(confirm.clone())
        } else {
            Action::Notify(Notification::info(format!(
                "Checked what uses {name}. Press D on it again to delete it."
            )))
        }
    }

    /// Mouse and touch input: rows and key hints by click, the list by
    /// wheel. The form holds the input while it is open.
    pub(super) fn handle_mouse_input(&mut self, mouse: MouseEvent) -> Option<Action> {
        if let Some(form) = self.form.as_mut() {
            if form.saving {
                return None;
            }
            if let Some(delta) = click::wheel_delta(mouse) {
                form.step_focus(delta);
                return None;
            }
            return match self.hits.borrow().clicked(mouse)? {
                Click::Row(index) => {
                    // A tap focuses the field, and flips it when it is a
                    // yes/no field: there is nothing else to do with one.
                    let field = form.fields().get(index).copied()?;
                    form.focus_on(field);
                    if field.is_toggle() {
                        form.toggle();
                    }
                    None
                }
                Click::Key(key) => Some(Action::PressKey(key)),
                _ => None,
            };
        }

        if let Some(delta) = click::wheel_delta(mouse) {
            self.move_selection(delta);
            return None;
        }
        let target = self.hits.borrow().clicked(mouse)?;
        match target {
            Click::Row(index) => {
                // A second tap on the selected row toggles its detail, like `Enter`.
                if self.selected_index() == index {
                    self.detail_open = !self.detail_open;
                } else {
                    self.select(index);
                }
                None
            }
            Click::Key(key) => Some(Action::PressKey(key)),
            _ => None,
        }
    }

    fn handle_form_key(&mut self, key: KeyEvent) -> Option<Action> {
        let form = self.form.as_mut()?;
        // The controller has the draft: it stays as sent until it answers.
        if form.saving {
            return None;
        }
        match key.code {
            KeyCode::Esc => {
                self.form = None;
            }
            KeyCode::Enter => return self.submit_form(),
            KeyCode::Tab | KeyCode::Down => form.step_focus(1),
            KeyCode::BackTab | KeyCode::Up => form.step_focus(-1),
            KeyCode::Char(' ') if form.focused().is_toggle() => form.toggle(),
            _ => {
                form.edit_key(key);
            }
        }
        None
    }

    /// Validate the draft and send it. A draft that cannot be saved stays
    /// open with the reason on the field to fix.
    fn submit_form(&mut self) -> Option<Action> {
        let form = self.form.as_mut()?;
        // Save to the network the form was opened on, never to whatever
        // row the selection sits on after a refresh.
        if let Target::Edit { id, name } = &form.target
            && !self.networks.iter().any(|network| &network.id == id)
        {
            let gone = Notification::warning(format!(
                "{name} is no longer on the controller, so nothing was saved"
            ));
            self.form = None;
            return Some(Action::Notify(gone));
        }
        match form.submit() {
            Err(FormError { field, message }) => {
                if let Some(field) = field {
                    form.focus_on(field);
                }
                form.error = Some(FormError { field, message });
                None
            }
            Ok(Submit::Unchanged) => {
                self.form = None;
                Some(Action::Notify(Notification::info("Nothing changed")))
            }
            Ok(Submit::Create(request)) => Some(Action::NetworkCreate(request)),
            Ok(Submit::Update { id, request }) => {
                let save = Action::NetworkSave(id, request);
                if !form.is_disruptive() {
                    return Some(save);
                }
                let name = match &form.target {
                    Target::Edit { name, .. } => name.clone(),
                    Target::Create => String::new(),
                };
                let mut detail = form.changes();
                detail.push("Devices on it can lose their connection while it applies.".into());
                Some(Action::ShowConfirm(
                    ConfirmRequest::new(format!("Save these changes to {name}?"), save)
                        .detail(detail.join("\n")),
                ))
            }
        }
    }
}
