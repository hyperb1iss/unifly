use std::sync::Arc;

use ratatui::widgets::TableState;
use unifly_api::EntityId;
use unifly_api::model::{AclRule, FirewallPolicy, FirewallZone, NatPolicy};

use crate::cli::commands::util::order;
use crate::tui::action::{Action, FirewallSubTab};
use crate::tui::table::{TableScroll, reselect};

use super::FirewallScreen;
use super::reorder::{Reorder, Stage, shifted};

impl FirewallScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            action_tx: None,
            sub_tab: FirewallSubTab::default(),
            policies: Arc::new(Vec::new()),
            zones: Arc::new(Vec::new()),
            acl_rules: Arc::new(Vec::new()),
            nat_policies: Arc::new(Vec::new()),
            networks: Arc::new(Vec::new()),
            access: None,
            policy_table: TableState::default(),
            zone_table: TableState::default(),
            acl_table: TableState::default(),
            nat_table: TableState::default(),
            policy_offset: std::cell::Cell::new(0),
            zone_scroll: TableScroll::default(),
            acl_scroll: TableScroll::default(),
            nat_scroll: TableScroll::default(),
            detail_open: false,
            reorder: None,
            hits: std::cell::RefCell::default(),
        }
    }

    pub(super) fn active_table_state(&mut self) -> &mut TableState {
        match self.sub_tab {
            FirewallSubTab::Policies => &mut self.policy_table,
            FirewallSubTab::Zones => &mut self.zone_table,
            FirewallSubTab::AclRules => &mut self.acl_table,
            FirewallSubTab::NatPolicies => &mut self.nat_table,
        }
    }

    pub(super) fn active_len(&self) -> usize {
        match self.sub_tab {
            FirewallSubTab::Policies => self.policies.len(),
            FirewallSubTab::Zones => self.zones.len(),
            FirewallSubTab::AclRules => self.acl_rules.len(),
            FirewallSubTab::NatPolicies => self.nat_policies.len(),
        }
    }

    pub(super) fn selected_index(&self) -> usize {
        match self.sub_tab {
            FirewallSubTab::Policies => self.policy_table.selected().unwrap_or(0),
            FirewallSubTab::Zones => self.zone_table.selected().unwrap_or(0),
            FirewallSubTab::AclRules => self.acl_table.selected().unwrap_or(0),
            FirewallSubTab::NatPolicies => self.nat_table.selected().unwrap_or(0),
        }
    }

    pub(super) fn select(&mut self, index: usize) {
        let clamped = index.min(self.active_len().saturating_sub(1));
        self.active_table_state().select(Some(clamped));
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        if self.active_len() == 0 {
            return;
        }
        self.select(self.selected_index().saturating_add_signed(delta));
    }

    pub(super) fn sub_tab_index(&self) -> usize {
        FirewallSubTab::ALL
            .iter()
            .position(|tab| *tab == self.sub_tab)
            .unwrap_or_default()
    }

    /// Switch sub-tabs. A picked-up policy goes back where it was: its
    /// keys only make sense with the Policies table in view.
    pub(super) fn show_sub_tab(&mut self, tab: FirewallSubTab) {
        if self.is_drafting() {
            self.end_reorder();
        }
        self.sub_tab = tab;
    }

    pub(super) fn apply_action(&mut self, action: &Action) {
        match action {
            Action::FirewallPoliciesUpdated(policies) => {
                self.set_policies(order::firewall_policies(policies, &self.zones), true);
            }
            Action::FirewallZonesUpdated(zones) => {
                let first_zones = self.zones.is_empty();
                replace_rows(
                    &mut self.zones,
                    &mut self.zone_table,
                    order::firewall_zones(zones),
                );
                // Policies group by zone name, so renamed zones can move them.
                // Before the first zones arrive that grouping is provisional,
                // so the cursor keeps its row instead of chasing a policy.
                let regrouped = order::firewall_policies(&self.policies, &self.zones);
                if first_zones {
                    self.policies = Arc::new(regrouped);
                } else {
                    self.set_policies(regrouped, false);
                }
            }
            Action::AclRulesUpdated(rules) => {
                let ordered = order::acl_rules(rules);
                replace_rows(&mut self.acl_rules, &mut self.acl_table, ordered);
            }
            Action::NatPoliciesUpdated(policies) => {
                let ordered = order::nat_policies(policies);
                replace_rows(&mut self.nat_policies, &mut self.nat_table, ordered);
            }
            Action::NetworksUpdated(networks) => {
                self.networks = Arc::clone(networks);
            }
            Action::FirewallSubTab(tab) => self.show_sub_tab(*tab),
            Action::AccessUpdated(access) => self.access = Some(*access),
            // Nothing of the old controller may stay on screen, least of
            // all a move that would be written to the new one.
            Action::ControllerChanged => {
                let action_tx = self.action_tx.take();
                let focused = self.focused;
                *self = Self::new();
                self.action_tx = action_tx;
                self.focused = focused;
            }
            Action::PolicyMoveSettled { moved } => {
                let sent = self
                    .reorder
                    .as_mut()
                    .filter(|reorder| reorder.stage == Stage::Sent);
                match sent {
                    // The row holds its place until the refreshed order
                    // shows it there, so it never jumps back and forth.
                    Some(reorder) if *moved => reorder.stage = Stage::Landed { ticks: 0 },
                    Some(_) => self.end_reorder(),
                    None => {}
                }
            }
            Action::Tick if self.reorder.as_mut().is_some_and(Reorder::tick) => {
                self.end_reorder();
            }
            _ => {}
        }
    }

    /// The Policies table's rows: the controller's order, with a policy
    /// being moved shown where the move puts it.
    pub(super) fn policy_view(&self) -> Vec<Arc<FirewallPolicy>> {
        match &self.reorder {
            Some(reorder) => shifted(&self.policies, &reorder.policy.id, reorder.steps).0,
            None => self.policies.to_vec(),
        }
    }

    /// The policy on the selected row of the Policies table.
    pub(super) fn selected_policy(&self) -> Option<Arc<FirewallPolicy>> {
        let index = self.policy_table.selected().unwrap_or(0);
        self.policy_view().get(index).cloned()
    }

    pub(super) fn selected_zone(&self) -> Option<&Arc<FirewallZone>> {
        self.zones.get(self.zone_table.selected().unwrap_or(0))
    }

    pub(super) fn selected_acl_rule(&self) -> Option<&Arc<AclRule>> {
        self.acl_rules.get(self.acl_table.selected().unwrap_or(0))
    }

    pub(super) fn selected_nat_policy(&self) -> Option<&Arc<NatPolicy>> {
        self.nat_policies
            .get(self.nat_table.selected().unwrap_or(0))
    }

    /// Whether a policy is picked up and waiting for `Enter` or `Esc`.
    pub(super) fn is_drafting(&self) -> bool {
        self.reorder
            .as_ref()
            .is_some_and(|reorder| reorder.stage == Stage::Draft)
    }

    /// Put the cursor on the policy `id`, wherever the view has it.
    pub(super) fn select_policy(&mut self, id: &EntityId) {
        if let Some(index) = self
            .policy_view()
            .iter()
            .position(|policy| &policy.id == id)
        {
            self.policy_table.select(Some(index));
        }
    }

    /// Drop the move in progress and show the controller's order again,
    /// with the cursor still on the policy it was on.
    pub(super) fn end_reorder(&mut self) {
        let selected = self.selected_policy().map(|policy| policy.id.clone());
        self.reorder = None;
        if let Some(id) = selected {
            self.select_policy(&id);
        }
    }

    /// Store an ordered policy list, keeping the cursor on the policy it
    /// was on. A landed move lets go when the list is `fresh` from the
    /// controller: that is the order it was waiting for.
    fn set_policies(&mut self, ordered: Vec<Arc<FirewallPolicy>>, fresh: bool) {
        let previous = self.policy_table.selected();
        let selected = self.selected_policy().map(|policy| policy.id.clone());
        self.policies = Arc::new(ordered);
        let keep = self.reorder.as_ref().is_some_and(|reorder| {
            !(fresh && matches!(reorder.stage, Stage::Landed { .. }))
                && self
                    .policies
                    .iter()
                    .any(|policy| policy.id == reorder.policy.id)
        });
        if !keep {
            self.reorder = None;
        }
        // A draft holds as many steps as the fresh order still allows.
        if let Some(reorder) = self.reorder.as_mut() {
            reorder.steps = shifted(&self.policies, &reorder.policy.id, reorder.steps).1;
        }
        let view = self.policy_view();
        let followed = reselect(
            view.iter().map(|policy| &policy.id),
            selected.as_ref(),
            previous,
        );
        self.policy_table.select(followed.or(previous));
    }
}

/// Rows the firewall tables list, identified by their entity ID.
trait FirewallRow {
    fn row_id(&self) -> &EntityId;
}

impl FirewallRow for FirewallZone {
    fn row_id(&self) -> &EntityId {
        &self.id
    }
}

impl FirewallRow for AclRule {
    fn row_id(&self) -> &EntityId {
        &self.id
    }
}

impl FirewallRow for NatPolicy {
    fn row_id(&self) -> &EntityId {
        &self.id
    }
}

/// Swap in a refreshed, already ordered row list, keeping the table's
/// selection on the entity it was on.
fn replace_rows<T: FirewallRow>(
    rows: &mut Arc<Vec<Arc<T>>>,
    table: &mut TableState,
    next: Vec<Arc<T>>,
) {
    let previous = table.selected();
    let selected = previous
        .and_then(|index| rows.get(index))
        .map(|row| row.row_id().clone());
    *rows = Arc::new(next);
    let followed = reselect(
        rows.iter().map(|row| row.row_id()),
        selected.as_ref(),
        previous,
    );
    table.select(followed.or(previous));
}

#[cfg(test)]
mod tests;
