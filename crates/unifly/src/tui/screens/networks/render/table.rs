use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Row, Table};

use unifly_api::Network;

use crate::tui::screens::networks::NetworksScreen;
use crate::tui::table::{Column, plan_columns};
use crate::tui::theme;
use crate::tui::widgets::click::{self, Hint};
use crate::tui::widgets::detail_grid::{NONE, render_empty, render_panel, split_detail};

use super::detail::{detail_grid, dhcp_label, management_label};
use super::edit::render_form;

/// Rows the table keeps when the detail panel opens: its header and three
/// networks.
const MIN_TABLE_ROWS: u16 = 4;

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

    if screen.networks.is_empty() {
        render_empty(frame, inner, empty_reason(screen));
    } else {
        let detail = screen
            .selected_network()
            .filter(|_| screen.detail_open)
            .map(|network| {
                let lines =
                    detail_grid(network, &screen.zones).lines(inner.width.saturating_sub(2));
                (network.name.clone(), lines)
            });
        let (table_area, panel) = match &detail {
            Some((_, lines)) => split_detail(inner, lines.len(), MIN_TABLE_ROWS),
            None => (inner, Rect::default()),
        };
        render_table(screen, frame, table_area);
        if let Some((name, lines)) = detail {
            render_panel(frame, panel, &name, lines);
        }
    }

    if let Some(form) = screen.form.as_ref() {
        render_form(frame, area, form, &mut screen.hits.borrow_mut());
    }
}

/// Why the table has nothing in it.
fn empty_reason(screen: &NetworksScreen) -> &'static str {
    match screen.access {
        None => "Waiting for the controller\u{2026}",
        Some(access) if !access.integration => {
            "Networks come from the Integration API, which this profile cannot reach. \
             Add an API key to it to see them."
        }
        Some(_) => "No networks on this site. Press c to create one.",
    }
}

/// What the status bar offers: the actions that work on the selected
/// network. Nothing acts on a network when there is none, creating one
/// needs the Integration API, and the default network cannot be deleted.
/// The form has the keyboard while it is open and shows its own keys.
pub(in crate::tui::screens::networks) fn status_hints(screen: &NetworksScreen) -> Vec<(u8, Hint)> {
    if screen.form.is_some() {
        return Vec::new();
    }
    let create = (2, Hint::button("c", "new", click::char_key('c')));
    let Some(network) = screen.selected_network() else {
        let can_create = screen.access.is_some_and(|access| access.integration);
        return if can_create { vec![create] } else { Vec::new() };
    };
    let mut hints = Vec::new();
    if !screen.detail_open {
        hints.push((0, Hint::button("Enter", "detail", KeyCode::Enter)));
    }
    hints.push((1, Hint::button("e", "edit", click::char_key('e'))));
    hints.push(create);
    if !network.is_default {
        hints.push((3, Hint::button("D", "delete", click::char_key('D'))));
    }
    if screen.detail_open {
        hints.push((0, Hint::button("Esc", "close", KeyCode::Esc)));
    }
    hints
}

fn render_table(screen: &NetworksScreen, frame: &mut Frame, area: Rect) {
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
        Column::fit("Type", cells.iter().map(|c| c.management), 12),
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
    let table = Table::new(rows, plan_columns(&columns, area.width))
        .header(header)
        .row_highlight_style(theme::table_selected());

    let state = screen
        .table_scroll
        .render(frame, table, area, &screen.table_state, len);
    screen.hits.borrow_mut().push_table_rows(area, &state, len);
}

/// One network's table cells as text, measured once for column sizing and
/// then styled into a row.
struct NetworkCells {
    name: String,
    enabled: bool,
    vlan: String,
    gateway: String,
    subnet: String,
    dhcp: &'static str,
    management: &'static str,
    ipv6: String,
}

impl NetworkCells {
    fn new(network: &Network) -> Self {
        Self {
            name: network.name.clone(),
            enabled: network.enabled,
            vlan: network
                .vlan_id
                .map_or_else(|| NONE.into(), |value| value.to_string()),
            gateway: network
                .gateway_ip
                .map_or_else(|| NONE.into(), |ip| ip.to_string()),
            subnet: network.subnet.clone().unwrap_or_else(|| NONE.into()),
            dhcp: dhcp_label(network),
            management: management_label(network.management),
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
    // A network that is switched off carries no traffic, so its row steps
    // back.
    let color = |color| {
        if cells.enabled {
            color
        } else {
            theme::text_muted()
        }
    };
    let name_style = Style::default()
        .fg(color(theme::accent_secondary()))
        .add_modifier(if is_selected {
            Modifier::BOLD
        } else {
            Modifier::empty()
        });
    let address = Style::default().fg(color(theme::accent_tertiary()));
    let row_style = if is_selected {
        theme::table_selected()
    } else if cells.enabled {
        theme::table_row()
    } else {
        Style::default().fg(theme::text_muted())
    };

    Row::new(vec![
        Cell::from(format!("{prefix}{}", cells.name)).style(name_style),
        Cell::from(cells.vlan),
        Cell::from(cells.gateway).style(address),
        Cell::from(cells.subnet).style(address),
        Cell::from(cells.dhcp),
        Cell::from(cells.management),
        Cell::from(cells.ipv6),
    ])
    .style(row_style)
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
