use std::sync::Arc;

use ratatui::widgets::TableState;

use unifly_api::Network;

use super::NetworksScreen;
use super::edit::FormError;
use crate::cli::commands::util::order;
use crate::tui::action::Action;
use crate::tui::table::{TableScroll, reselect};

pub(super) fn format_lease_time(secs: u64) -> String {
    if secs >= 86400 && secs.is_multiple_of(86400) {
        format!("{}d", secs / 86400)
    } else if secs >= 3600 && secs.is_multiple_of(3600) {
        format!("{}h", secs / 3600)
    } else if secs >= 60 && secs.is_multiple_of(60) {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

impl NetworksScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            networks: Arc::new(Vec::new()),
            zones: Arc::new(Vec::new()),
            access: None,
            table_state: TableState::default(),
            table_scroll: TableScroll::default(),
            detail_open: false,
            form: None,
            hits: std::cell::RefCell::default(),
            action_tx: None,
        }
    }

    pub(super) fn selected_index(&self) -> usize {
        self.table_state.selected().unwrap_or(0)
    }

    pub(super) fn select(&mut self, index: usize) {
        let clamped = index.min(self.networks.len().saturating_sub(1));
        self.table_state.select(Some(clamped));
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        if self.networks.is_empty() {
            return;
        }
        self.select(self.selected_index().saturating_add_signed(delta));
    }

    pub(super) fn selected_network(&self) -> Option<&Arc<Network>> {
        self.networks.get(self.selected_index())
    }

    pub(super) fn apply_action(&mut self, action: &Action) {
        match action {
            Action::NetworksUpdated(networks) => self.apply_networks_update(networks),
            Action::FirewallZonesUpdated(zones) => self.zones = Arc::clone(zones),
            Action::AccessUpdated(access) => self.access = Some(*access),
            // Nothing of the old controller may stay on screen, least of
            // all a form that would save to it.
            Action::ControllerChanged => {
                let action_tx = self.action_tx.take();
                let focused = self.focused;
                *self = Self::new();
                self.action_tx = action_tx;
                self.focused = focused;
            }
            Action::NetworkSave(..) | Action::NetworkCreate(_) => {
                if let Some(form) = self.form.as_mut() {
                    form.saving = true;
                    form.error = None;
                }
            }
            Action::NetworkEditResult(result) => {
                let Some(form) = self.form.as_mut().filter(|form| form.saving) else {
                    return;
                };
                match result {
                    Ok(()) => self.form = None,
                    Err(message) => {
                        form.saving = false;
                        form.error = Some(FormError {
                            field: None,
                            message: message.clone(),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    /// Store a refreshed network list in table order, keeping the selection
    /// on the network it was on.
    fn apply_networks_update(&mut self, networks: &[Arc<Network>]) {
        let selected = self.selected_network().map(|network| network.id.clone());
        let previous = self.table_state.selected();
        self.networks = Arc::new(order::networks(networks));
        let next = reselect(
            self.networks.iter().map(|network| &network.id),
            selected.as_ref(),
            previous,
        );
        self.table_state.select(next.or(previous));
        if self.networks.is_empty() {
            self.detail_open = false;
        }
    }
}

#[cfg(test)]
mod tests;
