//! Firewall screen: policies by zone pair, zones, ACL rules, and NAT
//! rules, each with a detail panel and the writes an operator reaches
//! for (enable, disable, delete, and moving a policy).

#[cfg(test)]
mod fixtures;
mod input;
mod render;
mod reorder;
mod state;

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::TableState;
use tokio::sync::mpsc::UnboundedSender;

use unifly_api::Network;
use unifly_api::model::{AclRule, FirewallPolicy, FirewallZone, NatPolicy};

use crate::tui::action::{Access, Action, FirewallSubTab};
use crate::tui::component::{Component, LIST_HELP};
use crate::tui::table::TableScroll;
use crate::tui::widgets::click::{Click, HitMap};

use self::reorder::Reorder;

pub struct FirewallScreen {
    focused: bool,
    action_tx: Option<UnboundedSender<Action>>,
    sub_tab: FirewallSubTab,
    /// Policies in the controller's evaluation order.
    policies: Arc<Vec<Arc<FirewallPolicy>>>,
    zones: Arc<Vec<Arc<FirewallZone>>>,
    acl_rules: Arc<Vec<Arc<AclRule>>>,
    nat_policies: Arc<Vec<Arc<NatPolicy>>>,
    /// Networks, for member and filter names.
    networks: Arc<Vec<Arc<Network>>>,
    /// What the controller can reach, once it has connected.
    access: Option<Access>,
    policy_table: TableState,
    zone_table: TableState,
    acl_table: TableState,
    nat_table: TableState,
    /// First row of the Policies table on screen, zone-pair rows counted.
    policy_offset: Cell<usize>,
    zone_scroll: TableScroll,
    acl_scroll: TableScroll,
    nat_scroll: TableScroll,
    /// Whether the selected row's detail panel is open.
    detail_open: bool,
    /// A policy being moved: picked up, sent, or just landed.
    reorder: Option<Reorder>,
    /// What the last frame drew where, for clicks.
    hits: RefCell<HitMap<Click>>,
}

impl Default for FirewallScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for FirewallScreen {
    fn init(&mut self, action_tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(action_tx);
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        Ok(self.handle_key_input(key))
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        Ok(self.handle_mouse_input(mouse))
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        self.apply_action(action);
        Ok(None)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        self.render_screen(frame, area);
    }

    fn help(&self) -> Vec<(&'static str, &'static str)> {
        if self.is_drafting() {
            return vec![
                ("K/J k/j \u{2191}/\u{2193}", "Move it up / down"),
                ("Enter", "Apply the move"),
                ("Esc", "Put it back"),
            ];
        }
        let mut keys = LIST_HELP.to_vec();
        keys.extend([
            ("h/l \u{2190}/\u{2192}", "Previous / next sub-tab"),
            ("Enter", "Show / hide the detail"),
            ("Esc", "Hide the detail"),
        ]);
        match self.sub_tab {
            FirewallSubTab::Policies => keys.extend([
                ("E", "Enable / disable"),
                ("L", "Logging on / off"),
                ("K/J", "Pick it up to move it"),
                ("D", "Delete the policy"),
            ]),
            FirewallSubTab::AclRules | FirewallSubTab::NatPolicies => {
                keys.extend([("E", "Enable / disable"), ("D", "Delete the rule")]);
            }
            FirewallSubTab::Zones => {}
        }
        keys
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn id(&self) -> &'static str {
        "Firewall"
    }
}
