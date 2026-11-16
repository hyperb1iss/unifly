use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use unifly_api::Device;
use unifly_api::model::{PoeMode, UplinkMedium};

use super::grid::{self, Field};
use super::table::{UPDATE_MARKER, command_hints};
use crate::tui::action::DeviceDetailTab;
use crate::tui::screens::devices::DevicesScreen;
use crate::tui::screens::devices::commands::device_name;
use crate::tui::screens::devices::detail::{Detail, tab_label, tabs_for};
use crate::tui::screens::devices::view::{state_label, type_label};
use crate::tui::theme;
use crate::tui::widgets::click::{self, Hint};
use crate::tui::widgets::{bytes_fmt, status_indicator, sub_tabs};

/// Rows the pane takes besides what its tab holds: the border and the tab
/// strip.
const CHROME_ROWS: u16 = 3;

pub(super) fn render_detail(
    screen: &DevicesScreen,
    frame: &mut Frame,
    area: Rect,
    device: &Device,
) {
    let Some(detail) = &screen.detail else {
        return;
    };
    let name = device_name(device);
    let title = match device.model.as_deref() {
        Some(model) => format!(" {name}  ·  {model} "),
        None => format!(" {name} "),
    };
    let mut block = Block::default()
        .title(title)
        .title_style(theme::title_style())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border_focused());
    if let Some(position) = position_marker(screen, detail) {
        block = block.title_bottom(
            Line::from(Span::styled(
                position,
                Style::default().fg(theme::text_secondary()),
            ))
            .right_aligned(),
        );
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);
    screen.detail_hits.borrow_mut().pane = area;

    let layout = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(inner);

    render_detail_tabs(screen, frame, layout[0], device, detail);
    let content = layout[1];
    match detail.tab {
        DeviceDetailTab::Overview => {
            let fields = overview_fields(screen, device);
            frame.render_widget(
                Paragraph::new(grid::lines(&fields, content.width, content.height)),
                content,
            );
        }
        DeviceDetailTab::Radios => render_radios_tab(frame, content, device),
        DeviceDetailTab::Clients => super::clients::render(screen, frame, content, device, detail),
        DeviceDetailTab::Ports => super::ports::render(screen, frame, content, device, detail),
    }
}

/// Rows the pane wants in a screen `width` columns wide when it may take
/// up to `room` of them: its border, its tab strip, and what the open tab
/// holds. The two list tabs ask for every row of their list.
pub(super) fn detail_height(screen: &DevicesScreen, device: &Device, width: u16, room: u16) -> u16 {
    let Some(detail) = &screen.detail else {
        return 0;
    };
    let width = width.saturating_sub(2);
    let content = match detail.tab {
        DeviceDetailTab::Overview => grid::rows_within(
            overview_fields(screen, device).len(),
            width,
            room.saturating_sub(CHROME_ROWS),
        ),
        DeviceDetailTab::Radios => list_rows(device.radios.len()),
        DeviceDetailTab::Clients => list_rows(screen.device_clients(device).len()),
        DeviceDetailTab::Ports => super::ports::wanted_rows(device, detail, width),
    };
    content.saturating_add(CHROME_ROWS)
}

/// Rows a list of `len` entries takes under its header, or the one row of
/// the note that stands in for an empty list.
pub(super) fn list_rows(len: usize) -> u16 {
    match u16::try_from(len) {
        Ok(0) => 1,
        Ok(len) => len.saturating_add(1),
        Err(_) => u16::MAX,
    }
}

/// Where the cursor is in the open tab's list, such as `port 3 of 24`.
/// A port goes by its own number, which on a device with gaps in its
/// numbering is not its place in the list.
fn position_marker(screen: &DevicesScreen, detail: &Detail) -> Option<String> {
    let len = screen.detail_list_len();
    let place = detail.cursor_index() + 1;
    match detail.tab {
        DeviceDetailTab::Ports => {
            let (_, port) = screen.cursor_port()?;
            Some(if usize::try_from(port.index) == Ok(place) {
                format!(" port {place} of {len} ")
            } else {
                format!(" port {} \u{b7} {place} of {len} ", port.index)
            })
        }
        DeviceDetailTab::Clients if len > 0 => Some(format!(" client {place} of {len} ")),
        _ => None,
    }
}

fn render_detail_tabs(
    screen: &DevicesScreen,
    frame: &mut Frame,
    area: Rect,
    device: &Device,
    detail: &Detail,
) {
    let tabs = tabs_for(device);
    let labels: Vec<&str> = tabs.iter().map(|tab| tab_label(*tab)).collect();
    let active = tabs.iter().position(|tab| *tab == detail.tab).unwrap_or(0);
    let line = sub_tabs::sub_tab_strip(&labels, active).draw(area, &mut screen.hits.borrow_mut());
    frame.render_widget(Paragraph::new(line), area);
}

/// What the status bar offers while the detail is open: the open tab's
/// own commands, then the device commands its state allows, then the way
/// out.
pub(super) fn detail_hints(
    screen: &DevicesScreen,
    device: &Device,
    detail: &Detail,
) -> Vec<(u8, Hint)> {
    let mut hints = match detail.tab {
        DeviceDetailTab::Ports => port_hints(screen),
        DeviceDetailTab::Clients if screen.cursor_client().is_some() => {
            vec![(0, Hint::button("Enter", "client", KeyCode::Enter))]
        }
        _ => Vec::new(),
    };
    // A tab with its own commands keeps those longest on a narrow bar.
    let behind = if hints.is_empty() { 0 } else { 4 };
    hints.extend(
        command_hints(screen, device)
            .into_iter()
            .map(|(rank, hint)| (rank + behind, hint)),
    );
    if screen.can_remove(device) {
        hints.push((7, Hint::button("F", "remove", click::char_key('F'))));
    }
    hints.push((0, Hint::button("Esc", "close", KeyCode::Esc)));
    hints
}

/// Hints for the commands the port under the cursor can take. `O` shows
/// once the port's profile says which way it goes.
fn port_hints(screen: &DevicesScreen) -> Vec<(u8, Hint)> {
    if screen.cursor_port().is_none() {
        return Vec::new();
    }
    let mut hints = Vec::new();
    if screen.cursor_port_powers() && screen.has_integration() {
        hints.push((1, Hint::button("C", "power cycle", click::char_key('C'))));
    }
    // The settings keys need the Session API the profiles come from.
    if screen.ports_writable() {
        let label = match screen.cursor_poe_mode() {
            Some(PoeMode::Off) => Some("PoE on"),
            Some(PoeMode::Auto | PoeMode::Passive24V | PoeMode::Passthrough) => Some("PoE off"),
            Some(PoeMode::Other) | None => None,
        };
        if let Some(label) = label {
            hints.push((2, Hint::button("O", label, click::char_key('O'))));
        }
        hints.push((3, Hint::button("N", "rename", click::char_key('N'))));
    }
    hints
}

/// Everything the Overview says about `device`: what it is and how it
/// connects, then how it is doing. A value the device has not reported is
/// a dash, so every field keeps its place from one device to the next.
fn overview_fields(screen: &DevicesScreen, device: &Device) -> Vec<Field> {
    let firmware = device.firmware_version.as_ref().map(|version| {
        if device.firmware_updatable {
            format!("{version} {UPDATE_MARKER} update")
        } else {
            version.clone()
        }
    });
    let firmware_color = if device.firmware_updatable {
        theme::warning()
    } else {
        theme::accent_secondary()
    };
    // The controller's count, or the clients listed when it reports none.
    let listed = screen.device_clients(device).len();
    let client_count = device
        .client_count
        .map(|count| usize::try_from(count).unwrap_or(listed).max(listed))
        .or((listed > 0).then_some(listed));
    let stats = &device.stats;
    let load = match (
        stats.load_average_1m,
        stats.load_average_5m,
        stats.load_average_15m,
    ) {
        (Some(one), Some(five), Some(fifteen)) => {
            Some(format!("{one:.2}  {five:.2}  {fifteen:.2}"))
        }
        (Some(one), _, _) => Some(format!("{one:.2}")),
        _ => None,
    };
    let rates = stats.uplink_bandwidth.as_ref().map(|bandwidth| {
        format!(
            "{} / {}",
            bytes_fmt::fmt_rate(bandwidth.tx_bytes_per_sec),
            bytes_fmt::fmt_rate(bandwidth.rx_bytes_per_sec)
        )
    });
    // The Session API dates a device's last contact and the Integration
    // API its last heartbeat: one moment under two names.
    let last_seen = device.last_seen.or(stats.last_heartbeat);

    let mut fields = vec![
        Field::new("State", state_label(device.state))
            .color(theme::text_primary())
            .marker(status_indicator::status_span(device.state)),
        Field::optional("IP", device.ip.map(|ip| ip.to_string())).color(theme::accent_tertiary()),
    ];
    if let Some(ipv6) = &device.wan_ipv6 {
        fields.push(Field::new("WAN IPv6", ipv6.clone()).color(theme::accent_tertiary()));
    }
    fields.extend(grid::ipv6_fields(&device.ipv6_addresses));
    fields.extend([
        Field::new("MAC", device.mac.to_string()).color(theme::accent_tertiary()),
        Field::new("Type", type_label(device.device_type)),
        Field::optional("Uplink", uplink_label(screen, device)),
        Field::optional("Clients", client_count.map(|count| count.to_string())),
        colored("Firmware", firmware, firmware_color),
        Field::optional("Uptime", stats.uptime_secs.map(bytes_fmt::fmt_uptime)),
        percent_field("CPU", stats.cpu_utilization_pct),
        percent_field("Memory", stats.memory_utilization_pct),
        colored("Load", load, theme::text_primary()),
        Field::optional("TX/RX", rates),
        Field::optional("Last seen", last_seen.map(grid::ago)),
        Field::optional("Adopted", device.adopted_at.map(grid::local_time)),
        Field::optional("Serial", device.serial.clone()),
    ]);
    fields
}

/// What `device` connects up through: the upstream device's name and the
/// port on it, or that the link is wireless.
fn uplink_label(screen: &DevicesScreen, device: &Device) -> Option<String> {
    let upstream = screen.devices.iter().find(|other| {
        device
            .uplink_device_mac
            .as_ref()
            .is_some_and(|mac| *mac == other.mac)
            || device
                .uplink_device_id
                .as_ref()
                .is_some_and(|id| *id == other.id)
    });
    let name = match upstream {
        Some(upstream) => device_name(upstream).to_owned(),
        None => device.uplink_device_mac.as_ref()?.to_string(),
    };
    Some(match (device.uplink_medium, device.uplink_port_idx) {
        (Some(UplinkMedium::Wireless), _) => format!("{name} (wireless)"),
        (_, Some(port)) => format!("{name} port {port}"),
        _ => name,
    })
}

/// Green through red as a load percentage climbs.
fn load_color(value: f64) -> Color {
    if value < 50.0 {
        theme::success()
    } else if value < 75.0 {
        theme::accent_secondary()
    } else if value < 90.0 {
        theme::warning()
    } else {
        theme::error()
    }
}

/// A field drawn in `color` where its value is known, and as the muted
/// dash where it is not.
fn colored(label: &'static str, value: Option<String>, color: Color) -> Field {
    let known = value.is_some();
    let field = Field::optional(label, value);
    if known { field.color(color) } else { field }
}

fn percent_field(label: &'static str, value: Option<f64>) -> Field {
    match value {
        Some(value) => Field::new(label, format!("{value:.1}%")).color(load_color(value)),
        None => Field::optional(label, None),
    }
}

fn radio_band_label(freq: f32) -> &'static str {
    if freq < 3.0 {
        "2.4 GHz"
    } else if freq < 5.9 {
        "5 GHz"
    } else if freq < 7.0 {
        "6 GHz"
    } else {
        "60 GHz"
    }
}

/// A percentage colored by how it compares to a fair and a poor level.
fn graded(value: Option<f64>, decimals: usize, fair: f64, poor: f64) -> (String, Color) {
    value.map_or_else(
        || (grid::DASH.to_owned(), theme::text_secondary()),
        |value| {
            let color = if value < fair {
                theme::success()
            } else if value < poor {
                theme::warning()
            } else {
                theme::error()
            };
            (format!("{value:.decimals$}%"), color)
        },
    )
}

fn render_radios_tab(frame: &mut Frame, area: Rect, device: &Device) {
    if device.radios.is_empty() {
        grid::note(frame, area, "This access point reports no radios yet");
        return;
    }
    let dash = || grid::DASH.to_owned();
    let mut lines = vec![Line::from(Span::styled(
        "  Band      Channel    Width      Standard       Ch Util    TX Retry",
        theme::table_header(),
    ))];
    for radio in &device.radios {
        let band = radio_band_label(radio.frequency_ghz);
        let channel = radio.channel.map_or_else(dash, |v| v.to_string());
        let width = radio
            .channel_width_mhz
            .map_or_else(dash, |v| format!("{v} MHz"));
        let standard = radio.wlan_standard.as_deref().unwrap_or(grid::DASH);
        let (util, util_color) = graded(radio.channel_utilization_pct, 0, 30.0, 60.0);
        let (retries, retries_color) = graded(radio.tx_retries_pct, 1, 5.0, 15.0);

        lines.push(Line::from(vec![
            Span::styled(
                format!("  {band:<10}"),
                Style::default()
                    .fg(theme::accent_secondary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{channel:<11}"),
                Style::default().fg(theme::text_primary()),
            ),
            Span::styled(
                format!("{width:<11}"),
                Style::default().fg(theme::text_secondary()),
            ),
            Span::styled(
                format!("{standard:<15}"),
                Style::default().fg(theme::accent_tertiary()),
            ),
            Span::styled(format!("{util:<11}"), Style::default().fg(util_color)),
            Span::styled(retries, Style::default().fg(retries_color)),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), area);
}
