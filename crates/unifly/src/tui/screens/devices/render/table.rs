use std::sync::Arc;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table};

use unifly_api::{Device, DeviceState};

use crate::tui::table::{Column, plan_columns};
use crate::tui::theme;
use crate::tui::widgets::{bytes_fmt, status_indicator};

use super::detail::render_detail;
use crate::tui::screens::devices::DevicesScreen;

pub(super) fn render_screen(screen: &DevicesScreen, frame: &mut Frame, area: Rect) {
    screen.hits.borrow_mut().clear();
    let filtered = screen.filtered_devices();
    let selected_index = screen.selected_row_index(&filtered);
    let total = screen.devices.len();
    let shown = filtered.len();
    let title = if screen.search_query.is_empty() {
        format!(" Devices ({total}) ")
    } else {
        format!(" Devices ({shown}/{total}) ")
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

    let header_layout = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(table_area);

    render_filter_bar(screen, frame, header_layout[0], shown);
    render_device_table(screen, frame, header_layout[1], &filtered, selected_index);
    render_table_hints(frame, header_layout[2]);

    if let Some(detail_area) = detail_area
        && let Some(device) = screen.detail_device()
    {
        render_detail(screen, frame, detail_area, device);
    }
}

fn render_filter_bar(screen: &DevicesScreen, frame: &mut Frame, area: Rect, shown: usize) {
    let filter_text = if screen.search_query.is_empty() {
        Span::styled("[all]", Style::default().fg(theme::accent_secondary()))
    } else {
        Span::styled(
            format!("[\"{}\" ]", screen.search_query),
            Style::default().fg(theme::warning()),
        )
    };
    let mut spans = vec![
        Span::styled(" Filter: ", Style::default().fg(theme::text_secondary())),
        filter_text,
        Span::styled("  Sort: ", Style::default().fg(theme::text_secondary())),
        Span::styled("[name ↑]", Style::default().fg(theme::accent_secondary())),
        Span::styled(
            format!("  {:>width$}", format!("{shown} devices"), width = 20),
            Style::default().fg(theme::text_secondary()),
        ),
    ];
    let updates = screen
        .devices
        .iter()
        .filter(|device| device.firmware_updatable)
        .count();
    if updates > 0 {
        let noun = if updates == 1 { "update" } else { "updates" };
        spans.push(Span::styled(
            format!("  {} {updates} firmware {noun}", UPDATE_MARKER.trim_start()),
            Style::default().fg(theme::warning()),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Table width from which the Devices table adds its Firmware and Clients columns:
/// a 120-column terminal, less the screen border.
const WIDE_TABLE: u16 = 118;

/// Marker after a firmware version that has an update waiting.
const UPDATE_MARKER: &str = " ↑";

/// One device's table cells as text, measured once for column sizing and
/// then styled into a row.
struct DeviceCells {
    name: String,
    model: String,
    ip: String,
    firmware: String,
    update_available: bool,
    clients: String,
    cpu: String,
    mem: String,
    traffic: String,
    uptime: String,
}

impl DeviceCells {
    fn new(device: &Device) -> Self {
        Self {
            name: device.name.as_deref().unwrap_or("Unknown").to_owned(),
            model: device.model.as_deref().unwrap_or("─").to_owned(),
            ip: device.ip.map_or_else(|| "─".into(), |ip| ip.to_string()),
            firmware: device
                .firmware_version
                .clone()
                .unwrap_or_else(|| "─".into()),
            update_available: device.firmware_updatable,
            clients: device
                .client_count
                .map_or_else(|| "─".into(), |count| count.to_string()),
            cpu: device
                .stats
                .cpu_utilization_pct
                .map_or_else(|| "·····".into(), |value| format!("{value:.0}%")),
            mem: device
                .stats
                .memory_utilization_pct
                .map_or_else(|| "·····".into(), |value| format!("{value:.0}%")),
            traffic: device.stats.uplink_bandwidth.as_ref().map_or_else(
                || "···/···".into(),
                |bandwidth| {
                    bytes_fmt::fmt_tx_rx(bandwidth.tx_bytes_per_sec, bandwidth.rx_bytes_per_sec)
                },
            ),
            uptime: device
                .stats
                .uptime_secs
                .map_or_else(|| "···".into(), bytes_fmt::fmt_uptime),
        }
    }

    fn firmware_label(&self) -> String {
        if self.update_available {
            format!("{}{UPDATE_MARKER}", self.firmware)
        } else {
            self.firmware.clone()
        }
    }
}

fn render_device_table(
    screen: &DevicesScreen,
    frame: &mut Frame,
    area: Rect,
    filtered: &[&Arc<Device>],
    selected_index: Option<usize>,
) {
    let wide = area.width >= WIDE_TABLE;
    let cells: Vec<DeviceCells> = filtered
        .iter()
        .map(|device| DeviceCells::new(device))
        .collect();

    let mut headers = vec!["", "Name", "Model", "IP"];
    let mut columns = vec![
        Column::fixed(2).tight(),
        Column::fit("Name", cells.iter().map(|c| &c.name), 32).shrink_to(10),
        Column::fit("Model", cells.iter().map(|c| &c.model), 24).shrink_to(10),
        Column::fit("IP", cells.iter().map(|c| &c.ip), 39).shrink_to(15),
    ];
    if wide {
        headers.extend(["Firmware", "Clients"]);
        columns.extend([
            Column::fit(
                "Firmware",
                cells.iter().map(DeviceCells::firmware_label),
                24,
            ),
            Column::fit("Clients", cells.iter().map(|c| &c.clients), 7),
        ]);
    }
    headers.extend(["CPU", "Mem", "TX/RX", "Uptime"]);
    columns.extend([
        Column::fit("CPU", cells.iter().map(|c| &c.cpu), 5),
        Column::fit("Mem", cells.iter().map(|c| &c.mem), 5),
        Column::fit("TX/RX", cells.iter().map(|c| &c.traffic), 15).at_least(9),
        Column::fit("Uptime", cells.iter().map(|c| &c.uptime), 8).at_least(7),
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
        .map(|(index, (device, cells))| {
            render_table_row(Some(index) == selected_index, device, cells, wide)
        })
        .collect();

    let len = rows.len();
    let table = Table::new(rows, plan_columns(&columns, area.width))
        .header(header)
        .row_highlight_style(theme::table_selected());

    let mut state = screen.table_state;
    frame.render_stateful_widget(table, area, &mut state);
    screen.hits.borrow_mut().push_table_rows(area, &state, len);
}

fn render_table_row(
    is_selected: bool,
    device: &Device,
    cells: DeviceCells,
    wide: bool,
) -> Row<'static> {
    let prefix = if is_selected { "▸" } else { " " };
    let status = status_indicator::status_char(device.state);
    let status_color = match device.state {
        DeviceState::Online => theme::success(),
        DeviceState::Offline | DeviceState::ConnectionInterrupted | DeviceState::Isolated => {
            theme::error()
        }
        DeviceState::PendingAdoption => theme::accent_primary(),
        _ => theme::warning(),
    };

    let name_style = Style::default()
        .fg(theme::accent_secondary())
        .add_modifier(if is_selected {
            Modifier::BOLD
        } else {
            Modifier::empty()
        });

    let mut row = vec![
        Cell::from(format!("{prefix}{status}")).style(Style::default().fg(status_color)),
        Cell::from(cells.name).style(name_style),
        Cell::from(cells.model),
        Cell::from(cells.ip).style(Style::default().fg(theme::accent_tertiary())),
    ];
    if wide {
        let firmware = if cells.update_available {
            Line::from(vec![
                Span::raw(cells.firmware),
                Span::styled(
                    UPDATE_MARKER,
                    Style::default()
                        .fg(theme::warning())
                        .add_modifier(Modifier::BOLD),
                ),
            ])
        } else {
            Line::from(cells.firmware)
        };
        row.extend([Cell::from(firmware), Cell::from(cells.clients)]);
    }
    row.extend([
        Cell::from(cells.cpu),
        Cell::from(cells.mem),
        Cell::from(cells.traffic),
        Cell::from(cells.uptime),
    ]);

    Row::new(row).style(if is_selected {
        theme::table_selected()
    } else {
        theme::table_row()
    })
}

fn render_table_hints(frame: &mut Frame, area: Rect) {
    let hints = Line::from(vec![
        Span::styled("  j/k ", theme::key_hint_key()),
        Span::styled("navigate  ", theme::key_hint()),
        Span::styled("Enter ", theme::key_hint_key()),
        Span::styled("detail  ", theme::key_hint()),
        Span::styled("R ", theme::key_hint_key()),
        Span::styled("restart  ", theme::key_hint()),
        Span::styled("L ", theme::key_hint_key()),
        Span::styled("locate", theme::key_hint()),
    ]);
    frame.render_widget(Paragraph::new(hints), area);
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use unifly_api::Device;

    use crate::tui::action::Action;
    use crate::tui::screens::devices::DevicesScreen;
    use crate::tui::table::render_lines;

    fn device(index: u8, name: &str, model: &str, firmware: &str, updatable: bool) -> Arc<Device> {
        Arc::new(
            serde_json::from_value(json!({
                "id": format!("device-{index}"),
                "mac": format!("aa:bb:cc:dd:ee:{index:02x}"),
                "ip": format!("10.40.0.{index}"),
                "wan_ipv6": null,
                "name": name,
                "model": model,
                "device_type": "AccessPoint",
                "state": "Online",
                "firmware_version": firmware,
                "firmware_updatable": updatable,
                "adopted_at": null,
                "provisioned_at": null,
                "last_seen": null,
                "serial": null,
                "supported": true,
                "ports": [],
                "radios": [],
                "uplink_device_id": null,
                "uplink_device_mac": null,
                "has_switching": false,
                "has_access_point": true,
                "stats": { "uptime_secs": 3_542_400 },
                "client_count": 16,
                "origin": null
            }))
            .expect("device fixture should deserialize"),
        )
    }

    fn screen() -> DevicesScreen {
        let mut screen = DevicesScreen::new();
        screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![
            device(15, "ap-atrium", "U6 Enterprise", "8.1.113", false),
            device(18, "ap-roof", "U7 Outdoor", "8.0.26", true),
            device(2, "sw-core", "USW Pro Max 24 PoE", "7.2.123", false),
        ])));
        screen
    }

    #[test]
    fn clicking_a_detail_tab_switches_to_it_until_the_detail_closes() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        use crate::tui::action::DeviceDetailTab;
        use crate::tui::widgets::click::click_label;

        let mut screen = screen();
        screen.handle_key_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(screen.detail_open);

        for (label, tab) in [
            ("Ports", DeviceDetailTab::Ports),
            ("Radios", DeviceDetailTab::Radios),
            ("Overview", DeviceDetailTab::Overview),
        ] {
            let rows = render_lines(120, 30, |frame, area| screen.render_screen(frame, area));
            let action = screen.handle_mouse_input(click_label(&rows, "Performance", label));
            assert!(
                matches!(action, Some(Action::DeviceDetailTab(clicked)) if clicked == tab),
                "{label}: {action:?}"
            );
            assert_eq!(screen.detail_tab, tab);
        }

        let rows = render_lines(120, 30, |frame, area| screen.render_screen(frame, area));
        let ports = click_label(&rows, "Performance", "Ports");
        screen.handle_key_input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        render_lines(120, 30, |frame, area| screen.render_screen(frame, area));

        assert!(screen.handle_mouse_input(ports).is_none());
        assert_eq!(screen.detail_tab, DeviceDetailTab::Overview);
    }

    #[test]
    fn tapping_rows_selects_then_opens_and_the_detail_follows() {
        use crossterm::event::MouseEventKind;

        use crate::tui::action::DeviceDetailTab;
        use crate::tui::widgets::click::{click_label, mouse};

        let draw = |screen: &DevicesScreen| {
            render_lines(120, 30, |frame, area| screen.render_screen(frame, area))
        };
        let selected = |screen: &DevicesScreen| {
            screen
                .selected_device()
                .and_then(|device| device.name.clone())
        };
        let shown = |screen: &DevicesScreen| {
            screen
                .detail_device()
                .and_then(|device| device.name.clone())
        };
        let mut screen = screen();

        let rows = draw(&screen);
        let action = screen.handle_mouse_input(click_label(&rows, "ap-roof", "ap-roof"));
        assert!(action.is_none());
        assert_eq!(selected(&screen).as_deref(), Some("ap-roof"));
        assert!(!screen.detail_open);

        let rows = draw(&screen);
        let action = screen.handle_mouse_input(click_label(&rows, "ap-roof", "ap-roof"));
        assert!(matches!(action, Some(Action::OpenDeviceDetail(_))));
        assert_eq!(shown(&screen).as_deref(), Some("ap-roof"));

        let rows = draw(&screen);
        screen.handle_mouse_input(click_label(&rows, "Performance", "Ports"));
        let rows = draw(&screen);
        screen.handle_mouse_input(click_label(&rows, "sw-core", "sw-core"));
        assert_eq!(shown(&screen).as_deref(), Some("sw-core"));
        assert_eq!(
            screen.detail_tab,
            DeviceDetailTab::Ports,
            "the tab stays put"
        );

        draw(&screen);
        screen.handle_mouse_input(mouse(MouseEventKind::ScrollUp, 10, 10));
        assert_eq!(
            shown(&screen).as_deref(),
            Some("ap-roof"),
            "the wheel moves it too"
        );
        assert_eq!(screen.detail_tab, DeviceDetailTab::Ports);
    }

    fn render(width: u16) -> String {
        let screen = screen();
        render_lines(width, 12, |frame, area| screen.render_screen(frame, area)).join("\n")
    }

    #[test]
    fn wide_table_shows_whole_values_and_the_firmware_columns() {
        for width in [120, 183] {
            let text = render(width);
            for expected in [
                "U6 Enterprise",
                "USW Pro Max 24 PoE",
                "8.0.26 ↑",
                "Firmware",
                "Clients",
            ] {
                assert!(text.contains(expected), "missing {expected:?}:\n{text}");
            }
            assert!(!text.contains("Sta "), "clipped status header:\n{text}");
        }
    }

    #[test]
    fn narrow_table_keeps_the_core_columns_whole() {
        let text = render(80);
        for expected in [
            "U6 Enterprise",
            "USW Pro Max 24 PoE",
            "10.40.0.15",
            "Uptime",
        ] {
            assert!(text.contains(expected), "missing {expected:?}:\n{text}");
        }
        assert!(
            !text.contains("Firmware"),
            "wide-only column at 80:\n{text}"
        );
    }
}
