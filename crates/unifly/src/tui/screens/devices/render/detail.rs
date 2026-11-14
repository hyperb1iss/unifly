use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use unifly_api::Device;
use unifly_api::model::UplinkMedium;

use super::grid::{self, Field};
use super::table::command_hints;
use crate::tui::action::DeviceDetailTab;
use crate::tui::screens::devices::DevicesScreen;
use crate::tui::screens::devices::commands::{device_name, port_powers};
use crate::tui::screens::devices::detail::{Detail, tab_label, tabs_for};
use crate::tui::screens::devices::view::{state_label, type_label};
use crate::tui::theme;
use crate::tui::widgets::click::{self, Hint};
use crate::tui::widgets::{bytes_fmt, status_indicator, sub_tabs};

/// Pane height from which the tab strip gets a blank row above and below.
const ROOMY: u16 = 14;

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

    let spacer = u16::from(inner.height >= ROOMY);
    let layout = Layout::vertical([
        Constraint::Length(spacer),
        Constraint::Length(1),
        Constraint::Length(spacer),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(inner);

    render_detail_tabs(screen, frame, layout[1], device, detail);
    let content = layout[3];
    match detail.tab {
        DeviceDetailTab::Overview => render_overview_tab(screen, frame, content, device),
        DeviceDetailTab::Performance => render_performance_tab(frame, content, device),
        DeviceDetailTab::Radios => render_radios_tab(frame, content, device),
        DeviceDetailTab::Clients => super::clients::render(screen, frame, content, device, detail),
        DeviceDetailTab::Ports => super::ports::render(screen, frame, content, device, detail),
    }
    render_detail_hints(screen, frame, layout[4], device, detail);
}

/// Where the cursor is in the open tab's list, such as `port 3 of 24`.
fn position_marker(screen: &DevicesScreen, detail: &Detail) -> Option<String> {
    let noun = match detail.tab {
        DeviceDetailTab::Ports => "port",
        DeviceDetailTab::Clients => "client",
        _ => return None,
    };
    let len = screen.detail_list_len();
    (len > 0).then(|| format!(" {noun} {} of {len} ", detail.cursor_index() + 1))
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
    let mut strip = click::ClickLine::default();
    strip.push_gap(Span::raw(" "));
    strip.append(sub_tabs::sub_tab_strip(&labels, active));
    let line = strip.draw(area, &mut screen.hits.borrow_mut());
    frame.render_widget(Paragraph::new(line), area);
}

/// The detail's hint bar: what movement does on this tab, the tab's own
/// commands, then the device commands its state allows.
fn render_detail_hints(
    screen: &DevicesScreen,
    frame: &mut Frame,
    area: Rect,
    device: &Device,
    detail: &Detail,
) {
    let mut hints = vec![(0, Hint::info("h/l", "tabs"))];
    match detail.tab {
        DeviceDetailTab::Ports => {
            hints.push((0, Hint::info("j/k", "port")));
            hints.extend(port_hints(screen));
        }
        DeviceDetailTab::Clients => {
            hints.push((0, Hint::info("j/k", "client")));
            if screen.cursor_client().is_some() {
                hints.push((1, Hint::button("Enter", "open client", KeyCode::Enter)));
            }
        }
        _ => hints.push((2, Hint::info("j/k", "device"))),
    }
    let own_commands = hints.len() > 2;
    hints.extend(command_hints(device).into_iter().map(|(rank, hint)| {
        // A tab with its own commands shows those first.
        (if own_commands { rank + 4 } else { rank }, hint)
    }));
    hints.push((7, Hint::button("F", "forget", click::char_key('F'))));
    hints.push((0, Hint::button("Esc", "close", KeyCode::Esc)));
    let hints = grid::fit_hints(hints, area.width);
    let line = click::key_hints(&hints).draw(area, &mut screen.hits.borrow_mut());
    frame.render_widget(Paragraph::new(line), area);
}

/// Hints for the commands the port under the cursor can take.
fn port_hints(screen: &DevicesScreen) -> Vec<(u8, Hint)> {
    let Some((_, port)) = screen.cursor_port() else {
        return Vec::new();
    };
    let mut hints = Vec::new();
    if port_powers(port) {
        hints.push((1, Hint::button("C", "power cycle", click::char_key('C'))));
        hints.push((2, Hint::button("O", "PoE off", click::char_key('O'))));
    } else if port.poe.is_some() {
        hints.push((2, Hint::button("O", "PoE on", click::char_key('O'))));
    }
    hints.push((3, Hint::button("N", "rename", click::char_key('N'))));
    hints
}

fn render_overview_tab(screen: &DevicesScreen, frame: &mut Frame, area: Rect, device: &Device) {
    let firmware = device.firmware_version.as_ref().map(|version| {
        if device.firmware_updatable {
            format!("{version} (update available)")
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

    let mut fields = vec![
        Field::new("State", state_label(device.state))
            .color(theme::text_primary())
            .marker(status_indicator::status_span(device.state)),
        Field::optional("IP", device.ip.map(|ip| ip.to_string())).color(theme::accent_tertiary()),
        Field::optional(
            "Uptime",
            device.stats.uptime_secs.map(bytes_fmt::fmt_uptime),
        ),
        if firmware.is_some() {
            Field::optional("Firmware", firmware).color(firmware_color)
        } else {
            Field::optional("Firmware", None)
        },
        Field::optional("Uplink", uplink_label(screen, device)),
        Field::optional("Clients", client_count.map(|count| count.to_string())),
        Field::new("Type", type_label(device.device_type)),
        Field::new("MAC", device.mac.to_string()).color(theme::accent_tertiary()),
        Field::optional("Serial", device.serial.clone()),
        Field::optional("Last seen", device.last_seen.map(grid::ago)),
        Field::optional("Adopted", device.adopted_at.map(grid::local_time)),
    ];
    if let Some(ipv6) = &device.wan_ipv6 {
        fields.push(Field::new("WAN IPv6", ipv6.clone()).color(theme::accent_tertiary()));
    }
    frame.render_widget(
        Paragraph::new(grid::lines(&fields, area.width, area.height)),
        area,
    );
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

fn percent_field(label: &'static str, value: Option<f64>) -> Field {
    match value {
        Some(value) => Field::new(label, format!("{value:.1}%")).color(load_color(value)),
        None => Field::optional(label, None),
    }
}

fn render_performance_tab(frame: &mut Frame, area: Rect, device: &Device) {
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
    let rate = |pick: fn(&unifly_api::model::Bandwidth) -> u64| {
        stats
            .uplink_bandwidth
            .as_ref()
            .map(|bandwidth| bytes_fmt::fmt_rate(pick(bandwidth)))
    };
    let fields = [
        percent_field("CPU", stats.cpu_utilization_pct),
        percent_field("Memory", stats.memory_utilization_pct),
        Field::optional("Load", load).color(theme::text_primary()),
        Field::optional("Uplink TX", rate(|bandwidth| bandwidth.tx_bytes_per_sec)),
        Field::optional("Uplink RX", rate(|bandwidth| bandwidth.rx_bytes_per_sec)),
        Field::optional("Heartbeat", stats.last_heartbeat.map(grid::ago)),
    ];
    frame.render_widget(
        Paragraph::new(grid::lines(&fields, area.width, area.height)),
        area,
    );
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
