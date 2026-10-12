//! Firewall screen — zone-pair policies with sub-tabs (spec §2.5).

mod input;
mod render;
mod state;

use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::TableState;
use tokio::sync::mpsc::UnboundedSender;

use unifly_api::Network;
use unifly_api::model::{AclRule, FirewallPolicy, FirewallZone, NatPolicy};

use crate::tui::action::{Action, FirewallSubTab};
use crate::tui::component::Component;

pub struct FirewallScreen {
    focused: bool,
    action_tx: Option<UnboundedSender<Action>>,
    sub_tab: FirewallSubTab,
    policies: Arc<Vec<Arc<FirewallPolicy>>>,
    zones: Arc<Vec<Arc<FirewallZone>>>,
    acl_rules: Arc<Vec<Arc<AclRule>>>,
    nat_policies: Arc<Vec<Arc<NatPolicy>>>,
    /// Networks, for the zone table's member names.
    networks: Arc<Vec<Arc<Network>>>,
    policy_table: TableState,
    zone_table: TableState,
    acl_table: TableState,
    nat_table: TableState,
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

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        self.apply_action(action);
        Ok(None)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        self.render_screen(frame, area);
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
