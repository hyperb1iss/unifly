use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::action::{Action, Direction, FirewallSubTab};

use super::FirewallScreen;

impl FirewallScreen {
    pub(super) fn handle_key_input(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1);
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1);
                None
            }
            KeyCode::Char('g') => {
                self.select(0);
                Some(Action::ScrollToTop)
            }
            KeyCode::Char('G') => {
                let len = self.active_len();
                if len > 0 {
                    self.select(len - 1);
                }
                Some(Action::ScrollToBottom)
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(10);
                Some(Action::PageDown)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(-10);
                Some(Action::PageUp)
            }
            KeyCode::Char('l') => {
                self.sub_tab = match self.sub_tab {
                    FirewallSubTab::Policies => FirewallSubTab::Zones,
                    FirewallSubTab::Zones => FirewallSubTab::AclRules,
                    FirewallSubTab::AclRules => FirewallSubTab::NatPolicies,
                    FirewallSubTab::NatPolicies => FirewallSubTab::Policies,
                };
                Some(Action::FirewallSubTab(self.sub_tab))
            }
            KeyCode::Char('h') => {
                self.sub_tab = match self.sub_tab {
                    FirewallSubTab::Policies => FirewallSubTab::NatPolicies,
                    FirewallSubTab::Zones => FirewallSubTab::Policies,
                    FirewallSubTab::AclRules => FirewallSubTab::Zones,
                    FirewallSubTab::NatPolicies => FirewallSubTab::AclRules,
                };
                Some(Action::FirewallSubTab(self.sub_tab))
            }
            KeyCode::Char('K') if self.sub_tab == FirewallSubTab::Policies => self
                .selected_policy_id()
                .map(|id| Action::ReorderPolicy(id, Direction::Up)),
            KeyCode::Char('J') if self.sub_tab == FirewallSubTab::Policies => self
                .selected_policy_id()
                .map(|id| Action::ReorderPolicy(id, Direction::Down)),
            _ => None,
        }
    }
}
