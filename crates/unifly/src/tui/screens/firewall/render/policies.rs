//! The Policies and Zones tables.

use std::collections::HashMap;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Cell, Row, Table};
use unifly_api::EntityId;
use unifly_api::model::{FirewallAction, FirewallZone};

use super::{enabled_cell, index_label, name_style, row_style};
use crate::tui::screens::firewall::FirewallScreen;
use crate::tui::table::{Column, plan_columns};
use crate::tui::theme;

/// Table width from which the Policies table shows each rule's zone pair
/// (a 96-column terminal, less the screen border). Narrower, the pair
/// would be cut to a stub, so the column steps aside.
const ZONES_TABLE: u16 = 94;

/// One policy's table cells as text, measured once for column sizing and
/// then styled into a row.
struct PolicyCells {
    index: String,
    enabled: bool,
    zones: String,
    /// Whether this row opens a new zone pair; later rows of the same pair
    /// draw their zones dimmed so the groups stand out.
    group_start: bool,
    name: String,
    action: FirewallAction,
    protocol: String,
    source: String,
    destination: String,
}

impl FirewallScreen {
    pub(super) fn render_policies(&self, frame: &mut Frame, area: Rect) {
        let selected_idx = self.policy_table.selected().unwrap_or(0);
        let zone_names = self.zone_names();
        let zone =
            |id: Option<&EntityId>| id.and_then(|id| zone_names.get(id).copied()).unwrap_or("─");

        let mut previous_pair = None;
        let cells: Vec<PolicyCells> = self
            .policies
            .iter()
            .enumerate()
            .map(|(index, policy)| {
                let pair = (
                    policy.source.zone_id.as_ref(),
                    policy.destination.zone_id.as_ref(),
                );
                let group_start = previous_pair != Some(pair);
                previous_pair = Some(pair);
                PolicyCells {
                    index: index_label(index == selected_idx, policy.index.map(|i| i.to_string())),
                    enabled: policy.enabled,
                    zones: format!("{} → {}", zone(pair.0), zone(pair.1)),
                    group_start,
                    name: policy.name.clone(),
                    action: policy.action,
                    protocol: policy
                        .protocol_summary
                        .clone()
                        .unwrap_or_else(|| "Any".into()),
                    source: policy.source_summary.clone().unwrap_or_else(|| "─".into()),
                    destination: policy
                        .destination_summary
                        .clone()
                        .unwrap_or_else(|| "─".into()),
                }
            })
            .collect();

        let show_zones = area.width >= ZONES_TABLE;
        let mut headers = vec!["#", "On"];
        let mut columns = vec![
            Column::fit("#", cells.iter().map(|c| &c.index), 12).tight(),
            Column::fixed(2).tight(),
        ];
        if show_zones {
            headers.push("Zones");
            columns.push(Column::fit("Zones", cells.iter().map(|c| &c.zones), 32).shrink_to(12));
        }
        headers.extend(["Name", "Action", "Protocol", "Source", "Destination"]);
        columns.extend([
            Column::fit("Name", cells.iter().map(|c| &c.name), 40).shrink_to(12),
            Column::fit("Action", cells.iter().map(|c| action_label(c.action)), 6),
            Column::fit("Protocol", cells.iter().map(|c| &c.protocol), 12),
            Column::fit("Source", cells.iter().map(|c| &c.source), 32).shrink_to(8),
            Column::fit("Destination", cells.iter().map(|c| &c.destination), 32).shrink_to(11),
        ]);
        let header = Row::new(
            headers
                .into_iter()
                .map(|label| Cell::from(label).style(theme::table_header())),
        );

        let rows: Vec<Row> = cells
            .into_iter()
            .enumerate()
            .map(|(index, cells)| policy_row(index == selected_idx, cells, show_zones))
            .collect();

        let len = rows.len();
        let table = Table::new(rows, plan_columns(&columns, area.width))
            .header(header)
            .row_highlight_style(theme::table_selected());

        let mut state = self.policy_table;
        frame.render_stateful_widget(table, area, &mut state);
        self.hits.borrow_mut().push_table_rows(area, &state, len);
    }

    pub(super) fn render_zones(&self, frame: &mut Frame, area: Rect) {
        let selected_idx = self.zone_table.selected().unwrap_or(0);
        let network_names: HashMap<&EntityId, &str> = self
            .networks
            .iter()
            .map(|network| (&network.id, network.name.as_str()))
            .collect();

        let cells: Vec<[String; 3]> = self
            .zones
            .iter()
            .enumerate()
            .map(|(index, zone)| {
                let prefix = if index == selected_idx { "▸" } else { " " };
                [
                    format!("{prefix}{}", zone.name),
                    self.zone_policy_count(zone).to_string(),
                    zone_networks_label(zone, &network_names),
                ]
            })
            .collect();

        let columns = [
            Column::fit("Name", cells.iter().map(|c| &c[0]), 32).shrink_to(10),
            Column::fit("Policies", cells.iter().map(|c| &c[1]), 8),
            Column::fit("Networks", cells.iter().map(|c| &c[2]), 80).shrink_to(10),
        ];
        let header = Row::new(
            ["Name", "Policies", "Networks"]
                .into_iter()
                .map(|label| Cell::from(label).style(theme::table_header())),
        );

        let rows: Vec<Row> = cells
            .into_iter()
            .enumerate()
            .map(|(index, [name, policies, networks])| {
                let is_selected = index == selected_idx;
                Row::new(vec![
                    Cell::from(name).style(name_style(is_selected)),
                    Cell::from(policies),
                    Cell::from(networks).style(Style::default().fg(theme::text_secondary())),
                ])
                .style(row_style(is_selected))
            })
            .collect();

        let len = rows.len();
        let table = Table::new(rows, plan_columns(&columns, area.width))
            .header(header)
            .row_highlight_style(theme::table_selected());

        let mut state = self.zone_table;
        frame.render_stateful_widget(table, area, &mut state);
        self.hits.borrow_mut().push_table_rows(area, &state, len);
    }

    fn zone_names(&self) -> HashMap<&EntityId, &str> {
        self.zones
            .iter()
            .map(|zone| (&zone.id, zone.name.as_str()))
            .collect()
    }

    /// Policies whose source or destination is `zone`.
    fn zone_policy_count(&self, zone: &FirewallZone) -> usize {
        self.policies
            .iter()
            .filter(|policy| {
                policy.source.zone_id.as_ref() == Some(&zone.id)
                    || policy.destination.zone_id.as_ref() == Some(&zone.id)
            })
            .count()
    }
}

/// The zone's member networks by name; a count when some are unknown.
fn zone_networks_label(zone: &FirewallZone, names: &HashMap<&EntityId, &str>) -> String {
    let resolved: Vec<&str> = zone
        .network_ids
        .iter()
        .filter_map(|id| names.get(id).copied())
        .collect();
    match zone.network_ids.len() {
        0 => "─".into(),
        count if resolved.len() < count => {
            format!("{count} network{}", if count == 1 { "" } else { "s" })
        }
        _ => resolved.join(", "),
    }
}

fn action_label(action: FirewallAction) -> &'static str {
    match action {
        FirewallAction::Allow => "Allow",
        FirewallAction::Block => "Block",
        FirewallAction::Reject => "Reject",
    }
}

fn policy_row(is_selected: bool, cells: PolicyCells, show_zones: bool) -> Row<'static> {
    let action_color = match cells.action {
        FirewallAction::Allow => theme::success(),
        FirewallAction::Block => theme::error(),
        FirewallAction::Reject => theme::accent_tertiary(),
    };
    let zones_color = if cells.group_start {
        theme::text_secondary()
    } else {
        theme::text_muted()
    };

    let mut row = vec![Cell::from(cells.index), enabled_cell(cells.enabled)];
    if show_zones {
        row.push(Cell::from(cells.zones).style(Style::default().fg(zones_color)));
    }
    row.extend([
        Cell::from(cells.name).style(name_style(is_selected)),
        Cell::from(action_label(cells.action)).style(Style::default().fg(action_color)),
        Cell::from(cells.protocol),
        Cell::from(cells.source),
        Cell::from(cells.destination),
    ]);
    Row::new(row).style(row_style(is_selected))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use serde_json::json;
    use unifly_api::EntityId;
    use unifly_api::model::{FirewallPolicy, FirewallZone};

    use super::zone_networks_label;
    use crate::tui::action::Action;
    use crate::tui::screens::firewall::FirewallScreen;
    use crate::tui::table::render_lines;

    fn zone(id: &str, name: &str, networks: &[&str]) -> Arc<FirewallZone> {
        Arc::new(
            serde_json::from_value(json!({
                "id": id, "name": name, "network_ids": networks, "origin": null
            }))
            .expect("zone fixture should deserialize"),
        )
    }

    fn policy(name: &str, source: &str, destination: &str, index: i32) -> Arc<FirewallPolicy> {
        Arc::new(
            serde_json::from_value(json!({
                "id": name, "name": name, "description": null, "enabled": true,
                "index": index, "action": "Block", "ip_version": "Both",
                "source": { "zone_id": source, "filter": null },
                "destination": { "zone_id": destination, "filter": null },
                "source_summary": null, "destination_summary": null,
                "protocol_summary": null, "schedule": null, "ipsec_mode": null,
                "connection_states": [], "logging_enabled": false, "origin": null
            }))
            .expect("policy fixture should deserialize"),
        )
    }

    fn screen() -> FirewallScreen {
        let mut screen = FirewallScreen::new();
        screen.apply_action(&Action::FirewallZonesUpdated(Arc::new(vec![
            zone("ext", "External", &[]),
            zone("gw", "Gateway", &[]),
        ])));
        screen.apply_action(&Action::FirewallPoliciesUpdated(Arc::new(vec![
            policy("Drop inbound SSH from outside", "ext", "gw", 10_000),
            policy("Allow WireGuard handshakes", "ext", "gw", 10_001),
        ])));
        screen
    }

    fn render(width: u16) -> String {
        let screen = screen();
        render_lines(width, 8, |frame, area| screen.render_screen(frame, area)).join("\n")
    }

    #[test]
    fn policies_show_their_zone_pair_when_there_is_room() {
        for width in [96, 120] {
            let text = render(width);
            for expected in ["External → Gateway", "Drop inbound SSH from outside"] {
                assert!(text.contains(expected), "missing {expected:?}:\n{text}");
            }
        }
    }

    #[test]
    fn narrow_policies_drop_the_zone_pair_and_keep_names() {
        let text = render(80);
        assert!(
            !text.contains("External → Gateway"),
            "zone pair at 80 columns:\n{text}"
        );
        assert!(
            text.contains("Drop inbound SSH from outside"),
            "name cut at 80 columns:\n{text}"
        );
    }

    #[test]
    fn zone_networks_list_names_or_fall_back_to_a_count() {
        let studio = EntityId::Legacy("studio".into());
        let render_net = EntityId::Legacy("render".into());
        let names: HashMap<&EntityId, &str> =
            HashMap::from([(&studio, "Studio"), (&render_net, "Render")]);

        let known = zone("int", "Internal", &["studio", "render"]);
        assert_eq!(zone_networks_label(&known, &names), "Studio, Render");

        let partly_known = zone("iot", "IoT", &["studio", "cameras"]);
        assert_eq!(zone_networks_label(&partly_known, &names), "2 networks");

        let empty = zone("ext", "External", &[]);
        assert_eq!(zone_networks_label(&empty, &names), "─");
    }
}
