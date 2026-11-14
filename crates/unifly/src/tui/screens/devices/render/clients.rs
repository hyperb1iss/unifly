//! The Clients tab: who is connected through the device, with a cursor
//! that reaches every one of them.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Cell, Row, Table};

use unifly_api::model::ClientType;
use unifly_api::{Client, Device};

use super::grid::{self, DASH};
use super::ports::register_rows;
use crate::tui::screens::devices::DevicesScreen;
use crate::tui::screens::devices::detail::Detail;
use crate::tui::screens::devices::view::counted;
use crate::tui::table::{Column, plan_columns};
use crate::tui::theme;

/// Width from which the list has room for a MAC column.
const MAC_WIDTH: u16 = 88;

pub(super) fn render(
    screen: &DevicesScreen,
    frame: &mut Frame,
    area: Rect,
    device: &Device,
    detail: &Detail,
) {
    let clients = screen.device_clients(device);
    if clients.is_empty() {
        let reported = device.client_count.unwrap_or(0);
        let note = if reported > 0 {
            format!(
                "{} reported, their details not loaded yet",
                counted(usize::try_from(reported).unwrap_or(0), "client", "clients")
            )
        } else {
            "No clients connected through this device".to_owned()
        };
        grid::note(frame, area, &note);
        return;
    }

    let cells: Vec<ClientCells> = clients
        .iter()
        .map(|client| ClientCells::new(client))
        .collect();
    let show_mac = area.width >= MAC_WIDTH;
    let mut headers = vec!["", "Name", "IP"];
    let mut columns = vec![
        Column::fixed(1).tight(),
        Column::fit("Name", cells.iter().map(|c| &c.name), 32).shrink_to(10),
        Column::fit("IP", cells.iter().map(|c| &c.ip), 39).shrink_to(15),
    ];
    if show_mac {
        headers.push("MAC");
        columns.push(Column::fixed(17));
    }
    headers.extend(["Link", "Signal"]);
    columns.extend([
        Column::fit("Link", cells.iter().map(|c| &c.link), 12),
        Column::fit("Signal", cells.iter().map(|c| &c.signal), 8),
    ]);

    let widths = plan_columns(&columns, area.width);
    let selected = detail.cursor_index();
    let rows: Vec<Row> = cells
        .into_iter()
        .enumerate()
        .map(|(index, cells)| {
            let is_selected = index == selected;
            let mut row = vec![
                Cell::from(if is_selected { "▸" } else { " " }),
                Cell::from(grid::clipped(&widths, 1, cells.name, 0))
                    .style(Style::default().fg(theme::accent_secondary())),
                Cell::from(cells.ip).style(Style::default().fg(theme::accent_tertiary())),
            ];
            if show_mac {
                row.push(Cell::from(cells.mac));
            }
            row.extend([
                Cell::from(cells.link),
                Cell::from(cells.signal).style(Style::default().fg(cells.signal_color)),
            ]);
            Row::new(row).style(if is_selected {
                theme::table_selected()
            } else {
                theme::table_row()
            })
        })
        .collect();

    let header = Row::new(
        headers
            .into_iter()
            .map(|label| Cell::from(label).style(theme::table_header())),
    );
    let len = rows.len();
    let table = Table::new(rows, widths).header(header);
    let drawn = detail
        .scroll
        .render(frame, table, area, &detail.cursor, len);
    register_rows(screen, area, drawn.offset(), len);
}

/// One client's cells as text.
struct ClientCells {
    name: String,
    ip: String,
    mac: String,
    /// How it connects: the switch port, or the kind of link.
    link: String,
    signal: String,
    signal_color: Color,
}

impl ClientCells {
    fn new(client: &Client) -> Self {
        let link = match (client.client_type, client.switch_port) {
            (ClientType::Wired, Some(port)) => format!("port {port}"),
            (ClientType::Wired, None) => "wired".to_owned(),
            (ClientType::Wireless, _) => "WiFi".to_owned(),
            (ClientType::Vpn, _) => "VPN".to_owned(),
            (ClientType::Teleport, _) => "Teleport".to_owned(),
            _ => DASH.to_owned(),
        };
        let dbm = client
            .wireless
            .as_ref()
            .and_then(|wireless| wireless.signal_dbm);
        let (signal, signal_color) = dbm.map_or_else(
            || (DASH.to_owned(), theme::text_muted()),
            |dbm| {
                let color = if dbm > -50 {
                    theme::success()
                } else if dbm > -70 {
                    theme::accent_secondary()
                } else if dbm > -80 {
                    theme::warning()
                } else {
                    theme::error()
                };
                (format!("{dbm} dBm"), color)
            },
        );
        Self {
            name: client
                .name
                .as_deref()
                .or(client.hostname.as_deref())
                .unwrap_or("unknown")
                .to_owned(),
            ip: client
                .ip
                .map_or_else(|| DASH.to_owned(), |ip| ip.to_string()),
            mac: client.mac.to_string(),
            link,
            signal,
            signal_color,
        }
    }
}
