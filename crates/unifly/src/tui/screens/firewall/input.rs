use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};

use unifly_api::model::EntityOrigin;
use unifly_api::{Command, EntityId};

use crate::tui::action::{
    Action, ConfirmRequest, FirewallSubTab, Notice, Notification, RuleSwitch,
};
use crate::tui::widgets::click::{self, Click};

use super::FirewallScreen;
use super::reorder::{Reorder, Stage, owner, pair_range, plan, shifted};

/// The action that runs `command` under `pending` and `done` toasts.
fn run(command: Command, pending: String, done: String) -> Action {
    Action::Run {
        command: Box::new(command),
        notice: Notice::new(pending, done),
    }
}

/// Ask before switching a rule on or off: either way the firewall treats
/// traffic differently the moment it lands. The app flips the switch and
/// reports what the controller then says the rule is.
fn confirm_toggle(
    kind: &str,
    name: &str,
    enabled: bool,
    id: &EntityId,
    switch: RuleSwitch,
) -> Action {
    let (verb, cost) = if enabled {
        (
            "Disable",
            "It stops matching traffic at once; whatever comes after it decides instead.",
        )
    } else {
        ("Enable", "It starts matching traffic at once.")
    };
    Action::ShowConfirm(
        ConfirmRequest::new(
            format!("{verb} {kind} {name}?"),
            Action::SetRuleSwitch {
                id: id.clone(),
                switch,
                on: !enabled,
            },
        )
        .detail(cost),
    )
}

fn confirm_delete(kind: &str, name: &str, command: Command) -> Action {
    Action::ShowConfirm(
        ConfirmRequest::new(
            format!("Delete {kind} {name}?"),
            run(
                command,
                format!("Deleting {name}\u{2026}"),
                format!("Deleted {name}"),
            ),
        )
        .detail("This cannot be undone."),
    )
}

fn refuse(message: String) -> Action {
    Action::Notify(Notification::warning(message))
}

/// Why a rule that is not an admin's cannot take `change`, or `None` for
/// one that can. The controller refuses these writes; saying so here
/// costs no round trip and names the reason.
fn not_ours(name: &str, origin: Option<EntityOrigin>, change: &str) -> Option<Action> {
    owner(origin).map(|owner| refuse(format!("{name} is {owner} and {change}")))
}

impl FirewallScreen {
    pub(super) fn handle_key_input(&mut self, key: KeyEvent) -> Option<Action> {
        if self.is_drafting() {
            return self.handle_draft_key(key);
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
            KeyCode::Char('g') => self.select(0),
            KeyCode::Char('G') => self.select(self.active_len().saturating_sub(1)),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(10);
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(-10);
            }
            KeyCode::Char('l') | KeyCode::Right => return Some(self.step_sub_tab(1)),
            KeyCode::Char('h') | KeyCode::Left => return Some(self.step_sub_tab(-1)),
            // With no row there is nothing to show a detail of.
            KeyCode::Enter if self.active_len() > 0 => {
                self.detail_open = !self.detail_open;
            }
            // A consumed Esc still returns an action, or the App would go on
            // to its global Esc and jump back to the previous screen.
            KeyCode::Esc if self.detail_open => {
                self.detail_open = false;
                return Some(Action::Invalidate);
            }
            KeyCode::Char('K') if self.sub_tab == FirewallSubTab::Policies => {
                return self.pick_up(-1);
            }
            KeyCode::Char('J') if self.sub_tab == FirewallSubTab::Policies => {
                return self.pick_up(1);
            }
            KeyCode::Char('E') => return self.toggle_enabled(),
            KeyCode::Char('L') if self.sub_tab == FirewallSubTab::Policies => {
                return self.toggle_logging();
            }
            KeyCode::Char('D') => return self.request_delete(),
            _ => {}
        }
        None
    }

    /// Keys while a policy is picked up: it moves, lands, or goes back.
    /// Every other key is left to the app, and nothing else on this
    /// screen answers until the move is settled.
    fn handle_draft_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Char('K' | 'k') | KeyCode::Up => self.nudge(-1),
            KeyCode::Char('J' | 'j') | KeyCode::Down => self.nudge(1),
            KeyCode::Enter => return self.apply_move(),
            KeyCode::Esc => {
                self.end_reorder();
                return Some(Action::Invalidate);
            }
            _ => {}
        }
        None
    }

    fn step_sub_tab(&mut self, delta: isize) -> Action {
        let count = FirewallSubTab::ALL.len();
        let next = self
            .sub_tab_index()
            .saturating_add_signed(delta.rem_euclid(isize::try_from(count).unwrap_or(1)))
            % count;
        self.show_sub_tab(FirewallSubTab::ALL[next]);
        Action::FirewallSubTab(self.sub_tab)
    }

    /// Pick the selected policy up and move it one place, on screen only.
    /// A policy that cannot move says why here, with no trip to the
    /// controller.
    fn pick_up(&mut self, step: i32) -> Option<Action> {
        if let Some(moving) = &self.reorder {
            return Some(Action::Notify(Notification::info(format!(
                "Still moving {}",
                moving.policy.name
            ))));
        }
        let policy = self.selected_policy()?;
        let name = &policy.name;
        if let Some(refusal) = not_ours(name, policy.origin, "keeps its place") {
            return Some(refusal);
        }
        if policy.source.zone_id.is_none() || policy.destination.zone_id.is_none() {
            return Some(refuse(format!(
                "{name} has no zone pair, so it has no order to change"
            )));
        }
        let (_, taken) = shifted(&self.policies, &policy.id, step);
        if taken == 0 {
            let edge = if step < 0 { "first" } else { "last" };
            return Some(Action::Notify(Notification::info(format!(
                "{name} is already {edge} in its zone pair"
            ))));
        }
        let id = policy.id.clone();
        self.reorder = Some(Reorder {
            policy,
            steps: taken,
            stage: Stage::Draft,
        });
        self.select_policy(&id);
        None
    }

    /// Move the picked-up policy one more place, as far as its zone pair
    /// goes.
    fn nudge(&mut self, step: i32) {
        let Some(reorder) = self.reorder.as_mut() else {
            return;
        };
        let id = reorder.policy.id.clone();
        reorder.steps = shifted(&self.policies, &id, reorder.steps + step).1;
        self.select_policy(&id);
    }

    /// Send the picked-up policy's move to the controller, as the order
    /// on screen now and the order the draft leaves. A policy put back
    /// where it started has nothing to send.
    fn apply_move(&mut self) -> Option<Action> {
        let reorder = self.reorder.as_ref()?;
        let plan = (reorder.steps != 0)
            .then(|| plan(&self.policies, &reorder.policy.id, reorder.steps))
            .flatten();
        let Some(plan) = plan else {
            self.end_reorder();
            return Some(Action::Invalidate);
        };
        let reorder = self.reorder.as_mut()?;
        reorder.stage = Stage::Sent;
        Some(Action::MovePolicy {
            policy: std::sync::Arc::clone(&reorder.policy),
            plan: Box::new(plan),
        })
    }

    /// Whether `E` and `D` do anything on the selected row. Only the
    /// rules an admin made can be switched on and off or deleted.
    pub(super) fn can_toggle(&self) -> bool {
        let ours = |origin| owner(origin).is_none();
        match self.sub_tab {
            FirewallSubTab::Policies => self
                .selected_policy()
                .is_some_and(|policy| ours(policy.origin)),
            FirewallSubTab::AclRules => self
                .selected_acl_rule()
                .is_some_and(|rule| ours(rule.origin)),
            FirewallSubTab::NatPolicies => self
                .selected_nat_policy()
                .is_some_and(|rule| ours(rule.origin)),
            FirewallSubTab::Zones => false,
        }
    }

    fn toggle_enabled(&self) -> Option<Action> {
        const CHANGE: &str = "cannot be switched on or off here";
        match self.sub_tab {
            FirewallSubTab::Policies => {
                let policy = self.selected_policy()?;
                not_ours(&policy.name, policy.origin, CHANGE).or_else(|| {
                    Some(confirm_toggle(
                        "policy",
                        &policy.name,
                        policy.enabled,
                        &policy.id,
                        RuleSwitch::PolicyEnabled,
                    ))
                })
            }
            FirewallSubTab::AclRules => {
                let rule = self.selected_acl_rule()?;
                not_ours(&rule.name, rule.origin, CHANGE).or_else(|| {
                    Some(confirm_toggle(
                        "ACL rule",
                        &rule.name,
                        rule.enabled,
                        &rule.id,
                        RuleSwitch::AclEnabled,
                    ))
                })
            }
            FirewallSubTab::NatPolicies => {
                let rule = self.selected_nat_policy()?;
                not_ours(&rule.name, rule.origin, CHANGE).or_else(|| {
                    Some(confirm_toggle(
                        "NAT rule",
                        &rule.name,
                        rule.enabled,
                        &rule.id,
                        RuleSwitch::NatEnabled,
                    ))
                })
            }
            FirewallSubTab::Zones => None,
        }
    }

    /// Logging changes what is recorded, not what passes, so it needs no
    /// confirmation. It is the one switch the controller also takes on
    /// its built-in policies; the app checks afterwards that it took.
    fn toggle_logging(&self) -> Option<Action> {
        let policy = self.selected_policy()?;
        Some(Action::SetRuleSwitch {
            id: policy.id.clone(),
            switch: RuleSwitch::PolicyLogging,
            on: !policy.logging_enabled,
        })
    }

    fn request_delete(&self) -> Option<Action> {
        const CHANGE: &str = "cannot be deleted";
        match self.sub_tab {
            FirewallSubTab::Policies => {
                let policy = self.selected_policy()?;
                not_ours(&policy.name, policy.origin, CHANGE).or_else(|| {
                    Some(confirm_delete(
                        "policy",
                        &policy.name,
                        Command::DeleteFirewallPolicy {
                            id: policy.id.clone(),
                        },
                    ))
                })
            }
            FirewallSubTab::AclRules => {
                let rule = self.selected_acl_rule()?;
                not_ours(&rule.name, rule.origin, CHANGE).or_else(|| {
                    Some(confirm_delete(
                        "ACL rule",
                        &rule.name,
                        Command::DeleteAclRule {
                            id: rule.id.clone(),
                        },
                    ))
                })
            }
            FirewallSubTab::NatPolicies => {
                let rule = self.selected_nat_policy()?;
                not_ours(&rule.name, rule.origin, CHANGE).or_else(|| {
                    Some(confirm_delete(
                        "NAT rule",
                        &rule.name,
                        Command::DeleteNatPolicy {
                            id: rule.id.clone(),
                        },
                    ))
                })
            }
            FirewallSubTab::Zones => None,
        }
    }

    /// Mouse and touch input: sub-tabs and rows by click, the list by
    /// wheel. A picked-up policy holds the rows until it is settled.
    pub(super) fn handle_mouse_input(&mut self, mouse: MouseEvent) -> Option<Action> {
        if let Some(delta) = click::wheel_delta(mouse) {
            if !self.is_drafting() {
                self.move_selection(delta);
            }
            return None;
        }
        let target = self.hits.borrow().clicked(mouse)?;
        match target {
            Click::Key(key) => Some(Action::PressKey(key)),
            _ if self.is_drafting() => None,
            Click::Tab(index) => self.click_sub_tab(index),
            Click::Row(index) => {
                // A second tap on the selected row toggles its detail, like `Enter`.
                if self.selected_index() == index {
                    self.detail_open = !self.detail_open;
                } else {
                    self.select(index);
                }
                None
            }
            _ => None,
        }
    }

    /// A sub-tab switches to it, like `h`/`l`.
    fn click_sub_tab(&mut self, index: usize) -> Option<Action> {
        let tab = *FirewallSubTab::ALL.get(index)?;
        if tab == self.sub_tab {
            return None;
        }
        self.show_sub_tab(tab);
        Some(Action::FirewallSubTab(tab))
    }

    /// The selected policy's place in its zone pair: `(position, of)`,
    /// counting from one.
    pub(super) fn policy_position(&self) -> Option<(usize, usize)> {
        let view = self.policy_view();
        let at = self.policy_table.selected().unwrap_or(0);
        let pair = pair_range(&view, at);
        (!pair.is_empty()).then(|| (at - pair.start + 1, pair.len()))
    }
}

#[cfg(test)]
mod tests;
