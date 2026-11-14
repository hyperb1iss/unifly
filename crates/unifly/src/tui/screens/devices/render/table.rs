use std::sync::Arc;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table};

use unifly_api::{Device, DeviceState};

use crossterm::event::KeyCode;

use crate::tui::table::{Column, plan_columns};
use crate::tui::theme;
use crate::tui::widgets::click::{self, Click, Hint};
use crate::tui::widgets::{bytes_fmt, status_indicator, sub_tabs};

use super::detail::render_detail;
use super::grid;
use crate::tui::screens::devices::DevicesScreen;
use crate::tui::screens::devices::commands::{is_gateway, is_pending, is_reachable};
use crate::tui::screens::devices::view::{DeviceFilter, counted};

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

    // With a detail open the table keeps a few rows around the cursor and
    // the pane takes the rest; its hint bar then speaks for the screen.
    // On a very short screen the pane takes all of it.
    let detail_device = screen.detail_device();
    let (table_area, detail_area) = if detail_device.is_some() {
        let list = grid::list_rows_beside_detail(inner.height);
        let chunks = Layout::vertical([Constraint::Length(list), Constraint::Min(0)]).split(inner);
        (chunks[0], Some(chunks[1]))
    } else {
        (inner, None)
    };

    let hint_rows = u16::from(detail_area.is_none());
    let layout = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(hint_rows),
    ])
    .split(table_area);

    render_filter_bar(screen, frame, layout[0], filtered.len());
    if filtered.is_empty() {
        grid::note(frame, layout[1], &empty_note(screen));
    } else {
        render_device_table(screen, frame, layout[1], &filtered, selected_index);
    }
    if detail_area.is_none() {
        render_table_hints(screen, frame, layout[2]);
    }

    if let (Some(detail_area), Some(device)) = (detail_area, detail_device) {
        render_detail(screen, frame, detail_area, device);
    }
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

/// The filter strip, then the sort, the row count, and the firmware
/// updates waiting, as far as the width allows.
fn render_filter_bar(screen: &DevicesScreen, frame: &mut Frame, area: Rect, shown: usize) {
    let mut bar = click::ClickLine::default();
    bar.push_gap(Span::raw(" "));
    bar.append(sub_tabs::sub_tab_strip(
        &DeviceFilter::LABELS,
        screen.filter.index(),
    ));

    let secondary = Style::default().fg(theme::text_secondary());
    let sort = [
        Span::styled("   sort ", secondary),
        Span::styled(
            screen.sort.label(),
            Style::default().fg(theme::accent_secondary()),
        ),
    ];
    let room = |bar: &click::ClickLine<'_, Click>, spans: &[Span<'_>]| {
        bar.width() + spans.iter().map(Span::width).sum::<usize>() < usize::from(area.width)
    };
    if room(&bar, &sort) {
        for span in sort {
            // `s` belongs to the table, so the label is only a button
            // while no detail has the keys.
            if screen.detail.is_none() {
                bar.push(Click::Key(click::char_key('s')), span);
            } else {
                bar.push_gap(span);
            }
        }
    }
    let count = [Span::styled(
        format!("   {}", counted(shown, "device", "devices")),
        secondary,
    )];
    if room(&bar, &count) {
        bar.push_gap(count[0].clone());
    }
    let updates = screen
        .devices
        .iter()
        .filter(|device| device.firmware_updatable)
        .count();
    let waiting = [Span::styled(
        format!(
            "   {UPDATE_MARKER} {}",
            counted(updates, "update", "updates")
        ),
        Style::default().fg(theme::warning()),
    )];
    if updates > 0 && room(&bar, &waiting) {
        bar.push_gap(waiting[0].clone());
    }
    let line = bar.draw(area, &mut screen.hits.borrow_mut());
    frame.render_widget(Paragraph::new(line), area);
}

/// Table width from which the Devices table adds its Firmware and Clients columns:
/// a 120-column terminal, less the screen border.
const WIDE_TABLE: u16 = 118;

/// Marks a device with a firmware update waiting.
const UPDATE_MARKER: &str = "\u{2191}";

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

/// The table's hints: movement and view keys, then the commands the
/// selected device can take right now.
fn render_table_hints(screen: &DevicesScreen, frame: &mut Frame, area: Rect) {
    let mut hints = vec![
        (0, Hint::info("j/k", "navigate")),
        (0, Hint::button("Enter", "detail", KeyCode::Enter)),
        (2, Hint::button("h/l", "filter", click::char_key('l'))),
        (3, Hint::button("s", "sort", click::char_key('s'))),
    ];
    if let Some(device) = screen.selected_device() {
        hints.extend(command_hints(screen, device));
    }
    let hints = grid::fit_hints(hints, area.width);
    let line = click::key_hints(&hints).draw(area, &mut screen.hits.borrow_mut());
    frame.render_widget(Paragraph::new(line), area);
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
    use crate::tui::screens::devices::fixtures::{client, device, switch};
    use crate::tui::table::render_lines;
    use crate::tui::widgets::click::{click_label, mouse};

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

    /// The hint bar: the last row inside the screen's border, or inside
    /// the detail's when one is open.
    fn hints(rows: &[String]) -> &str {
        rows.iter()
            .rev()
            .find(|row| row.contains("j/k") || row.contains("h/l"))
            .map_or("", String::as_str)
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
            screen.handle_mouse_input(click_label(&rows, "Performance", label));
            assert_eq!(tab(&screen), Some(expected), "{label}");
        }

        let rows = draw(&screen, 120, 30);
        let ports = click_label(&rows, "Performance", "Ports");
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
        screen.handle_mouse_input(click_label(&rows, "Performance", "Radios"));
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
    fn the_filter_bar_counts_in_the_singular_and_shows_the_sort() {
        let mut screen = screen();
        let top = draw(&screen, 120, 12).join("\n");
        assert!(top.contains("Devices (3)"), "{top}");
        assert!(top.contains("3 devices"), "{top}");
        assert!(top.contains("sort name ↑"), "{top}");
        assert!(top.contains("↑ 1 update "), "{top}");

        // A tap on the sort steps it, like `s`.
        let rows = draw(&screen, 120, 12);
        let tap = screen.handle_mouse_input(click_label(&rows, "sort name", "sort name"));
        assert!(matches!(tap, Some(Action::PressKey(key)) if key.code == KeyCode::Char('s')));

        // With a detail open `s` is not the table's, so the label is text.
        press(&mut screen, KeyCode::Enter);
        let rows = draw(&screen, 120, 24);
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
        assert!(top.contains("1 device "), "{top}");
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
        let bar = |screen: &DevicesScreen| hints(&draw(screen, 120, 14)).to_owned();

        let alpha = bar(&screen);
        assert!(
            alpha.contains("Enter detail") && alpha.contains("R restart"),
            "{alpha}"
        );
        assert!(
            !alpha.contains("upgrade") && !alpha.contains("adopt"),
            "{alpha}"
        );

        screen.select(1);
        assert!(bar(&screen).contains("U upgrade"));

        screen.select(2);
        let pending = bar(&screen);
        assert!(
            pending.contains("A adopt") && !pending.contains("restart"),
            "{pending}"
        );

        screen.select(3);
        let offline = bar(&screen);
        assert!(
            !offline.contains("restart") && !offline.contains("locate"),
            "{offline}"
        );

        // With a detail open there is one bar, and it is the detail's.
        screen.select(0);
        press(&mut screen, KeyCode::Enter);
        let rows = draw(&screen, 120, 24);
        let text = rows.join("\n");
        assert!(!text.contains("Enter detail"), "a dead Enter hint:\n{text}");
        assert!(!text.contains("s sort"), "{text}");
        let detail = hints(&rows);
        assert!(
            detail.contains("h/l tabs") && detail.contains("j/k device"),
            "{detail}"
        );
        assert!(detail.contains("Esc close"), "{detail}");
    }

    #[test]
    fn a_narrow_hint_bar_drops_whole_hints_and_keeps_esc() {
        let mut screen = screen();
        screen.select(2);
        press(&mut screen, KeyCode::Enter);
        screen.set_detail_tab(DeviceDetailTab::Ports);
        let rows = draw(&screen, 60, 24);
        let bar = hints(&rows);
        assert!(bar.contains("Esc close"), "{bar}");
        assert!(bar.contains("C power cycle"), "{bar}");
        assert!(!bar.contains("provision"), "{bar}");
    }

    #[test]
    fn port_hints_leave_out_the_settings_keys_without_a_session() {
        let mut screen = screen();
        screen.select(2);
        press(&mut screen, KeyCode::Enter);
        screen.set_detail_tab(DeviceDetailTab::Ports);
        // Until the port's profile loads, O has no direction to offer.
        let loading = hints(&draw(&screen, 120, 30)).to_owned();
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
        let with_session = hints(&draw(&screen, 120, 30)).to_owned();
        assert!(
            with_session.contains("O PoE off") && with_session.contains("N rename"),
            "{with_session}"
        );

        screen.apply_action(&Action::AccessUpdated(Access {
            integration: true,
            session: false,
            live_events: false,
        }));
        let rows = draw(&screen, 120, 30);
        let cloud = hints(&rows);
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
        let session_only = hints(&draw(&screen, 120, 30)).to_owned();
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
        let text = draw(&screen, 100, 24).join("\n");
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
        assert!(!text.contains("up to date"), "unknown firmware:\n{text}");
        // An access point has no ports to show.
        assert!(!text.contains("Ports"), "{text}");

        let pane = rows
            .iter()
            .position(|row| row.contains(" Overview "))
            .unwrap_or(0);
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
