use std::sync::Arc;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Row, Table};

use unifly_api::{Device, DeviceState};

use crossterm::event::KeyCode;

use crate::tui::table::{Column, plan_columns};
use crate::tui::theme;
use crate::tui::widgets::click::{self, Click, Hint};
use crate::tui::widgets::{bytes_fmt, status_indicator, sub_tabs, toolbar};

use super::detail::{detail_height, detail_hints, render_detail};
use super::grid;
use crate::tui::screens::devices::DevicesScreen;
use crate::tui::screens::devices::commands::{is_gateway, is_pending, is_reachable};
use crate::tui::screens::devices::view::DeviceFilter;

pub(super) fn render_screen(screen: &DevicesScreen, frame: &mut Frame, area: Rect) {
    screen.hits.borrow_mut().clear();
    screen.detail_hits.borrow_mut().clear();
    let filtered = screen.filtered_devices();
    let selected_index = screen.selected_row_index(&filtered);

    let block = Block::default()
        .title(title(screen, filtered.len()))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(if screen.focused {
            theme::border_focused()
        } else {
            theme::border_default()
        });

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // An open detail takes the rows its tab needs, and sits right under
    // the table where the table is shorter than its share, so the tab
    // strip stays on one row as the tabs change. The table keeps at least
    // a third of the screen; on a very short one the pane takes all of it.
    let detail_device = screen.detail_device();
    let (table_area, detail_area) = if let Some(device) = detail_device {
        let table = table_rows(filtered.len());
        let room = inner
            .height
            .saturating_sub(grid::list_rows_beside_detail(inner.height).min(table));
        let pane = detail_height(screen, device, inner.width, room).min(room);
        let list = (inner.height - pane).min(table);
        // A filter bar and a header with no device under them say nothing.
        let list = if list < MIN_TABLE_ROWS { 0 } else { list };
        let table_area = Rect {
            height: list,
            ..inner
        };
        let detail_area = Rect {
            y: inner.y + list,
            height: pane,
            ..inner
        };
        (table_area, Some(detail_area))
    } else {
        (inner, None)
    };

    let layout = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(table_area);

    render_filter_bar(screen, frame, layout[0]);
    if filtered.is_empty() {
        grid::note(frame, layout[1], &empty_note(screen));
    } else {
        render_device_table(screen, frame, layout[1], &filtered, selected_index);
    }

    if let (Some(detail_area), Some(device)) = (detail_area, detail_device) {
        render_detail(screen, frame, detail_area, device);
    }
}

/// Fewest rows worth giving the table beside an open detail: the filter
/// bar, the header, and one device.
const MIN_TABLE_ROWS: u16 = 3;

/// Rows the table takes to show `devices` of them: the filter bar, the
/// header, and a row each.
fn table_rows(devices: usize) -> u16 {
    u16::try_from(devices).unwrap_or(u16::MAX).saturating_add(2)
}

/// What the status bar offers: the way into the selected device's detail
/// and the commands it can take, or the open detail's own.
pub(in crate::tui::screens::devices) fn status_hints(screen: &DevicesScreen) -> Vec<(u8, Hint)> {
    if let Some(detail) = &screen.detail {
        return match screen.detail_device() {
            Some(device) => detail_hints(screen, device, detail),
            None => vec![(0, Hint::button("Esc", "close", KeyCode::Esc))],
        };
    }
    let Some(device) = screen.selected_device() else {
        return Vec::new();
    };
    let mut hints = vec![(0, Hint::button("Enter", "detail", KeyCode::Enter))];
    hints.extend(command_hints(screen, device));
    hints
}

/// The screen title: how many devices show, of how many, and the search
/// query narrowing them.
fn title(screen: &DevicesScreen, shown: usize) -> Line<'static> {
    let total = screen.devices.len();
    let count = if shown == total {
        format!(" Devices ({total}) ")
    } else {
        format!(" Devices ({shown}/{total}) ")
    };
    let mut spans = vec![Span::styled(count, theme::title_style())];
    if !screen.search_query.is_empty() {
        spans.push(Span::styled(
            format!("[\"{}\"] ", screen.search_query),
            Style::default().fg(theme::warning()),
        ));
    }
    Line::from(spans)
}

/// What the table area says when it has no rows: still waiting for the
/// controller, nothing on the site, or nothing passing the filter.
fn empty_note(screen: &DevicesScreen) -> String {
    if screen.access.is_none() && screen.devices.is_empty() {
        return "Waiting for the controller\u{2026}".into();
    }
    if !screen.search_query.is_empty() {
        return format!(
            "No device matches \"{}\" (/ then Esc clears the search)",
            screen.search_query
        );
    }
    if screen.devices.is_empty() {
        return DeviceFilter::All.empty_note().into();
    }
    screen.filter.empty_note().into()
}

/// The filter strip, with the sort against the right edge. The firmware
/// updates waiting are counted on the filter that lists them.
fn render_filter_bar(screen: &DevicesScreen, frame: &mut Frame, area: Rect) {
    let mut filters = sub_tabs::sub_tab_strip(&DeviceFilter::LABELS, screen.filter.index());
    let updates = screen
        .devices
        .iter()
        .filter(|device| device.firmware_updatable)
        .count();
    if updates > 0 {
        // Updates closes the strip, so its count follows the strip's last
        // cell and is part of the same tab.
        let tab = Click::Tab(DeviceFilter::Updates.index());
        filters.push(
            tab.clone(),
            Span::styled(
                updates.to_string(),
                Style::default()
                    .fg(theme::warning())
                    .add_modifier(Modifier::BOLD),
            ),
        );
        filters.push(tab, Span::raw(" "));
    }
    // `s` belongs to the table, so the chip is only a button while no
    // detail has the keys.
    let key = screen.detail.is_none().then(|| ("s", click::char_key('s')));
    let sort = toolbar::chip(key, "sort", screen.sort.label());
    toolbar::render(frame, area, filters, sort, &mut screen.hits.borrow_mut());
}

/// Table width from which the Devices table adds its Firmware and Clients columns:
/// a 120-column terminal, less the screen border.
const WIDE_TABLE: u16 = 118;

/// Marks a device with a firmware update waiting.
pub(super) const UPDATE_MARKER: &str = "\u{2191}";

/// Header of the uplink rate column: bits per second, sent then received.
const RATE_HEADER: &str = "TX/RX bps";

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
                .map_or_else(|| "─".into(), |value| format!("{value:.0}%")),
            mem: device
                .stats
                .memory_utilization_pct
                .map_or_else(|| "─".into(), |value| format!("{value:.0}%")),
            traffic: device.stats.uplink_bandwidth.as_ref().map_or_else(
                || "─".into(),
                |bandwidth| {
                    format!(
                        "{}/{}",
                        bytes_fmt::fmt_rate_axis(rate(bandwidth.tx_bytes_per_sec)),
                        bytes_fmt::fmt_rate_axis(rate(bandwidth.rx_bytes_per_sec))
                    )
                },
            ),
            uptime: device
                .stats
                .uptime_secs
                .map_or_else(|| "─".into(), bytes_fmt::fmt_uptime),
        }
    }

    /// A value with the update marker after it when one is waiting.
    fn marked(&self, value: &str) -> String {
        if self.update_available {
            format!("{value} {UPDATE_MARKER}")
        } else {
            value.to_owned()
        }
    }
}

/// A byte rate as the float the rate formatter takes.
#[allow(clippy::cast_precision_loss, clippy::as_conversions)]
fn rate(bytes_per_sec: u64) -> f64 {
    bytes_per_sec as f64
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

    // The update marker rides on the firmware version, and on the name
    // where the table is too narrow for a Firmware column.
    let names = cells.iter().map(|c| {
        if wide {
            c.name.clone()
        } else {
            c.marked(&c.name)
        }
    });
    let mut headers = vec!["", "Name", "Model", "IP"];
    let mut columns = vec![
        Column::fixed(2).tight(),
        Column::fit("Name", names, 32).shrink_to(10),
        Column::fit("Model", cells.iter().map(|c| &c.model), 24).shrink_to(10),
        Column::fit("IP", cells.iter().map(|c| &c.ip), 39).shrink_to(15),
    ];
    if wide {
        headers.extend(["Firmware", "Clients"]);
        columns.extend([
            Column::fit("Firmware", cells.iter().map(|c| c.marked(&c.firmware)), 24),
            Column::fit("Clients", cells.iter().map(|c| &c.clients), 7),
        ]);
    }
    headers.extend(["CPU", "Mem", RATE_HEADER, "Uptime"]);
    columns.extend([
        Column::fit("CPU", cells.iter().map(|c| &c.cpu), 5),
        Column::fit("Mem", cells.iter().map(|c| &c.mem), 5),
        Column::fit(RATE_HEADER, cells.iter().map(|c| &c.traffic), 15).at_least(9),
        Column::fit("Uptime", cells.iter().map(|c| &c.uptime), 8).at_least(7),
    ]);

    let header = Row::new(
        headers
            .into_iter()
            .map(|label| Cell::from(label).style(theme::table_header())),
    );
    let widths = plan_columns(&columns, area.width);
    let rows: Vec<Row> = filtered
        .iter()
        .zip(cells)
        .enumerate()
        .map(|(index, (device, mut cells))| {
            // The marker takes two of the name's cells where it rides there.
            let marker = usize::from(cells.update_available && !wide) * 2;
            cells.name = grid::clipped(&widths, 1, cells.name, marker);
            cells.model = grid::clipped(&widths, 2, cells.model, 0);
            render_table_row(Some(index) == selected_index, device, cells, wide)
        })
        .collect();

    let len = rows.len();
    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(theme::table_selected());

    let state = screen.table_state.with_selected(selected_index);
    let state = screen.table_scroll.render(frame, table, area, &state, len);
    screen.hits.borrow_mut().push_table_rows(area, &state, len);
}

/// A cell's text with the update marker after it in the warning color.
fn with_marker(text: String, marked: bool) -> Line<'static> {
    if !marked {
        return Line::from(text);
    }
    Line::from(vec![
        Span::raw(text),
        Span::styled(
            format!(" {UPDATE_MARKER}"),
            Style::default()
                .fg(theme::warning())
                .add_modifier(Modifier::BOLD),
        ),
    ])
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

    let update = cells.update_available;
    let mut row = vec![
        Cell::from(format!("{prefix}{status}")).style(Style::default().fg(status_color)),
        Cell::from(with_marker(cells.name, update && !wide)).style(name_style),
        Cell::from(cells.model),
        Cell::from(cells.ip).style(Style::default().fg(theme::accent_tertiary())),
    ];
    if wide {
        row.extend([
            Cell::from(with_marker(cells.firmware, update)),
            Cell::from(cells.clients),
        ]);
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

/// Hints for the device commands `device` can take in its state and under
/// the controller's access, ranked for a narrow bar.
pub(super) fn command_hints(screen: &DevicesScreen, device: &Device) -> Vec<(u8, Hint)> {
    if is_pending(device) {
        return vec![(1, Hint::button("A", "adopt", click::char_key('A')))];
    }
    if !is_reachable(device) {
        return Vec::new();
    }
    let session = screen.has_session();
    let mut hints = vec![
        (1, Hint::button("R", "restart", click::char_key('R'))),
        (4, Hint::button("L", "locate", click::char_key('L'))),
    ];
    if session && device.firmware_updatable {
        hints.push((1, Hint::button("U", "upgrade", click::char_key('U'))));
    }
    if session {
        hints.push((6, Hint::button("P", "provision", click::char_key('P'))));
    }
    if session && is_gateway(device) {
        hints.push((5, Hint::button("T", "speed test", click::char_key('T'))));
    }
    hints
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crossterm::event::{KeyCode, KeyEvent, MouseEventKind};
    use serde_json::json;
    use unifly_api::Device;

    use crate::tui::action::{Access, Action, DeviceDetailTab};
    use crate::tui::screens::devices::DevicesScreen;
    use crate::tui::screens::devices::fixtures::{client, device, switch, wireless};
    use crate::tui::table::render_lines;
    use crate::tui::widgets::click::{self, click_label, mouse};

    use super::status_hints;

    fn ap(index: u8, name: &str, model: &str, firmware: &str, updatable: bool) -> Arc<Device> {
        device(
            index,
            name,
            &json!({
                "model": model,
                "firmware_version": firmware,
                "firmware_updatable": updatable,
                "client_count": 16
            }),
        )
    }

    fn screen_of(devices: Vec<Arc<Device>>) -> DevicesScreen {
        let mut screen = DevicesScreen::new();
        screen.apply_action(&Action::AccessUpdated(Access {
            integration: true,
            session: true,
            live_events: true,
        }));
        screen.apply_action(&Action::DevicesUpdated(Arc::new(devices)));
        screen
    }

    /// ap-atrium, ap-roof (update waiting), and the 24-port sw-core.
    fn screen() -> DevicesScreen {
        screen_of(vec![
            ap(15, "ap-atrium", "U6 Enterprise", "8.1.113", false),
            ap(18, "ap-roof", "U7 Outdoor", "8.0.26", true),
            switch(2, "sw-core", 24),
        ])
    }

    fn draw(screen: &DevicesScreen, width: u16, height: u16) -> Vec<String> {
        render_lines(width, height, |frame, area| {
            screen.render_screen(frame, area);
        })
    }

    fn press(screen: &mut DevicesScreen, code: KeyCode) -> Option<Action> {
        screen.handle_key_input(KeyEvent::from(code))
    }

    fn tab(screen: &DevicesScreen) -> Option<DeviceDetailTab> {
        screen.detail.as_ref().map(|detail| detail.tab)
    }

    fn shown(screen: &DevicesScreen) -> Option<String> {
        screen
            .detail_device()
            .and_then(|device| device.name.clone())
    }

    /// What the status bar offers for the screen as it stands.
    fn hints(screen: &DevicesScreen) -> String {
        click::hint_text(&status_hints(screen))
    }

    /// The row the detail's tab strip is drawn on.
    fn tab_row(rows: &[String]) -> usize {
        rows.iter()
            .position(|row| row.contains(" Overview "))
            .expect("the tab strip is drawn")
    }

    /// Rows of the device table on screen, by the cursor and status column.
    fn device_rows(rows: &[String]) -> usize {
        rows.iter()
            .filter(|row| {
                row.starts_with("\u{2502} \u{25cf}") || row.starts_with("\u{2502}\u{25b8}\u{25cf}")
            })
            .count()
    }

    #[test]
    fn clicking_a_detail_tab_switches_to_it_until_the_detail_closes() {
        let mut screen = screen();
        screen.select(2);
        press(&mut screen, KeyCode::Enter);
        assert!(screen.detail.is_some());

        for (label, expected) in [
            ("Ports", DeviceDetailTab::Ports),
            ("Clients", DeviceDetailTab::Clients),
            ("Overview", DeviceDetailTab::Overview),
        ] {
            let rows = draw(&screen, 120, 30);
            screen.handle_mouse_input(click_label(&rows, " Overview ", label));
            assert_eq!(tab(&screen), Some(expected), "{label}");
        }

        let rows = draw(&screen, 120, 30);
        let ports = click_label(&rows, " Overview ", "Ports");
        press(&mut screen, KeyCode::Esc);
        draw(&screen, 120, 30);

        assert!(screen.handle_mouse_input(ports).is_none());
        assert!(screen.detail.is_none());
    }

    #[test]
    fn tapping_rows_selects_then_opens_and_the_detail_follows() {
        let selected = |screen: &DevicesScreen| {
            screen
                .selected_device()
                .and_then(|device| device.name.clone())
        };
        let mut screen = screen();

        let rows = draw(&screen, 120, 30);
        screen.handle_mouse_input(click_label(&rows, "ap-roof", "ap-roof"));
        assert_eq!(selected(&screen).as_deref(), Some("ap-roof"));
        assert!(screen.detail.is_none());

        let rows = draw(&screen, 120, 30);
        screen.handle_mouse_input(click_label(&rows, "ap-roof", "ap-roof"));
        assert_eq!(shown(&screen).as_deref(), Some("ap-roof"));

        let rows = draw(&screen, 120, 30);
        screen.handle_mouse_input(click_label(&rows, " Overview ", "Radios"));
        let rows = draw(&screen, 120, 30);
        screen.handle_mouse_input(click_label(&rows, "ap-atrium", "ap-atrium"));
        assert_eq!(shown(&screen).as_deref(), Some("ap-atrium"));
        assert_eq!(
            tab(&screen),
            Some(DeviceDetailTab::Radios),
            "the tab stays put"
        );

        // The wheel over the table moves the selection, and the detail
        // follows it.
        let rows = draw(&screen, 120, 30);
        let over_table = click_label(&rows, "ap-roof", "ap-roof");
        screen.handle_mouse_input(mouse(
            MouseEventKind::ScrollDown,
            over_table.column,
            over_table.row,
        ));
        assert_eq!(shown(&screen).as_deref(), Some("ap-roof"));
        assert_eq!(tab(&screen), Some(DeviceDetailTab::Radios));
    }

    #[test]
    fn the_wheel_over_the_ports_tab_scrolls_its_list_and_a_tap_moves_the_cursor() {
        let mut screen = screen();
        screen.select(2);
        press(&mut screen, KeyCode::Enter);
        screen.set_detail_tab(DeviceDetailTab::Ports);
        let port = |screen: &DevicesScreen| screen.cursor_port().map(|(_, port)| port.index);

        let rows = draw(&screen, 120, 34);
        let over_list = click_label(&rows, "Port 2 ", "Port 2 ");
        for _ in 0..3 {
            screen.handle_mouse_input(mouse(
                MouseEventKind::ScrollDown,
                over_list.column,
                over_list.row,
            ));
        }
        assert_eq!(port(&screen), Some(4));
        assert_eq!(shown(&screen).as_deref(), Some("sw-core"));

        let rows = draw(&screen, 120, 34);
        screen.handle_mouse_input(click_label(&rows, "Port 6 ", "Port 6 "));
        assert_eq!(port(&screen), Some(6));
    }

    #[test]
    fn scrolling_back_up_keeps_the_rows_below_the_cursor_in_view() {
        let mut screen = screen_of(
            (0..40)
                .map(|index| {
                    ap(
                        index,
                        &format!("dev-{index:02}"),
                        "U7 Pro",
                        "8.1.113",
                        false,
                    )
                })
                .collect(),
        );
        let tap = |screen: &mut DevicesScreen, code: KeyCode| {
            press(screen, code);
            draw(screen, 120, 20)
        };

        let rows = tap(&mut screen, KeyCode::Char('G'));
        assert!(rows.iter().any(|row| row.contains("dev-39")));
        for _ in 0..9 {
            tap(&mut screen, KeyCode::Char('k'));
        }
        let rows = tap(&mut screen, KeyCode::Char('k'));

        let text = rows.join("\n");
        assert!(text.contains("dev-29"), "cursor row scrolled away:\n{text}");
        assert!(text.contains("dev-39"), "the view jumped back:\n{text}");

        // Taps land on the rows as drawn.
        screen.handle_mouse_input(click_label(&rows, "dev-35", "dev-35"));
        assert_eq!(
            screen
                .selected_device()
                .and_then(|device| device.name.clone())
                .as_deref(),
            Some("dev-35")
        );
    }

    #[test]
    fn wide_table_shows_whole_values_and_the_firmware_columns() {
        for width in [120, 183] {
            let text = draw(&screen(), width, 12).join("\n");
            for expected in [
                "U6 Enterprise",
                "USW Pro Max 24 PoE",
                "8.0.26 ↑",
                "Firmware",
                "Clients",
                "TX/RX bps",
            ] {
                assert!(text.contains(expected), "missing {expected:?}:\n{text}");
            }
            assert!(!text.contains("ap-roof ↑"), "marker drawn twice:\n{text}");
        }
    }

    #[test]
    fn narrow_table_keeps_the_core_columns_and_marks_updates_on_the_name() {
        let text = draw(&screen(), 80, 12).join("\n");
        for expected in [
            "U6 Enterprise",
            "USW Pro Max 24 PoE",
            "10.40.0.15",
            "Uptime",
            "ap-roof ↑",
        ] {
            assert!(text.contains(expected), "missing {expected:?}:\n{text}");
        }
        assert!(!text.contains("ap-atrium ↑"), "{text}");
        assert!(
            !text.contains("Firmware"),
            "wide-only column at 80:\n{text}"
        );
    }

    #[test]
    fn the_title_counts_and_the_filter_bar_shows_the_sort_at_its_right_edge() {
        let mut screen = screen();
        let rows = draw(&screen, 120, 12);
        assert!(rows[0].contains("Devices (3)"), "{}", rows[0]);
        // The count is the title's alone, and the sort sits against the
        // right edge whatever the filter strip says.
        assert!(!rows[1].contains("devices"), "{}", rows[1]);
        assert!(
            rows[1].starts_with("\u{2502} All  Gateways  Switches  APs  Offline  Updates 1 "),
            "{}",
            rows[1]
        );
        assert!(
            rows[1].ends_with("s sort name \u{2191} \u{2502}"),
            "{}",
            rows[1]
        );

        // A tap on the sort steps it, like `s`.
        let tap = screen.handle_mouse_input(click_label(&rows, "sort name", "sort name"));
        assert!(matches!(tap, Some(Action::PressKey(key)) if key.code == KeyCode::Char('s')));

        // With a detail open `s` is not the table's, so the chip is text
        // and names no key.
        press(&mut screen, KeyCode::Enter);
        let rows = draw(&screen, 120, 24);
        assert!(rows[1].contains("  sort name"), "{}", rows[1]);
        assert!(!rows[1].contains("s sort name"), "{}", rows[1]);
        assert!(
            screen
                .handle_mouse_input(click_label(&rows, "sort name", "sort name"))
                .is_none(),
            "a button that presses a dead key"
        );
        press(&mut screen, KeyCode::Esc);

        screen.set_search_query("roof");
        let top = draw(&screen, 120, 12).join("\n");
        assert!(top.contains("Devices (1/3) [\"roof\"]"), "{top}");
    }

    #[test]
    fn waiting_updates_are_counted_on_the_filter_that_lists_them() {
        let mut screen = screen();
        let rows = draw(&screen, 120, 12);
        assert!(rows[1].contains("Updates 1 "), "{}", rows[1]);
        assert!(!rows[1].contains("1 update"), "{}", rows[1]);

        // The count is part of the tab: a tap on it selects the filter.
        screen.handle_mouse_input(click_label(&rows, "Updates", "1 "));
        let text = draw(&screen, 120, 12).join("\n");
        assert!(
            text.contains("ap-roof") && !text.contains("sw-core"),
            "{text}"
        );

        let current = screen_of(vec![ap(15, "ap-atrium", "U6 Enterprise", "8.1.113", false)]);
        let rows = draw(&current, 120, 12);
        assert!(rows[1].contains("Updates  "), "{}", rows[1]);
    }

    #[test]
    fn a_narrow_filter_bar_keeps_the_filters_and_drops_the_sort() {
        let rows = draw(&screen(), 60, 12);
        assert!(rows[1].contains("Updates 1"), "{}", rows[1]);
        assert!(!rows[1].contains("sort"), "{}", rows[1]);
    }

    #[test]
    fn a_filter_tap_selects_it() {
        let mut screen = screen();
        let rows = draw(&screen, 120, 12);
        screen.handle_mouse_input(click_label(&rows, "Gateways", "Switches"));
        let text = draw(&screen, 120, 12).join("\n");
        assert!(
            text.contains("sw-core") && !text.contains("ap-roof"),
            "{text}"
        );
    }

    #[test]
    fn an_empty_table_says_why_instead_of_drawing_a_bare_header() {
        let waiting = DevicesScreen::new();
        let text = draw(&waiting, 80, 12).join("\n");
        assert!(text.contains("Waiting for the controller"), "{text}");
        assert!(!text.contains("Model"), "a header over nothing:\n{text}");

        let empty = screen_of(Vec::new());
        let text = draw(&empty, 80, 12).join("\n");
        assert!(text.contains("No devices on this site"), "{text}");

        let mut screen = screen();
        press(&mut screen, KeyCode::Char('l'));
        let text = draw(&screen, 80, 12).join("\n");
        assert!(text.contains("No gateways on this site"), "{text}");

        screen.set_search_query("zzz");
        let text = draw(&screen, 80, 12).join("\n");
        assert!(text.contains("No device matches \"zzz\""), "{text}");
    }

    #[test]
    fn hints_only_offer_what_works_for_the_selected_device() {
        let mut screen = screen_of(vec![
            ap(1, "alpha", "U7 Pro", "8.1.113", false),
            ap(2, "bravo", "U7 Pro", "8.0.26", true),
            device(3, "charlie", &json!({ "state": "PendingAdoption" })),
            device(4, "delta", &json!({ "state": "Offline" })),
        ]);

        let alpha = hints(&screen);
        assert!(
            alpha.starts_with("Enter detail") && alpha.contains("R restart"),
            "{alpha}"
        );
        assert!(
            !alpha.contains("upgrade") && !alpha.contains("adopt"),
            "{alpha}"
        );
        // Movement and the keys the filter bar already shows are not
        // actions on the selection.
        for shared in ["j/k", "h/l", "sort", "filter"] {
            assert!(!alpha.contains(shared), "{shared} is a hint: {alpha}");
        }

        screen.select(1);
        assert!(hints(&screen).contains("U upgrade"));

        screen.select(2);
        assert_eq!(hints(&screen), "Enter detail  A adopt");

        screen.select(3);
        assert_eq!(hints(&screen), "Enter detail");

        // With a detail open the bar is the detail's.
        screen.select(0);
        press(&mut screen, KeyCode::Enter);
        let bar = hints(&screen);
        assert!(!bar.contains("Enter"), "a dead Enter hint: {bar}");
        assert!(
            bar.starts_with("R restart") && bar.contains("F remove"),
            "{bar}"
        );
        assert!(bar.ends_with("Esc close"), "{bar}");

        // Nothing is listed for an empty table.
        assert_eq!(hints(&screen_of(Vec::new())), "");
    }

    #[test]
    fn the_screen_draws_no_hint_row_and_gives_the_row_to_the_table() {
        let mut screen = screen_of(
            (0..40)
                .map(|index| {
                    ap(
                        index,
                        &format!("dev-{index:02}"),
                        "U7 Pro",
                        "8.1.113",
                        false,
                    )
                })
                .collect(),
        );
        // A border, the filter bar, and the header leave 16 of 20 rows.
        let rows = draw(&screen, 120, 20);
        assert!(rows[18].contains("dev-15"), "{}", rows[18]);
        for dead in ["navigate", "Enter detail", "R restart"] {
            assert!(!rows.join("\n").contains(dead), "{dead} is drawn");
        }

        press(&mut screen, KeyCode::Enter);
        let text = draw(&screen, 120, 30).join("\n");
        for dead in ["Esc close", "h/l tabs", "j/k device", "R restart"] {
            assert!(!text.contains(dead), "{dead} is drawn:\n{text}");
        }
    }

    #[test]
    fn a_narrow_bar_keeps_the_ports_own_commands_and_esc() {
        let mut screen = screen();
        screen.select(2);
        press(&mut screen, KeyCode::Enter);
        screen.set_detail_tab(DeviceDetailTab::Ports);
        let fitted: Vec<_> = click::fit_hints(status_hints(&screen), 40)
            .into_iter()
            .map(|hint| (0, hint))
            .collect();
        let bar = click::hint_text(&fitted);
        assert!(bar.ends_with("Esc close"), "{bar}");
        assert!(bar.starts_with("C power cycle"), "{bar}");
        assert!(!bar.contains("provision"), "{bar}");
    }

    #[test]
    fn the_clients_tab_offers_enter_only_with_a_client_under_the_cursor() {
        let mut screen = screen();
        screen.select(2);
        press(&mut screen, KeyCode::Enter);
        screen.set_detail_tab(DeviceDetailTab::Clients);
        assert!(!hints(&screen).contains("Enter"), "{}", hints(&screen));

        screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![client(
            1,
            "nas-vault",
            &json!({ "uplink_device_mac": "aa:bb:cc:dd:ee:02", "switch_port": 3 }),
        )])));
        let bar = hints(&screen);
        assert!(bar.starts_with("Enter client"), "{bar}");
        assert!(bar.ends_with("Esc close"), "{bar}");
    }

    #[test]
    fn port_hints_leave_out_the_settings_keys_without_a_session() {
        let mut screen = screen();
        screen.select(2);
        press(&mut screen, KeyCode::Enter);
        screen.set_detail_tab(DeviceDetailTab::Ports);
        // Until the port's profile loads, O has no direction to offer.
        let loading = hints(&screen);
        assert!(
            loading.contains("N rename") && !loading.contains("PoE"),
            "{loading}"
        );
        let profile: unifly_api::model::PortProfile = serde_json::from_value(json!({
            "index": 1, "name": "Port 1", "link_state": "Up", "mode": "access",
            "native_network_id": null, "native_vlan_id": null,
            "native_network_name": null, "tagged_network_ids": [],
            "tagged_vlan_ids": [], "tagged_network_names": [], "tagged_all": true,
            "poe_mode": "auto", "speed_setting": "auto", "link_speed_mbps": 1000,
            "stp_state": "forwarding", "port_profile_id": null
        }))
        .expect("profile fixture");
        screen.apply_action(&Action::PortProfilesLoaded {
            device: unifly_api::EntityId::from("device-2"),
            ports: Arc::new(vec![profile]),
        });
        let with_session = hints(&screen);
        assert!(
            with_session.starts_with("C power cycle  O PoE off  N rename"),
            "{with_session}"
        );

        // A port with no PoE has no power to cycle or switch.
        screen.set_detail_cursor(8);
        let plain = hints(&screen);
        assert!(plain.starts_with("N rename"), "{plain}");
        assert!(
            !plain.contains("power cycle") && !plain.contains("PoE"),
            "{plain}"
        );
        screen.set_detail_cursor(0);

        screen.apply_action(&Action::AccessUpdated(Access {
            integration: true,
            session: false,
            live_events: false,
        }));
        let cloud = hints(&screen);
        assert!(cloud.contains("C power cycle"), "{cloud}");
        for dead in ["PoE off", "rename", "provision", "upgrade"] {
            assert!(!cloud.contains(dead), "{dead} is offered: {cloud}");
        }
        assert!(cloud.contains("F remove"), "{cloud}");

        // A Session-only profile has no Integration API to power cycle a
        // port or remove a device with.
        screen.apply_action(&Action::AccessUpdated(Access {
            integration: false,
            session: true,
            live_events: true,
        }));
        let session_only = hints(&screen);
        assert!(session_only.contains("N rename"), "{session_only}");
        for dead in ["power cycle", "F remove"] {
            assert!(
                !session_only.contains(dead),
                "{dead} is offered: {session_only}"
            );
        }
    }

    #[test]
    fn the_port_marker_names_the_port_not_its_row() {
        // A device whose port numbering has gaps: 1, 2, 9, 10.
        let ports: Vec<serde_json::Value> = [1, 2, 9, 10]
            .iter()
            .map(|index| {
                json!({
                    "index": index, "name": format!("Port {index}"), "state": "Up",
                    "speed_mbps": 1000, "max_speed_mbps": 1000, "connector": "Rj45",
                    "poe": null
                })
            })
            .collect();
        let mut screen = DevicesScreen::new();
        screen.apply_devices_update(&[device(
            1,
            "gapped",
            &json!({ "device_type": "Switch", "has_access_point": false, "ports": ports }),
        )]);
        press(&mut screen, KeyCode::Enter);
        screen.set_detail_tab(DeviceDetailTab::Ports);

        let first = draw(&screen, 100, 24).join("\n");
        assert!(first.contains("port 1 of 4"), "{first}");
        screen.set_detail_cursor(2);
        let third = draw(&screen, 100, 24).join("\n");
        assert!(third.contains("port 9 \u{b7} 3 of 4"), "{third}");
    }

    #[test]
    fn every_port_of_a_24_port_switch_is_reachable_at_80_by_24() {
        let mut screen = screen();
        screen.select(2);
        press(&mut screen, KeyCode::Enter);
        screen.set_detail_tab(DeviceDetailTab::Ports);

        // The screen gets 22 of the 24 rows; the app keeps two.
        let first = draw(&screen, 80, 22).join("\n");
        assert!(first.contains("port 1 of 24"), "{first}");
        assert!(first.contains("Port 1 "), "{first}");
        assert!(!first.contains("Port 24"), "{first}");

        let mut seen = 0;
        for port in 1..=24 {
            let text = draw(&screen, 80, 22).join("\n");
            assert!(
                text.contains(&format!("port {port} of 24")),
                "no position marker for port {port}:\n{text}"
            );
            let cursor_row = text
                .lines()
                .find(|row| row.contains('▸') && row.contains(&format!("Port {port} ")));
            assert!(cursor_row.is_some(), "port {port} is off screen:\n{text}");
            seen += 1;
            press(&mut screen, KeyCode::Char('j'));
        }
        assert_eq!(seen, 24);
    }

    #[test]
    fn the_ports_tab_shows_profiles_once_loaded_and_says_why_when_it_cannot() {
        use unifly_api::model::PortProfile;

        let mut screen = screen();
        screen.select(2);
        press(&mut screen, KeyCode::Enter);
        let load = press(&mut screen, KeyCode::Char('h'));
        assert!(matches!(load, Some(Action::RequestPortProfiles(_))));
        let text = draw(&screen, 120, 34).join("\n");
        assert!(text.contains("Loading the port profiles"), "{text}");
        assert!(text.contains("Max"), "hardware columns until then:\n{text}");

        let id = unifly_api::EntityId::from("device-2");
        screen.apply_action(&Action::PortProfilesFailed {
            device: id.clone(),
            error: "port settings need a direct controller session".into(),
        });
        let text = draw(&screen, 120, 34).join("\n");
        assert!(
            text.contains("No port profiles: port settings need a direct controller session"),
            "{text}"
        );

        let profile: PortProfile = serde_json::from_value(json!({
            "index": 1, "name": "cam-lobby", "link_state": "Up", "mode": "access",
            "native_network_id": "n1", "native_vlan_id": 40,
            "native_network_name": "Cameras", "tagged_network_ids": [],
            "tagged_vlan_ids": [], "tagged_network_names": [], "tagged_all": false,
            "poe_mode": "auto", "speed_setting": "auto", "link_speed_mbps": 1000,
            "stp_state": "forwarding", "port_profile_id": null
        }))
        .expect("profile fixture");
        screen.apply_action(&Action::PortProfilesLoaded {
            device: id,
            ports: Arc::new(vec![profile]),
        });
        let text = draw(&screen, 120, 34).join("\n");
        for expected in ["Mode", "Native", "Tagged", "cam-lobby", "access", "Cameras"] {
            assert!(text.contains(expected), "missing {expected:?}:\n{text}");
        }
        assert!(!text.contains("No port profiles"), "{text}");
    }

    #[test]
    fn a_48_port_faceplate_is_drawn_whole_or_not_at_all() {
        let mut screen = screen_of(vec![switch(1, "sw-big", 48)]);
        press(&mut screen, KeyCode::Enter);
        screen.set_detail_tab(DeviceDetailTab::Ports);

        // Two banks at 100 columns: nine rows with the legend.
        let text = draw(&screen, 100, 40).join("\n");
        assert!(text.contains("⇅ uplink"), "the legend is cut:\n{text}");
        assert!(text.contains(" 48 "), "the second bank is cut:\n{text}");

        // Too short for it: the list takes the room.
        let text = draw(&screen, 100, 20).join("\n");
        assert!(!text.contains("⇅ uplink"), "{text}");
        assert!(text.contains("port 1 of 48"), "{text}");
    }

    #[test]
    fn the_overview_is_an_aligned_grid_in_words_and_local_time() {
        let mut screen = screen_of(vec![
            switch(2, "sw-core", 8),
            device(
                7,
                "ap-lab",
                &json!({
                    "state": "ConnectionInterrupted",
                    "serial": "503CC1984A07",
                    "uplink_device_mac": "aa:bb:cc:dd:ee:02",
                    "uplink_port_idx": 5,
                    "adopted_at": "2025-04-22T16:55:00Z",
                    "firmware_version": null
                }),
            ),
        ]);
        press(&mut screen, KeyCode::Enter);
        let rows = draw(&screen, 80, 24);
        let text = rows.join("\n");

        for expected in [
            "Connection interrupted",
            "Access Point",
            "10.40.0.7",
            "503CC1984A07",
            "sw-core port 5",
            "aa:bb:cc:dd:ee:07",
        ] {
            assert!(text.contains(expected), "missing {expected:?}:\n{text}");
        }
        assert!(!text.contains("AccessPoint") && !text.contains("ConnectionInterrupted"));
        assert!(!text.contains("UTC"), "{text}");
        assert!(!text.contains("update"), "unknown firmware:\n{text}");
        // An access point has no ports to show.
        assert!(!text.contains("Ports"), "{text}");

        let pane = tab_row(&rows);
        let column = |label: &str, value: &str| {
            let row = rows[pane..].iter().find(|row| row.contains(label))?;
            Some(row.find(value)? - row.find(label)?)
        };
        assert_eq!(
            column("IP ", "10.40.0.7"),
            column("Uplink ", "sw-core port 5"),
            "values start in different columns:\n{text}"
        );
    }

    #[test]
    fn the_overview_carries_the_load_figures_and_marks_an_update() {
        let mut screen = screen_of(vec![device(
            18,
            "ap-roof",
            &json!({
                "firmware_version": "8.0.26",
                "firmware_updatable": true,
                "stats": {
                    "uptime_secs": 86_400,
                    "cpu_utilization_pct": 11.3,
                    "memory_utilization_pct": 44.0,
                    "load_average_1m": 0.24,
                    "load_average_5m": 0.21,
                    "load_average_15m": 0.25,
                    "uplink_bandwidth": {
                        "tx_bytes_per_sec": 17_312_500,
                        "rx_bytes_per_sec": 40_412_500
                    },
                    "last_heartbeat": "2025-04-22T16:55:00Z"
                }
            }),
        )]);
        press(&mut screen, KeyCode::Enter);

        // Wide enough for two columns of whole values, and at the width
        // where the two columns are at their tightest.
        for width in [120, 80] {
            let rows = draw(&screen, width, 24);
            let text = rows.join("\n");
            for expected in [
                "11.3%",
                "44.0%",
                "0.24  0.21  0.25",
                "138.5 Mbps / 323.3 Mbps",
                "8.0.26 \u{2191} update",
                "aa:bb:cc:dd:ee:12",
            ] {
                assert!(
                    text.contains(expected),
                    "missing {expected:?} at {width}:\n{text}"
                );
            }
            let pane = &rows[tab_row(&rows)..];
            assert!(
                !pane.iter().any(|row| row.contains('\u{2026}')),
                "a value is cut at {width}:\n{text}"
            );
            // With no last contact from the Session API, the heartbeat
            // dates the device, under the one label.
            assert!(!text.contains("Heartbeat"), "{text}");
            let seen = pane.iter().find(|row| row.contains("Last seen"));
            assert!(seen.is_some_and(|row| row.contains(" ago")), "{text}");
            assert!(!text.contains("Performance"), "{text}");
        }
    }

    #[test]
    fn the_pane_takes_the_rows_its_tab_needs_and_no_spacers() {
        let mut screen = screen();
        screen.select(2);
        press(&mut screen, KeyCode::Enter);

        // Fifteen fields in two columns: a border, the tab strip, eight
        // rows, a border, right under a table that shows every device.
        let rows = draw(&screen, 120, 44);
        let tabs = tab_row(&rows);
        assert_eq!(device_rows(&rows), 3, "{}", rows.join("\n"));
        assert_eq!(tabs, 7, "the pane sits under the table");
        assert!(rows[tabs - 1].contains("sw-core  \u{b7}  USW Pro Max 24 PoE"));
        assert!(rows[tabs + 1].contains("State"), "{}", rows[tabs + 1]);
        assert!(rows[tabs + 8].contains("Uptime"), "{}", rows[tabs + 8]);
        assert!(
            rows[tabs + 9].starts_with("\u{2502}\u{2570}"),
            "{}",
            rows[tabs + 9]
        );

        // A list tab takes a row per entry, and the strip stays where it
        // was while the table has its rows.
        screen.set_detail_tab(DeviceDetailTab::Ports);
        let rows = draw(&screen, 120, 44);
        assert_eq!(tab_row(&rows), tabs, "the tab strip moved");
        assert!(rows.join("\n").contains("Port 24 "), "every port fits");

        // An empty list is one line of explanation, not a tall box.
        screen.set_detail_tab(DeviceDetailTab::Clients);
        let rows = draw(&screen, 120, 44);
        assert_eq!(tab_row(&rows), tabs);
        assert!(rows[tabs + 1].contains("No clients connected through this device"));
        assert!(
            rows[tabs + 2].starts_with("\u{2502}\u{2570}"),
            "{}",
            rows[tabs + 2]
        );
    }

    #[test]
    fn a_long_list_tab_leaves_the_table_its_share() {
        let mut devices: Vec<_> = (0..40)
            .map(|index| {
                ap(
                    index + 10,
                    &format!("dev-{index:02}"),
                    "U7 Pro",
                    "8.1.113",
                    false,
                )
            })
            .collect();
        devices.push(switch(2, "sw-big", 48));
        let mut screen = screen_of(devices);
        screen.select(40);
        press(&mut screen, KeyCode::Enter);
        screen.set_detail_tab(DeviceDetailTab::Ports);

        // 38 rows inside the border: the table keeps twelve of them, the
        // filter bar and header among them, however long the port list.
        let rows = draw(&screen, 120, 40);
        assert_eq!(tab_row(&rows), 14, "{}", rows.join("\n"));
        assert!(rows[12].contains("sw-big"), "the open device is in view");

        // On the Overview the table takes back what the pane does not use.
        screen.set_detail_tab(DeviceDetailTab::Overview);
        let rows = draw(&screen, 120, 40);
        assert_eq!(tab_row(&rows), 29, "{}", rows.join("\n"));
    }

    #[test]
    fn the_clients_tab_shows_signal_only_where_a_client_has_one() {
        let mut screen = screen();
        screen.select(2);
        press(&mut screen, KeyCode::Enter);
        screen.set_detail_tab(DeviceDetailTab::Clients);
        let wired = client(
            1,
            "nas-vault",
            &json!({ "uplink_device_mac": "aa:bb:cc:dd:ee:02", "switch_port": 3 }),
        );
        screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![wired.clone()])));
        let text = draw(&screen, 120, 30).join("\n");
        assert!(text.contains("Link"), "{text}");
        assert!(!text.contains("Signal"), "a column of dashes:\n{text}");

        let phone = client(2, "phone", &wireless(-58, "aa:bb:cc:dd:ee:02"));
        screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![wired, phone])));
        let text = draw(&screen, 120, 30).join("\n");
        assert!(
            text.contains("Signal") && text.contains("-58 dBm"),
            "{text}"
        );
    }

    #[test]
    fn a_table_with_no_room_for_a_device_steps_aside_for_the_detail() {
        let mut screen = screen();
        screen.select(2);
        press(&mut screen, KeyCode::Enter);
        screen.set_detail_tab(DeviceDetailTab::Clients);
        let on_switch = |index: u8, name: &str| {
            client(
                index,
                name,
                &json!({ "uplink_device_mac": "aa:bb:cc:dd:ee:02" }),
            )
        };
        screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![
            on_switch(1, "nas-vault"),
            on_switch(2, "render-01"),
        ])));

        // Seven rows inside the border and a pane of six: the one row
        // left would hold the filter bar and no device.
        let rows = draw(&screen, 100, 9);
        let text = rows.join("\n");
        assert!(rows[1].contains("sw-core  \u{b7}  USW"), "{text}");
        assert!(!text.contains("Gateways"), "a bare filter bar:\n{text}");
        assert!(text.contains("render-01"), "{text}");

        // Two more rows make room for the header and the open device.
        let rows = draw(&screen, 100, 11);
        assert!(rows[3].contains("sw-core"), "{}", rows.join("\n"));
        assert!(
            rows[4].contains("sw-core  \u{b7}  USW"),
            "{}",
            rows.join("\n")
        );
    }

    #[test]
    fn the_clients_tab_counts_the_clients_it_lists() {
        let mut screen = screen();
        screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![
            client(
                1,
                "nas-vault",
                &json!({ "uplink_device_mac": "aa:bb:cc:dd:ee:02", "switch_port": 3 }),
            ),
            client(
                2,
                "render-01",
                &json!({ "uplink_device_mac": "aa:bb:cc:dd:ee:02" }),
            ),
        ])));
        screen.select(2);
        press(&mut screen, KeyCode::Enter);
        screen.set_detail_tab(DeviceDetailTab::Clients);

        let text = draw(&screen, 120, 30).join("\n");
        assert!(text.contains("client 1 of 2"), "{text}");
        assert!(
            text.contains("nas-vault") && text.contains("port 3"),
            "{text}"
        );
        assert!(!text.contains("Connected: 0"), "{text}");

        screen.apply_action(&Action::ClientsUpdated(Arc::new(Vec::new())));
        let text = draw(&screen, 120, 30).join("\n");
        assert!(
            text.contains("No clients connected through this device"),
            "{text}"
        );
    }

    #[test]
    fn nothing_panics_at_tiny_sizes() {
        let mut screen = screen();
        screen.select(2);
        for (width, height) in [(40, 10), (20, 5), (10, 3), (1, 1)] {
            draw(&screen, width, height);
        }
        press(&mut screen, KeyCode::Enter);
        for tab in DeviceDetailTab::ALL {
            screen.set_detail_tab(tab);
            for (width, height) in [(40, 10), (20, 5), (10, 3), (1, 1)] {
                draw(&screen, width, height);
            }
        }
    }
}
