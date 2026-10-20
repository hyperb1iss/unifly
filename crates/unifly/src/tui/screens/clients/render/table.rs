use std::sync::Arc;

use super::super::ClientsScreen;
use super::super::labels::LinkIndex;
use super::detail::render_detail;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table};

use unifly_api::{Client, ClientType};

use crate::tui::table::{Column, plan_columns};
use crate::tui::theme;
use crate::tui::widgets::{bytes_fmt, sub_tabs};

pub(super) fn render_screen(screen: &ClientsScreen, frame: &mut Frame, area: Rect) {
    screen.hits.borrow_mut().clear();
    let filtered = screen.filtered_clients();
    let total = screen.clients.len();
    let shown = filtered.len();
    let title = if screen.search_query.is_empty() {
        format!(" Clients ({shown}/{total}) ")
    } else {
        format!(" Clients ({shown}/{total}) [\"{}\" ] ", screen.search_query)
    };

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
            Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).split(inner);
        (chunks[0], Some(chunks[1]))
    } else {
        (inner, None)
    };

    let layout = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(table_area);

    let filter_labels = &["All", "Wireless", "Wired", "VPN", "Guest"];
    let strip = sub_tabs::sub_tab_strip(filter_labels, screen.filter_index());
    let line = strip.draw(layout[0], &mut screen.hits.borrow_mut());
    frame.render_widget(Paragraph::new(line), layout[0]);

    let selected_idx = if screen.detail_open {
        screen
            .detail_client_index(&filtered)
            .unwrap_or_else(|| screen.selected_index())
    } else {
        screen.selected_index()
    };
    render_client_table(screen, frame, layout[1], &filtered, selected_idx);

    let hints = Line::from(vec![
        Span::styled("  j/k ", theme::key_hint_key()),
        Span::styled("navigate  ", theme::key_hint()),
        Span::styled("Enter ", theme::key_hint_key()),
        Span::styled("detail  ", theme::key_hint()),
        Span::styled("Tab ", theme::key_hint_key()),
        Span::styled("filter  ", theme::key_hint()),
        Span::styled("b ", theme::key_hint_key()),
        Span::styled("block  ", theme::key_hint()),
        Span::styled("x ", theme::key_hint_key()),
        Span::styled("kick", theme::key_hint()),
    ]);
    frame.render_widget(Paragraph::new(hints), layout[2]);

    if let Some(detail_area) = detail_area
        && let Some(client) = screen.detail_client(&filtered)
    {
        render_detail(frame, detail_area, client);
    }
}

/// Table width from which the Clients table adds its Network and Uplink columns:
/// a 120-column terminal, less the screen border.
const WIDE_TABLE: u16 = 118;

/// Table width below which the MAC column steps aside so client names
/// keep their room: a 90-column terminal, less the screen border.
const MAC_TABLE: u16 = 88;

/// One client's table cells as text, measured once for column sizing and
/// then styled into a row. Cells of columns the layout hides stay `None`
/// and are never computed.
struct ClientCells {
    name: String,
    ip: String,
    mac: Option<String>,
    network: Option<String>,
    uplink: Option<String>,
    traffic: String,
    duration: String,
}

impl ClientCells {
    fn new(links: &LinkIndex, client: &Client, layout: ClientColumns) -> Self {
        let dash = || "─".to_owned();
        Self {
            name: client
                .name
                .as_deref()
                .or(client.hostname.as_deref())
                .unwrap_or("unknown")
                .to_owned(),
            ip: client
                .ip
                .map_or_else(|| "─".into(), |client_ip| client_ip.to_string()),
            mac: layout.mac.then(|| client.mac.to_string()),
            network: layout
                .wide
                .then(|| links.network_label(client).unwrap_or_else(dash)),
            uplink: layout
                .wide
                .then(|| links.uplink_label(client).unwrap_or_else(dash)),
            traffic: bytes_fmt::fmt_tx_rx(
                client.tx_bytes.unwrap_or(0),
                client.rx_bytes.unwrap_or(0),
            ),
            duration: client.connected_at.map_or_else(
                || "─".into(),
                |ts| {
                    let dur = chrono::Utc::now().signed_duration_since(ts);
                    let secs = u64::try_from(dur.num_seconds().max(0)).unwrap_or_default();
                    bytes_fmt::fmt_uptime(secs)
                },
            ),
        }
    }
}

fn render_client_table(
    screen: &ClientsScreen,
    frame: &mut Frame,
    area: Rect,
    filtered: &[&Arc<Client>],
    selected_idx: usize,
) {
    let layout = ClientColumns {
        mac: area.width >= MAC_TABLE,
        wide: area.width >= WIDE_TABLE,
    };
    let cells: Vec<ClientCells> = filtered
        .iter()
        .map(|client| ClientCells::new(&screen.links, client, layout))
        .collect();

    let mut headers = vec!["Type", "Name", "IP"];
    let mut columns = vec![
        Column::fixed(4).tight(),
        Column::fit("Name", cells.iter().map(|c| &c.name), 32).shrink_to(10),
        Column::fit("IP", cells.iter().map(|c| &c.ip), 39).shrink_to(13),
    ];
    if layout.mac {
        headers.push("MAC");
        columns.push(Column::fixed(17));
    }
    if layout.wide {
        headers.extend(["Network", "Uplink"]);
        columns.extend([
            Column::fit(
                "Network",
                cells.iter().filter_map(|c| c.network.as_ref()),
                20,
            )
            .shrink_to(8),
            Column::fit("Uplink", cells.iter().filter_map(|c| c.uplink.as_ref()), 32).shrink_to(10),
        ]);
    }
    headers.extend(["Signal", "TX/RX", "Duration"]);
    columns.extend([
        Column::fixed(6),
        Column::fit("TX/RX", cells.iter().map(|c| &c.traffic), 15).at_least(11),
        Column::fit("Duration", cells.iter().map(|c| &c.duration), 8),
    ]);

    let header = Row::new(
        headers
            .into_iter()
            .map(|label| Cell::from(label).style(theme::table_header())),
    );
    let rows: Vec<Row> = filtered
        .iter()
        .zip(cells)
        .enumerate()
        .map(|(index, (client, cells))| render_table_row(index == selected_idx, client, cells))
        .collect();

    let table = Table::new(rows, plan_columns(&columns, area.width))
        .header(header)
        .row_highlight_style(theme::table_selected());

    let mut state = screen.table_state;
    frame.render_stateful_widget(table, area, &mut state);
}

/// Optional columns the Clients table has room for.
#[derive(Clone, Copy)]
struct ClientColumns {
    mac: bool,
    wide: bool,
}

fn render_table_row(is_selected: bool, client: &Client, cells: ClientCells) -> Row<'static> {
    let prefix = if is_selected { "▸" } else { " " };

    let type_char = match client.client_type {
        ClientType::Wireless => "W",
        ClientType::Wired => "E",
        ClientType::Vpn => "V",
        ClientType::Teleport => "T",
        _ => "?",
    };
    let type_str = if client.is_guest {
        format!("{prefix}G")
    } else {
        format!("{prefix}{type_char}")
    };
    let type_color = if client.is_guest {
        theme::warning()
    } else {
        match client.client_type {
            ClientType::Wireless => theme::accent_secondary(),
            ClientType::Vpn => theme::accent_primary(),
            ClientType::Teleport => theme::accent_tertiary(),
            _ => theme::text_secondary(),
        }
    };

    let signal_dbm = client
        .wireless
        .as_ref()
        .and_then(|wireless| wireless.signal_dbm);
    let (signal, signal_color) = signal_dbm.map_or(("····", theme::border_unfocused()), |dbm| {
        if dbm >= -50 {
            ("▂▄▆█", theme::success())
        } else if dbm >= -60 {
            ("▂▄▆ ", theme::accent_secondary())
        } else if dbm >= -70 {
            ("▂▄  ", theme::warning())
        } else if dbm >= -80 {
            ("▂   ", theme::accent_tertiary())
        } else {
            ("·   ", theme::error())
        }
    });

    let name_style = Style::default()
        .fg(theme::accent_secondary())
        .add_modifier(if is_selected {
            Modifier::BOLD
        } else {
            Modifier::empty()
        });

    let mut row = vec![
        Cell::from(type_str).style(Style::default().fg(type_color)),
        Cell::from(cells.name).style(name_style),
        Cell::from(cells.ip).style(Style::default().fg(theme::accent_tertiary())),
    ];
    if let Some(mac) = cells.mac {
        row.push(Cell::from(mac));
    }
    if let (Some(network), Some(uplink)) = (cells.network, cells.uplink) {
        row.extend([
            Cell::from(network),
            Cell::from(uplink).style(Style::default().fg(theme::text_secondary())),
        ]);
    }
    row.extend([
        Cell::from(signal).style(Style::default().fg(signal_color)),
        Cell::from(cells.traffic),
        Cell::from(cells.duration),
    ]);

    Row::new(row).style(if is_selected {
        theme::table_selected()
    } else {
        theme::table_row()
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use unifly_api::{Client, Device, Network};

    use super::{ClientCells, ClientColumns};
    use crate::tui::action::Action;
    use crate::tui::screens::clients::ClientsScreen;
    use crate::tui::table::render_lines;

    fn screen() -> ClientsScreen {
        let switch: Device = serde_json::from_value(json!({
            "id": "sw-core", "mac": "9e:b8:1f:d8:fc:f2", "ip": null, "wan_ipv6": null,
            "name": "sw-core", "model": null, "device_type": "Switch", "state": "Online",
            "firmware_version": null, "firmware_updatable": false, "adopted_at": null,
            "provisioned_at": null, "last_seen": null, "serial": null, "supported": true,
            "ports": [], "radios": [], "uplink_device_id": null, "uplink_device_mac": null,
            "has_switching": true, "has_access_point": false, "stats": {},
            "client_count": null, "origin": null
        }))
        .expect("device fixture should deserialize");
        let network: Network = serde_json::from_value(json!({
            "id": "11111111-1111-4111-8111-111111111111", "name": "Render", "vlan_id": 20,
            "enabled": true, "management": "Gateway", "purpose": "Corporate",
            "is_default": false, "subnet": null, "gateway_ip": null,
            "isolation_enabled": false, "internet_access_enabled": true,
            "mdns_forwarding_enabled": false, "ipv6_enabled": false, "ipv6_mode": null,
            "ipv6_prefix": null, "dhcpv6_enabled": false, "slaac_enabled": false,
            "ntp_server": null, "pxe_enabled": false, "tftp_server": null,
            "firewall_zone_id": null, "cellular_backup_enabled": false, "origin": null,
            "dhcp": null
        }))
        .expect("network fixture should deserialize");
        let client: Client = serde_json::from_value(json!({
            "id": "client-1", "mac": "82:8b:02:66:7c:85", "ip": "10.40.20.40",
            "name": "dailies-review-station", "hostname": null, "client_type": "Wired",
            "connected_at": null, "uplink_device_id": null,
            "uplink_device_mac": "9e:b8:1f:d8:fc:f2", "switch_port": 14,
            "network_id": "11111111-1111-4111-8111-111111111111", "vlan": 20,
            "wireless": null, "guest_auth": null, "is_guest": false,
            "tx_bytes": 151_400_000_000_u64, "rx_bytes": 352_000_000_000_u64,
            "bandwidth": null, "os_name": null, "device_class": null,
            "use_fixedip": false, "fixed_ip": null, "blocked": false
        }))
        .expect("client fixture should deserialize");

        let mut screen = ClientsScreen::new();
        screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![Arc::new(switch)])));
        screen.apply_action(&Action::NetworksUpdated(Arc::new(vec![Arc::new(network)])));
        screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![Arc::new(client)])));
        screen
    }

    fn render(width: u16) -> String {
        let screen = screen();
        render_lines(width, 10, |frame, area| screen.render_screen(frame, area)).join("\n")
    }

    #[test]
    fn wide_table_adds_network_and_uplink_and_keeps_values_whole() {
        for width in [120, 183] {
            let text = render(width);
            for expected in [
                "Type ",
                "dailies-review-station",
                "151.4G/352.0G",
                "82:8b:02:66:7c:85",
                "Render",
                "sw-core port 14",
            ] {
                assert!(text.contains(expected), "missing {expected:?}:\n{text}");
            }
        }
    }

    #[test]
    fn hidden_columns_are_never_resolved() {
        let screen = screen();
        let client = screen.clients.first().expect("fixture client");

        let narrow = ClientColumns {
            mac: false,
            wide: false,
        };
        let cells = ClientCells::new(&screen.links, client, narrow);
        assert_eq!(cells.mac, None);
        assert_eq!(cells.network, None);
        assert_eq!(cells.uplink, None);

        let wide = ClientColumns {
            mac: true,
            wide: true,
        };
        let cells = ClientCells::new(&screen.links, client, wide);
        assert_eq!(cells.mac.as_deref(), Some("82:8b:02:66:7c:85"));
        assert_eq!(cells.network.as_deref(), Some("Render"));
        assert_eq!(cells.uplink.as_deref(), Some("sw-core port 14"));
    }

    #[test]
    fn narrow_table_favours_names_over_the_mac() {
        let text = render(80);
        for expected in ["dailies-review-station", "10.40.20.40", "151.4G/352.0G"] {
            assert!(text.contains(expected), "missing {expected:?}:\n{text}");
        }
        assert!(
            !text.contains("82:8b:02:66:7c:85"),
            "MAC at 80 columns:\n{text}"
        );
        assert!(!text.contains("Uplink"), "wide-only column at 80:\n{text}");
    }
}
