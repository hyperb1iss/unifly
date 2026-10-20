use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};

use crate::tui::action::{Action, Direction, FirewallSubTab};
use crate::tui::widgets::click::{self, Click};

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

    /// Mouse and touch input: sub-tabs and rows by click, the list by wheel.
    pub(super) fn handle_mouse_input(&mut self, mouse: MouseEvent) -> Option<Action> {
        if let Some(delta) = click::wheel_delta(mouse) {
            self.move_selection(delta);
            return None;
        }
        let target = self.hits.borrow().clicked(mouse)?;
        match target {
            Click::Tab(index) => self.click_sub_tab(index),
            Click::Row(index) => {
                self.select(index);
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
        self.sub_tab = tab;
        Some(Action::FirewallSubTab(tab))
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::action::{Action, FirewallSubTab};
    use crate::tui::component::Component;
    use crate::tui::screens::firewall::FirewallScreen;
    use crate::tui::table::render_lines;
    use crate::tui::widgets::click::click_label;

    #[test]
    fn clicking_a_sub_tab_switches_to_it() {
        let mut screen = FirewallScreen::new();
        for (label, tab) in [
            ("Zones", FirewallSubTab::Zones),
            ("ACL Rules", FirewallSubTab::AclRules),
            ("NAT", FirewallSubTab::NatPolicies),
            ("Policies", FirewallSubTab::Policies),
        ] {
            let rows = render_lines(100, 12, |frame, area| screen.render(frame, area));
            let action = screen.handle_mouse_input(click_label(&rows, "ACL Rules", label));
            assert!(
                matches!(action, Some(Action::FirewallSubTab(clicked)) if clicked == tab),
                "{label}: {action:?}"
            );
            assert_eq!(screen.sub_tab, tab);
        }

        let rows = render_lines(100, 12, |frame, area| screen.render(frame, area));
        let action = screen.handle_mouse_input(click_label(&rows, "ACL Rules", "Policies"));
        assert!(action.is_none(), "the active tab is already showing");
    }
}
