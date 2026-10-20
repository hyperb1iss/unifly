use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table};

use unifly_api::Network;

use crate::tui::screens::networks::NetworksScreen;
use crate::tui::table::{Column, plan_columns};
use crate::tui::theme;

use super::detail::render_detail;
use super::edit::render_edit_overlay;

pub(super) fn render_screen(screen: &NetworksScreen, frame: &mut Frame, area: Rect) {
    screen.hits.borrow_mut().clear();
    let count = screen.networks.len();
    let title = format!(" Networks ({count}) ");
    let block = Block::default()
        .title(title)
        .title_style(theme::title_style())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(if screen.focused {
            theme::border_focused()
        } else {
            theme::border_default()
        });

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (table_area, detail_area) = if screen.detail_open {
        let chunks =
            Layout::vertical([Constraint::Percentage(45), Constraint::Percentage(55)]).split(inner);
        (chunks[0], Some(chunks[1]))
    } else {
        (inner, None)
    };

    let layout = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(table_area);

    let selected_idx = screen.selected_index();
    let cells: Vec<NetworkCells> = screen
        .networks
        .iter()
        .map(|n| NetworkCells::new(n))
        .collect();
    let columns = [
        Column::fit("Name", cells.iter().map(|c| &c.name), 32)
            .pad(1)
            .shrink_to(9),
        Column::fit("VLAN", cells.iter().map(|c| &c.vlan), 6),
        Column::fit("Gateway", cells.iter().map(|c| &c.gateway), 39).shrink_to(15),
        Column::fit("Subnet", cells.iter().map(|c| &c.subnet), 43).shrink_to(18),
        Column::fit("DHCP", cells.iter().map(|c| c.dhcp), 4),
        Column::fit("Type", cells.iter().map(|c| &c.management), 12),
        Column::fit("IPv6", cells.iter().map(|c| &c.ipv6), 18),
    ];
    let header = Row::new(
        ["Name", "VLAN", "Gateway", "Subnet", "DHCP", "Type", "IPv6"]
            .into_iter()
            .map(|label| Cell::from(label).style(theme::table_header())),
    );
    let rows: Vec<Row> = cells
        .into_iter()
        .enumerate()
        .map(|(index, cells)| render_table_row(index == selected_idx, cells))
        .collect();

    let len = rows.len();
    let table = Table::new(rows, plan_columns(&columns, layout[0].width))
        .header(header)
        .row_highlight_style(theme::table_selected());

    let mut state = screen.table_state;
    frame.render_stateful_widget(table, layout[0], &mut state);
    screen
        .hits
        .borrow_mut()
        .push_table_rows(layout[0], &state, len);

    let hints = Line::from(vec![
        Span::styled("  j/k ", theme::key_hint_key()),
        Span::styled("navigate  ", theme::key_hint()),
        Span::styled("Enter ", theme::key_hint_key()),
        Span::styled("expand  ", theme::key_hint()),
        Span::styled("e ", theme::key_hint_key()),
        Span::styled("edit  ", theme::key_hint()),
        Span::styled("Esc ", theme::key_hint_key()),
        Span::styled("collapse", theme::key_hint()),
    ]);
    frame.render_widget(Paragraph::new(hints), layout[1]);

    if let Some(detail_area) = detail_area
        && let Some(network) = screen.networks.get(selected_idx)
    {
        render_detail(frame, detail_area, network);
    }

    if let Some(edit) = screen.edit_state.as_ref() {
        render_edit_overlay(frame, area, edit);
    }
}

/// One network's table cells as text, measured once for column sizing and
/// then styled into a row.
struct NetworkCells {
    name: String,
    vlan: String,
    gateway: String,
    subnet: String,
    dhcp: &'static str,
    management: String,
    ipv6: String,
}

impl NetworkCells {
    fn new(network: &Network) -> Self {
        Self {
            name: network.name.clone(),
            vlan: network
                .vlan_id
                .map_or_else(|| "─".into(), |value| value.to_string()),
            gateway: network
                .gateway_ip
                .map_or_else(|| "─".into(), |ip| ip.to_string()),
            subnet: network.subnet.clone().unwrap_or_else(|| "─".into()),
            dhcp: network
                .dhcp
                .as_ref()
                .map_or("─", |dhcp| if dhcp.enabled { "On" } else { "Off" }),
            management: network
                .management
                .as_ref()
                .map_or_else(|| "─".into(), |value| format!("{value:?}")),
            ipv6: if network.ipv6_enabled {
                network
                    .ipv6_mode
                    .as_ref()
                    .map_or_else(|| "On".into(), ToString::to_string)
            } else {
                "Off".into()
            },
        }
    }
}

fn render_table_row(is_selected: bool, cells: NetworkCells) -> Row<'static> {
    let prefix = if is_selected { "▸" } else { " " };
    let name_style = Style::default()
        .fg(theme::accent_secondary())
        .add_modifier(if is_selected {
            Modifier::BOLD
        } else {
            Modifier::empty()
        });

    Row::new(vec![
        Cell::from(format!("{prefix}{}", cells.name)).style(name_style),
        Cell::from(cells.vlan),
        Cell::from(cells.gateway).style(Style::default().fg(theme::accent_tertiary())),
        Cell::from(cells.subnet).style(Style::default().fg(theme::accent_tertiary())),
        Cell::from(cells.dhcp),
        Cell::from(cells.management),
        Cell::from(cells.ipv6),
    ])
    .style(if is_selected {
        theme::table_selected()
    } else {
        theme::table_row()
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use unifly_api::Network;

    use crate::tui::action::Action;
    use crate::tui::screens::networks::NetworksScreen;
    use crate::tui::table::render_lines;

    fn network(name: &str, vlan: u16, ipv6_mode: Option<&str>) -> Arc<Network> {
        Arc::new(
            serde_json::from_value(json!({
                "id": format!("net-{vlan}"), "name": name, "vlan_id": vlan, "enabled": true,
                "management": "Gateway", "purpose": "Corporate", "is_default": false,
                "subnet": format!("10.40.{vlan}.0/24"), "gateway_ip": format!("10.40.{vlan}.1"),
                "isolation_enabled": false, "internet_access_enabled": true,
                "mdns_forwarding_enabled": false, "ipv6_enabled": ipv6_mode.is_some(),
                "ipv6_mode": ipv6_mode, "ipv6_prefix": null, "dhcpv6_enabled": false,
                "slaac_enabled": false, "ntp_server": null, "pxe_enabled": false,
                "tftp_server": null, "firewall_zone_id": null,
                "cellular_backup_enabled": false, "origin": null, "dhcp": null
            }))
            .expect("network fixture should deserialize"),
        )
    }

    #[test]
    fn ipv6_mode_and_subnets_render_whole_at_80_columns() {
        let mut screen = NetworksScreen::new();
        screen.apply_action(&Action::NetworksUpdated(Arc::new(vec![
            network("Studio Floor", 10, Some("PrefixDelegation")),
            network("IoT", 140, None),
        ])));

        let text = render_lines(80, 8, |frame, area| screen.render_screen(frame, area)).join("\n");
        for expected in [
            "Studio Floor",
            "10.40.140.0/24",
            "10.40.140.1",
            "Prefix Delegation",
        ] {
            assert!(text.contains(expected), "missing {expected:?}:\n{text}");
        }
    }
}
