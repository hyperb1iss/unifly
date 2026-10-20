//! The ACL Rules and NAT tables.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Cell, Row, Table};
use unifly_api::model::{AclAction, AclRuleType, NatPolicy};

use super::{enabled_cell, index_label, name_style, row_style};
use crate::tui::screens::firewall::FirewallScreen;
use crate::tui::table::{Column, plan_columns};
use crate::tui::theme;

impl FirewallScreen {
    pub(super) fn render_acl_rules(&self, frame: &mut Frame, area: Rect) {
        let selected_idx = self.acl_table.selected().unwrap_or(0);
        let cells: Vec<[String; 6]> = self
            .acl_rules
            .iter()
            .enumerate()
            .map(|(index, rule)| {
                [
                    index_label(index == selected_idx, rule.index.map(|i| i.to_string())),
                    rule.name.clone(),
                    acl_type_label(rule.rule_type).to_owned(),
                    format!("{:?}", rule.action),
                    rule.source_summary.clone().unwrap_or_else(|| "─".into()),
                    rule.destination_summary
                        .clone()
                        .unwrap_or_else(|| "─".into()),
                ]
            })
            .collect();

        let columns = [
            Column::fit("#", cells.iter().map(|c| &c[0]), 12).tight(),
            Column::fixed(2).tight(),
            Column::fit("Name", cells.iter().map(|c| &c[1]), 40).shrink_to(12),
            Column::fit("Type", cells.iter().map(|c| &c[2]), 6),
            Column::fit("Action", cells.iter().map(|c| &c[3]), 6),
            Column::fit("Source", cells.iter().map(|c| &c[4]), 32).shrink_to(8),
            Column::fit("Destination", cells.iter().map(|c| &c[5]), 32).shrink_to(11),
        ];
        let header = header_row(&["#", "On", "Name", "Type", "Action", "Source", "Destination"]);

        let rows: Vec<Row> = self
            .acl_rules
            .iter()
            .zip(cells)
            .enumerate()
            .map(
                |(index, (rule, [number, name, kind, action, source, destination]))| {
                    let is_selected = index == selected_idx;
                    let action_color = match rule.action {
                        AclAction::Allow => theme::success(),
                        AclAction::Block => theme::error(),
                    };
                    Row::new(vec![
                        Cell::from(number),
                        enabled_cell(rule.enabled),
                        Cell::from(name).style(name_style(is_selected)),
                        Cell::from(kind),
                        Cell::from(action).style(Style::default().fg(action_color)),
                        Cell::from(source),
                        Cell::from(destination),
                    ])
                    .style(row_style(is_selected))
                },
            )
            .collect();

        let len = rows.len();
        let table = Table::new(rows, plan_columns(&columns, area.width))
            .header(header)
            .row_highlight_style(theme::table_selected());

        let state = self
            .acl_scroll
            .render(frame, table, area, &self.acl_table, len);
        self.hits.borrow_mut().push_table_rows(area, &state, len);
    }

    pub(super) fn render_nat_policies(&self, frame: &mut Frame, area: Rect) {
        let selected_idx = self.nat_table.selected().unwrap_or(0);
        let cells: Vec<[String; 6]> = self
            .nat_policies
            .iter()
            .enumerate()
            .map(|(index, policy)| {
                [
                    index_label(
                        index == selected_idx,
                        policy.rule_index.map(|i| i.to_string()),
                    ),
                    policy.name.clone(),
                    format!("{:?}", policy.nat_type),
                    policy.protocol.clone().unwrap_or_else(|| "─".into()),
                    nat_endpoint(policy.dst_address.as_ref(), policy.dst_port.as_ref()),
                    nat_endpoint(
                        policy.translated_address.as_ref(),
                        policy.translated_port.as_ref(),
                    ),
                ]
            })
            .collect();

        let columns = [
            Column::fit("#", cells.iter().map(|c| &c[0]), 12).tight(),
            Column::fixed(2).tight(),
            Column::fit("Name", cells.iter().map(|c| &c[1]), 40).shrink_to(12),
            Column::fit("Type", cells.iter().map(|c| &c[2]), 12),
            Column::fit("Protocol", cells.iter().map(|c| &c[3]), 10),
            Column::fit("Destination", cells.iter().map(|c| &c[4]), 48).shrink_to(11),
            Column::fit("Translation", cells.iter().map(|c| &c[5]), 48).shrink_to(11),
        ];
        let header = header_row(&[
            "#",
            "On",
            "Name",
            "Type",
            "Protocol",
            "Destination",
            "Translation",
        ]);

        let rows: Vec<Row> = self
            .nat_policies
            .iter()
            .zip(cells)
            .enumerate()
            .map(|(index, (policy, cells))| nat_row(index == selected_idx, policy, cells))
            .collect();

        let len = rows.len();
        let table = Table::new(rows, plan_columns(&columns, area.width))
            .header(header)
            .row_highlight_style(theme::table_selected());

        let state = self
            .nat_scroll
            .render(frame, table, area, &self.nat_table, len);
        self.hits.borrow_mut().push_table_rows(area, &state, len);
    }
}

fn nat_row(is_selected: bool, policy: &NatPolicy, cells: [String; 6]) -> Row<'static> {
    let [number, name, kind, protocol, destination, translation] = cells;
    Row::new(vec![
        Cell::from(number),
        enabled_cell(policy.enabled),
        Cell::from(name).style(name_style(is_selected)),
        Cell::from(kind),
        Cell::from(protocol),
        Cell::from(destination),
        Cell::from(translation).style(Style::default().fg(theme::accent_tertiary())),
    ])
    .style(row_style(is_selected))
}

fn header_row(labels: &[&'static str]) -> Row<'static> {
    Row::new(
        labels
            .iter()
            .map(|label| Cell::from(*label).style(theme::table_header())),
    )
}

/// `address:port` for a NAT match or translation, `*` standing in for a
/// missing address.
fn nat_endpoint(address: Option<&String>, port: Option<&String>) -> String {
    match (address, port) {
        (Some(address), Some(port)) => format!("{address}:{port}"),
        (Some(address), None) => address.clone(),
        (None, Some(port)) => format!("*:{port}"),
        (None, None) => "─".into(),
    }
}

fn acl_type_label(rule_type: AclRuleType) -> &'static str {
    match rule_type {
        AclRuleType::Ipv4 => "IPv4",
        AclRuleType::Mac => "MAC",
    }
}
