use std::sync::Arc;

use ratatui::widgets::TableState;
use unifly_api::EntityId;
use unifly_api::model::{AclRule, FirewallPolicy, FirewallZone, NatPolicy};

use crate::cli::commands::util::order;
use crate::tui::action::{Action, Direction, FirewallSubTab};
use crate::tui::table::reselect;

use super::FirewallScreen;

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
            policy_table: TableState::default(),
            zone_table: TableState::default(),
            acl_table: TableState::default(),
            nat_table: TableState::default(),
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
        let len = self.active_len();
        let clamped = if len == 0 { 0 } else { index.min(len - 1) };
        self.active_table_state().select(Some(clamped));
    }

    #[allow(clippy::cast_sign_loss, clippy::as_conversions)]
    pub(super) fn move_selection(&mut self, delta: isize) {
        let len = self.active_len();
        if len == 0 {
            return;
        }

        #[allow(clippy::cast_possible_wrap)]
        let current = self.selected_index() as isize;
        #[allow(clippy::cast_possible_wrap)]
        let next = (current + delta).clamp(0, len as isize - 1);
        self.select(next as usize);
    }

    pub(super) fn sub_tab_index(&self) -> usize {
        match self.sub_tab {
            FirewallSubTab::Policies => 0,
            FirewallSubTab::Zones => 1,
            FirewallSubTab::AclRules => 2,
            FirewallSubTab::NatPolicies => 3,
        }
    }

    pub(super) fn apply_action(&mut self, action: &Action) {
        match action {
            Action::FirewallPoliciesUpdated(policies) => {
                let ordered = order::firewall_policies(policies, &self.zones);
                replace_rows(&mut self.policies, &mut self.policy_table, ordered);
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
                    replace_rows(&mut self.policies, &mut self.policy_table, regrouped);
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
            Action::FirewallSubTab(tab) => {
                self.sub_tab = *tab;
            }
            Action::ReorderPolicy(index, direction) => self.reorder_policy(*index, *direction),
            _ => {}
        }
    }

    /// Move a policy one row within its zone pair. Policies only ever run
    /// against the other policies of their own zone pair, so a move across
    /// the boundary into a neighbouring pair's rows is refused.
    fn reorder_policy(&mut self, index: usize, direction: Direction) {
        let len = self.policies.len();
        let target = match direction {
            Direction::Up if index > 0 => index - 1,
            Direction::Down if index + 1 < len => index + 1,
            _ => return,
        };

        let (Some(moving), Some(other)) = (self.policies.get(index), self.policies.get(target))
        else {
            return;
        };
        let same_pair = moving.source.zone_id == other.source.zone_id
            && moving.destination.zone_id == other.destination.zone_id;
        if !same_pair {
            return;
        }

        Arc::make_mut(&mut self.policies).swap(index, target);
        self.select(target);
    }
}

/// Rows the firewall tables list, identified by their entity ID.
trait FirewallRow {
    fn row_id(&self) -> &EntityId;
}

impl FirewallRow for FirewallPolicy {
    fn row_id(&self) -> &EntityId {
        &self.id
    }
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
mod tests {
    use super::*;

    use unifly_api::model::{AclRule, FirewallPolicy, FirewallZone};

    fn policy(name: &str) -> Arc<FirewallPolicy> {
        zoned_policy(name, None, None, 1)
    }

    fn zoned_policy(
        name: &str,
        source: Option<&str>,
        destination: Option<&str>,
        index: i32,
    ) -> Arc<FirewallPolicy> {
        Arc::new(
            serde_json::from_value(serde_json::json!({
                "id": name,
                "name": name,
                "description": null,
                "enabled": true,
                "index": index,
                "action": "Allow",
                "ip_version": "Both",
                "source": { "zone_id": source, "filter": null },
                "destination": { "zone_id": destination, "filter": null },
                "source_summary": "src",
                "destination_summary": "dst",
                "protocol_summary": "Any",
                "schedule": null,
                "ipsec_mode": null,
                "connection_states": [],
                "logging_enabled": false,
                "origin": null
            }))
            .expect("policy fixture should deserialize"),
        )
    }

    fn zone(name: &str) -> Arc<FirewallZone> {
        Arc::new(
            serde_json::from_value(serde_json::json!({
                "id": name,
                "name": name,
                "network_ids": [],
                "origin": null
            }))
            .expect("zone fixture should deserialize"),
        )
    }

    fn acl_rule(name: &str) -> Arc<AclRule> {
        Arc::new(
            serde_json::from_value(serde_json::json!({
                "id": name,
                "name": name,
                "enabled": true,
                "rule_type": "Ipv4",
                "action": "Allow",
                "source_summary": "src",
                "destination_summary": "dst",
                "origin": null
            }))
            .expect("acl fixture should deserialize"),
        )
    }

    #[test]
    fn select_clamps_to_active_tab_length() {
        let mut screen = FirewallScreen::new();
        screen.zones = Arc::new(vec![zone("lan"), zone("iot")]);
        screen.sub_tab = FirewallSubTab::Zones;

        screen.select(99);

        assert_eq!(screen.zone_table.selected(), Some(1));
        assert_eq!(screen.policy_table.selected(), None);
    }

    #[test]
    fn reorder_policy_swaps_items_and_updates_selection() {
        let mut screen = FirewallScreen::new();
        screen.policies = Arc::new(vec![policy("one"), policy("two"), policy("three")]);
        screen.policy_table.select(Some(1));

        screen.apply_action(&Action::ReorderPolicy(1, Direction::Down));

        let names: Vec<_> = screen
            .policies
            .iter()
            .map(|policy| policy.name.as_str())
            .collect();
        assert_eq!(names, vec!["one", "three", "two"]);
        assert_eq!(screen.policy_table.selected(), Some(2));
    }

    #[test]
    fn selected_index_tracks_active_tab() {
        let mut screen = FirewallScreen::new();
        screen.policy_table.select(Some(2));
        screen.zone_table.select(Some(1));
        screen.acl_rules = Arc::new(vec![
            acl_rule("one"),
            acl_rule("two"),
            acl_rule("three"),
            acl_rule("four"),
        ]);
        screen.acl_table.select(Some(3));

        screen.sub_tab = FirewallSubTab::Policies;
        assert_eq!(screen.selected_index(), 2);

        screen.sub_tab = FirewallSubTab::Zones;
        assert_eq!(screen.selected_index(), 1);

        screen.sub_tab = FirewallSubTab::AclRules;
        assert_eq!(screen.selected_index(), 3);
    }

    fn policy_names(screen: &FirewallScreen) -> Vec<&str> {
        screen
            .policies
            .iter()
            .map(|policy| policy.name.as_str())
            .collect()
    }

    fn zoned_fixture() -> (Vec<Arc<FirewallZone>>, Vec<Arc<FirewallPolicy>>) {
        let zones = vec![zone("lan"), zone("wan"), zone("iot")];
        let policies = vec![
            zoned_policy("wan-lan default", Some("wan"), Some("lan"), i32::MAX),
            zoned_policy("lan-wan second", Some("lan"), Some("wan"), 10_001),
            zoned_policy("iot-lan block", Some("iot"), Some("lan"), 10_000),
            zoned_policy("lan-wan first", Some("lan"), Some("wan"), 10_000),
            zoned_policy("wan-lan allow", Some("wan"), Some("lan"), 10_000),
        ];
        (zones, policies)
    }

    #[test]
    fn policies_list_in_evaluation_order_per_zone_pair() {
        let (zones, policies) = zoned_fixture();
        let mut screen = FirewallScreen::new();
        screen.apply_action(&Action::FirewallPoliciesUpdated(Arc::new(policies)));
        screen.apply_action(&Action::FirewallZonesUpdated(Arc::new(zones)));

        assert_eq!(
            policy_names(&screen),
            vec![
                "iot-lan block",
                "lan-wan first",
                "lan-wan second",
                "wan-lan allow",
                "wan-lan default",
            ]
        );
    }

    #[test]
    fn policy_selection_follows_the_policy_across_refreshes() {
        let (zones, mut policies) = zoned_fixture();
        let mut screen = FirewallScreen::new();
        screen.apply_action(&Action::FirewallZonesUpdated(Arc::new(zones)));
        screen.apply_action(&Action::FirewallPoliciesUpdated(Arc::new(policies.clone())));
        screen.select(2);
        assert_eq!(policy_names(&screen)[2], "lan-wan second");

        policies.push(zoned_policy(
            "iot-lan allow",
            Some("iot"),
            Some("lan"),
            9_000,
        ));
        screen.apply_action(&Action::FirewallPoliciesUpdated(Arc::new(policies)));

        assert_eq!(screen.policy_table.selected(), Some(3));
        assert_eq!(policy_names(&screen)[3], "lan-wan second");
    }

    #[test]
    fn first_zones_regroup_policies_without_moving_the_cursor() {
        let (zones, policies) = zoned_fixture();
        let mut screen = FirewallScreen::new();
        screen.apply_action(&Action::FirewallPoliciesUpdated(Arc::new(policies)));
        screen.apply_action(&Action::FirewallZonesUpdated(Arc::new(zones)));

        assert_eq!(screen.policy_table.selected(), Some(0));
        assert_eq!(policy_names(&screen)[0], "iot-lan block");
    }

    #[test]
    fn reorder_stays_inside_the_zone_pair() {
        let (zones, policies) = zoned_fixture();
        let mut screen = FirewallScreen::new();
        screen.apply_action(&Action::FirewallZonesUpdated(Arc::new(zones)));
        screen.apply_action(&Action::FirewallPoliciesUpdated(Arc::new(policies)));
        screen.select(1);

        screen.apply_action(&Action::ReorderPolicy(1, Direction::Up));
        assert_eq!(policy_names(&screen)[0], "iot-lan block");
        assert_eq!(screen.policy_table.selected(), Some(1));

        screen.apply_action(&Action::ReorderPolicy(1, Direction::Down));
        assert_eq!(
            policy_names(&screen)[1..3],
            ["lan-wan second", "lan-wan first"]
        );
        assert_eq!(screen.policy_table.selected(), Some(2));
    }
}
