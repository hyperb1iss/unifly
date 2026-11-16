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
use crate::tui::screens::devices::commands::device_name;
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
        grid::note(frame, area, &empty_note(screen, device));
        return;
    }

    let cells: Vec<ClientCells> = clients
        .iter()
        .map(|client| ClientCells::new(screen, device, client))
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
    // A gateway lists the clients of the devices behind it, so each row
    // says which one; a list that is all this device's own needs no column.
    let via_column = cells.iter().any(|c| c.via != DASH).then(|| {
        headers.push("Via");
        columns.push(Column::fit("Via", cells.iter().map(|c| &c.via), 24).shrink_to(8));
        columns.len() - 1
    });
    headers.push("Link");
    columns.push(Column::fit("Link", cells.iter().map(|c| &c.link), 12));
    // A switch's clients are wired: a column of dashes says nothing.
    let show_signal = clients.iter().any(|client| client.wireless.is_some());
    if show_signal {
        headers.push("Signal");
        columns.push(Column::fit("Signal", cells.iter().map(|c| &c.signal), 8));
    }

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
            if let Some(column) = via_column {
                row.push(Cell::from(grid::clipped(&widths, column, cells.via, 0)));
            }
            row.push(Cell::from(cells.link));
            if show_signal {
                row.push(Cell::from(cells.signal).style(Style::default().fg(cells.signal_color)));
            }
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

/// What stands in for the list when it is empty. A device can report
/// clients the list cannot place: before the client list arrives, or where
/// the controller's client records carry no uplink.
fn empty_note(screen: &DevicesScreen, device: &Device) -> String {
    let reported = usize::try_from(device.client_count.unwrap_or(0)).unwrap_or(0);
    if reported == 0 {
        return "No clients connected through this device".to_owned();
    }
    let reported = counted(reported, "client", "clients");
    if screen.clients.is_empty() {
        format!("{reported} reported, the client list has not loaded yet")
    } else {
        format!("{reported} reported, but the client list links none to this device")
    }
}

/// One client's cells as text.
struct ClientCells {
    name: String,
    ip: String,
    mac: String,
    /// The device it attaches to, where that is not the open one.
    via: String,
    /// How it connects: the switch port, or the kind of link.
    link: String,
    signal: String,
    signal_color: Color,
}

impl ClientCells {
    fn new(screen: &DevicesScreen, device: &Device, client: &Client) -> Self {
        let via = screen
            .attach_device(client)
            .filter(|attached| attached.id != device.id)
            .map_or_else(
                || DASH.to_owned(),
                |attached| device_name(attached).to_owned(),
            );
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
                .primary_address()
                .map_or_else(|| DASH.to_owned(), |ip| ip.to_string()),
            mac: client.mac.to_string(),
            via,
            link,
            signal,
            signal_color,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crossterm::event::{KeyCode, KeyEvent};
    use serde_json::json;
    use unifly_api::{Client, Device};

    use crate::tui::action::{Access, Action, DeviceDetailTab};
    use crate::tui::screens::devices::DevicesScreen;
    use crate::tui::screens::devices::fixtures::{client, device, switch, wireless};
    use crate::tui::table::render_lines;

    const GATEWAY_MAC: &str = "aa:bb:cc:dd:ee:01";
    const SWITCH_MAC: &str = "aa:bb:cc:dd:ee:02";
    const AP_MAC: &str = "aa:bb:cc:dd:ee:03";

    /// ap-loft behind sw-core behind gw-harbor, which reports `count`
    /// clients. They sort in that order.
    fn fleet(count: u32) -> Vec<Arc<Device>> {
        let gateway = device(
            1,
            "gw-harbor",
            &json!({
                "model": "UCG Max",
                "device_type": "Gateway",
                "has_access_point": false,
                "client_count": count
            }),
        );
        let core = device_behind(&switch(2, "sw-core", 8), GATEWAY_MAC);
        let loft = device_behind(&device(3, "ap-loft", &json!({})), SWITCH_MAC);
        vec![gateway, core, loft]
    }

    fn device_behind(device: &Arc<Device>, uplink: &str) -> Arc<Device> {
        let mut value = serde_json::to_value(device.as_ref()).expect("device as JSON");
        value["uplink_device_mac"] = json!(uplink);
        Arc::new(serde_json::from_value(value).expect("device fixture"))
    }

    /// The Clients tab of the device on row `row`, as text.
    fn clients_tab(devices: Vec<Arc<Device>>, clients: Vec<Arc<Client>>, row: usize) -> String {
        let mut screen = DevicesScreen::new();
        screen.apply_action(&Action::AccessUpdated(Access {
            integration: true,
            session: true,
            live_events: true,
        }));
        screen.apply_action(&Action::DevicesUpdated(Arc::new(devices)));
        screen.apply_action(&Action::ClientsUpdated(Arc::new(clients)));
        screen.select(row);
        screen.handle_key_input(KeyEvent::from(KeyCode::Enter));
        screen.set_detail_tab(DeviceDetailTab::Clients);
        render_lines(120, 30, |frame, area| screen.render_screen(frame, area)).join("\n")
    }

    fn everyone() -> Vec<Arc<Client>> {
        vec![
            client(
                1,
                "nas-vault",
                &json!({ "uplink_device_mac": SWITCH_MAC, "switch_port": 3 }),
            ),
            client(2, "phone", &wireless(-58, AP_MAC)),
            client(
                3,
                "console",
                &json!({ "uplink_device_mac": GATEWAY_MAC, "switch_port": 4 }),
            ),
        ]
    }

    #[test]
    fn a_gateway_lists_the_clients_of_the_devices_behind_it() {
        let text = clients_tab(fleet(3), everyone(), 1);
        assert!(text.contains("client 1 of 3"), "{text}");
        for name in ["nas-vault", "phone", "console"] {
            assert!(text.contains(name), "{name} is missing:\n{text}");
        }
        // Each row names the device its client attaches to.
        assert!(text.contains("Via"), "{text}");
        let row = |name: &str| {
            text.lines()
                .find(|line| line.contains(name))
                .unwrap_or_default()
                .to_owned()
        };
        assert!(row("nas-vault").contains("sw-core"), "{text}");
        assert!(row("phone").contains("ap-loft"), "{text}");
        assert!(!row("console").contains("gw-harbor"), "{text}");
    }

    #[test]
    fn a_switch_lists_only_its_own_clients() {
        let text = clients_tab(fleet(3), everyone(), 2);
        assert!(text.contains("client 1 of 1"), "{text}");
        assert!(text.contains("nas-vault"), "{text}");
        assert!(!text.contains("phone"), "an access point's client:\n{text}");
        assert!(!text.contains("Via"), "a column of dashes:\n{text}");
    }

    #[test]
    fn a_count_the_list_cannot_place_does_not_blame_loading() {
        let unplaced = vec![client(1, "nas-vault", &json!({}))];
        let text = clients_tab(fleet(41), unplaced, 1);
        assert!(
            text.contains("41 clients reported, but the client list links none to this device"),
            "{text}"
        );

        let text = clients_tab(fleet(41), Vec::new(), 1);
        assert!(
            text.contains("41 clients reported, the client list has not loaded yet"),
            "{text}"
        );
    }

    #[test]
    fn uplinks_that_loop_end_the_walk() {
        let mut devices = fleet(1);
        // sw-core and ap-loft name each other and never reach the gateway.
        devices[1] = device_behind(&devices[1], AP_MAC);
        let text = clients_tab(devices, everyone(), 1);
        assert!(text.contains("client 1 of 1"), "{text}");
        assert!(text.contains("console"), "{text}");
    }
}
