//! The Policies and Zones tables.

use std::collections::HashMap;
use std::sync::Arc;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table};
use unifly_api::EntityId;
use unifly_api::model::{FirewallAction, FirewallPolicy, FirewallZone};

use super::{enabled_cell, header_row, name_style, position_label, row_style};
use crate::tui::screens::firewall::FirewallScreen;
use crate::tui::screens::firewall::reorder::{Stage, is_fixed};
use crate::tui::table::{Column, plan_columns};
use crate::tui::theme;
use crate::tui::widgets::click::Click;
use crate::tui::widgets::detail_grid::NONE;

/// The mark on a policy the controller keeps in place: it cannot be moved
/// or deleted.
pub(super) const FIXED_MARK: &str = "\u{25A3}";

/// One row of the Policies table as drawn.
enum Shown {
    /// The zone pair the policies under it run between.
    Pair(String),
    /// A policy, by its row in the table's data.
    Policy(usize),
}

/// One policy's table cells as text, measured once for column sizing and
/// then styled into a row.
struct PolicyCells {
    position: String,
    enabled: bool,
    fixed: bool,
    /// Set on the policy being moved, with whether the move is still only
    /// on screen.
    moving: Option<bool>,
    name: String,
    action: FirewallAction,
    protocol: String,
    source: String,
    destination: String,
}

impl FirewallScreen {
    /// The Policies table's rows under their zone pairs, each policy
    /// numbered by its place in its pair.
    fn policy_rows(&self, view: &[Arc<FirewallPolicy>]) -> (Vec<Shown>, Vec<PolicyCells>) {
        let selected_idx = self.policy_table.selected().unwrap_or(0);
        let zone_names = self.zone_names();
        let zone = |id: Option<&EntityId>| {
            id.and_then(|id| zone_names.get(id).copied())
                .unwrap_or(NONE)
        };

        let mut shown = Vec::new();
        let mut cells = Vec::new();
        let mut previous_pair = None;
        let mut position = 0;
        for (index, policy) in view.iter().enumerate() {
            let pair = (
                policy.source.zone_id.as_ref(),
                policy.destination.zone_id.as_ref(),
            );
            if previous_pair != Some(pair) {
                shown.push(Shown::Pair(format!(
                    "{} \u{2192} {}",
                    zone(pair.0),
                    zone(pair.1)
                )));
                previous_pair = Some(pair);
                position = 0;
            }
            position += 1;
            shown.push(Shown::Policy(index));
            let moving = self
                .reorder
                .as_ref()
                .filter(|reorder| reorder.policy.id == policy.id)
                .map(|reorder| reorder.stage == Stage::Draft);
            cells.push(PolicyCells {
                position: if moving.is_some() {
                    format!("\u{2195}{position}")
                } else {
                    position_label(index == selected_idx, position)
                },
                enabled: policy.enabled,
                fixed: is_fixed(policy),
                moving,
                name: policy.name.clone(),
                action: policy.action,
                protocol: policy
                    .protocol_summary
                    .clone()
                    .unwrap_or_else(|| "Any".into()),
                source: policy.source_summary.clone().unwrap_or_else(|| NONE.into()),
                destination: policy
                    .destination_summary
                    .clone()
                    .unwrap_or_else(|| NONE.into()),
            });
        }
        (shown, cells)
    }

    pub(super) fn render_policies(&self, frame: &mut Frame, area: Rect) {
        let selected_idx = self.policy_table.selected().unwrap_or(0);
        let view = self.policy_view();
        let (shown, cells) = self.policy_rows(&view);

        let columns = [
            Column::fit("#", cells.iter().map(|c| &c.position), 6).tight(),
            Column::fixed(2).tight(),
            Column::fixed(1).tight(),
            Column::fit("Name", cells.iter().map(|c| &c.name), 40).shrink_to(12),
            Column::fit("Action", cells.iter().map(|c| action_label(c.action)), 6),
            Column::fit("Protocol", cells.iter().map(|c| &c.protocol), 12),
            Column::fit("Source", cells.iter().map(|c| &c.source), 32).shrink_to(8),
            Column::fit("Destination", cells.iter().map(|c| &c.destination), 32).shrink_to(11),
        ];
        let widths = plan_columns(&columns, area.width);
        let header = header_row(&[
            "#",
            "On",
            "",
            "Name",
            "Action",
            "Protocol",
            "Source",
            "Destination",
        ]);
        let row = |y: u16| Rect {
            y,
            height: 1,
            ..area
        };
        frame.render_widget(Table::new([header], widths.clone()), row(area.y));

        // The table scrolls by hand: a zone-pair row is not a policy, so
        // the cursor never rests on one, and the pair above the selected
        // policy stays in view with it.
        let body = usize::from(area.height.saturating_sub(1));
        let selected_row = shown
            .iter()
            .position(|row| matches!(row, Shown::Policy(index) if *index == selected_idx))
            .unwrap_or(0);
        let lead = usize::from(
            selected_row > 0 && matches!(shown.get(selected_row - 1), Some(Shown::Pair(_))),
        );
        let mut offset = self
            .policy_offset
            .get()
            .min(shown.len().saturating_sub(body));
        offset = offset.min(selected_row - lead);
        if body > 0 && selected_row >= offset + body {
            offset = selected_row + 1 - body;
        }
        self.policy_offset.set(offset);

        let mut cells = cells.into_iter().map(Some).collect::<Vec<_>>();
        let rows = (area.y.saturating_add(1)..area.bottom()).zip(shown.iter().skip(offset));
        for (y, shown) in rows {
            match shown {
                Shown::Pair(label) => {
                    frame.render_widget(Paragraph::new(pair_line(label, area.width)), row(y));
                }
                Shown::Policy(index) => {
                    let Some(cells) = cells.get_mut(*index).and_then(Option::take) else {
                        continue;
                    };
                    let line = policy_row(*index == selected_idx, cells);
                    frame.render_widget(Table::new([line], widths.clone()), row(y));
                    self.hits.borrow_mut().push(row(y), Click::Row(*index));
                }
            }
        }
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
        let header = header_row(&["Name", "Policies", "Networks"]);

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

        let state = self
            .zone_scroll
            .render(frame, table, area, &self.zone_table, len);
        self.hits.borrow_mut().push_table_rows(area, &state, len);
    }

    fn zone_names(&self) -> HashMap<&EntityId, &str> {
        self.zones
            .iter()
            .map(|zone| (&zone.id, zone.name.as_str()))
            .collect()
    }

    /// The names of a policy's source and destination zones.
    pub(super) fn zone_pair_label(&self, policy: &FirewallPolicy) -> String {
        let names = self.zone_names();
        let zone = |id: Option<&EntityId>| id.and_then(|id| names.get(id).copied()).unwrap_or(NONE);
        format!(
            "{} \u{2192} {}",
            zone(policy.source.zone_id.as_ref()),
            zone(policy.destination.zone_id.as_ref())
        )
    }

    /// Policies whose source or destination is `zone`: `(from, to)`.
    pub(super) fn zone_policy_counts(&self, zone: &FirewallZone) -> (usize, usize) {
        let count = |from: bool| {
            self.policies
                .iter()
                .filter(|policy| {
                    let endpoint = if from {
                        &policy.source
                    } else {
                        &policy.destination
                    };
                    endpoint.zone_id.as_ref() == Some(&zone.id)
                })
                .count()
        };
        (count(true), count(false))
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

/// The row that names a zone pair, ruled out to the table's width so the
/// groups read as groups.
fn pair_line(label: &str, width: u16) -> Line<'static> {
    let lead = format!("\u{2500}\u{2500} {label} ");
    let rule = usize::from(width).saturating_sub(Span::raw(lead.as_str()).width());
    Line::from(vec![
        Span::styled(
            lead,
            Style::default()
                .fg(theme::text_secondary())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "\u{2500}".repeat(rule),
            Style::default().fg(theme::border_unfocused()),
        ),
    ])
}

/// The zone's member networks by name; a count when some are unknown.
pub(super) fn zone_networks_label(zone: &FirewallZone, names: &HashMap<&EntityId, &str>) -> String {
    let resolved: Vec<&str> = zone
        .network_ids
        .iter()
        .filter_map(|id| names.get(id).copied())
        .collect();
    match zone.network_ids.len() {
        0 => NONE.into(),
        count if resolved.len() < count => {
            format!("{count} network{}", if count == 1 { "" } else { "s" })
        }
        _ => resolved.join(", "),
    }
}

pub(super) fn action_label(action: FirewallAction) -> &'static str {
    match action {
        FirewallAction::Allow => "Allow",
        FirewallAction::Block => "Block",
        FirewallAction::Reject => "Reject",
    }
}

pub(super) fn action_color(action: FirewallAction) -> ratatui::style::Color {
    match action {
        FirewallAction::Allow => theme::success(),
        FirewallAction::Block => theme::error(),
        FirewallAction::Reject => theme::accent_tertiary(),
    }
}

fn policy_row(is_selected: bool, cells: PolicyCells) -> Row<'static> {
    let muted = Style::default().fg(theme::text_muted());
    // A policy picked up to move stands out; once sent it steps back
    // until the controller has it.
    let style = match cells.moving {
        Some(true) => Style::default()
            .fg(theme::warning())
            .add_modifier(Modifier::BOLD),
        Some(false) => muted,
        None => row_style(is_selected),
    };
    let name = if cells.moving.is_some() {
        style
    } else {
        name_style(is_selected)
    };
    let action = if cells.moving == Some(false) {
        muted
    } else {
        Style::default().fg(action_color(cells.action))
    };
    Row::new(vec![
        Cell::from(cells.position),
        enabled_cell(cells.enabled),
        Cell::from(if cells.fixed { FIXED_MARK } else { "" }).style(muted),
        Cell::from(cells.name).style(name),
        Cell::from(action_label(cells.action)).style(action),
        Cell::from(cells.protocol),
        Cell::from(cells.source),
        Cell::from(cells.destination),
    ])
    .style(style)
}

#[cfg(test)]
mod tests;
