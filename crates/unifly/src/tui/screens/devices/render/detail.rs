use std::sync::Arc;

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use unifly_api::{Client, Device};

use crate::tui::action::DeviceDetailTab;
use crate::tui::theme;
use crate::tui::widgets::click::{self, Hint};
use crate::tui::widgets::hyperchart::SwitchFaceplate;
use crate::tui::widgets::{bytes_fmt, status_indicator, sub_tabs};

use crate::tui::screens::devices::DevicesScreen;

pub(super) fn render_detail(
    screen: &DevicesScreen,
    frame: &mut Frame,
    area: Rect,
    device: &Device,
) {
    let name = device.name.as_deref().unwrap_or("Unknown");
    let model = device.model.as_deref().unwrap_or("─");
    let ip = device
        .ip
        .map_or_else(|| "─".into(), |device_ip| device_ip.to_string());
    let mac = device.mac.to_string();

    let title = format!(" {name}  ·  {model}  ·  {ip}  ·  {mac} ");
    let block = Block::default()
        .title(title)
        .title_style(theme::title_style())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border_focused());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let tabs_layout = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);

    render_detail_tabs(screen, frame, tabs_layout[0]);

    match screen.detail_tab {
        DeviceDetailTab::Overview => render_overview_tab(frame, tabs_layout[1], device),
        DeviceDetailTab::Performance => render_performance_tab(frame, tabs_layout[1], device),
        DeviceDetailTab::Radios => render_radios_tab(frame, tabs_layout[1], device),
        DeviceDetailTab::Clients => {
            let clients = screen.device_clients(device);
            render_clients_tab(frame, tabs_layout[1], device, &clients);
        }
        DeviceDetailTab::Ports => render_ports_tab(frame, tabs_layout[1], device),
    }

    render_detail_hints(screen, frame, tabs_layout[2]);
}

fn render_detail_tabs(screen: &DevicesScreen, frame: &mut Frame, area: Rect) {
    let tab_labels = &["Overview", "Performance", "Radios", "Clients", "Ports"];
    let active_idx = DeviceDetailTab::ALL
        .iter()
        .position(|tab| *tab == screen.detail_tab)
        .unwrap_or_default();
    let strip = sub_tabs::sub_tab_strip(tab_labels, active_idx);
    // The tabs sit on the second row, under a spacer line.
    let tab_row = Rect {
        y: area.y.saturating_add(1),
        height: area.height.saturating_sub(1),
        ..area
    };
    let line = strip.draw(tab_row, &mut screen.hits.borrow_mut());
    frame.render_widget(Paragraph::new(vec![Line::from(""), line]), area);
}

fn render_detail_hints(screen: &DevicesScreen, frame: &mut Frame, area: Rect) {
    let hints = click::key_hints(&[
        Hint::info("h/l", "tabs"),
        Hint::button("R", "restart", click::char_key('R')),
        Hint::button("L", "locate", click::char_key('L')),
        Hint::button("U", "upgrade", click::char_key('U')),
        Hint::button("Esc", "back", KeyCode::Esc),
    ]);
    let line = hints.draw(area, &mut screen.hits.borrow_mut());
    frame.render_widget(Paragraph::new(line), area);
}

fn render_overview_tab(frame: &mut Frame, area: Rect, device: &Device) {
    let state_span = status_indicator::status_span(device.state);
    let state_label = format!("{:?}", device.state);
    let firmware = device.firmware_version.as_deref().unwrap_or("─");
    let fw_status = if device.firmware_updatable {
        "update available"
    } else {
        "up to date"
    };
    let uptime = device
        .stats
        .uptime_secs
        .map_or_else(|| "─".into(), bytes_fmt::fmt_uptime);
    let adopted = device.adopted_at.map_or_else(
        || "─".into(),
        |dt| dt.format("%Y-%m-%d %H:%M UTC").to_string(),
    );

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  State          ",
                Style::default().fg(theme::text_secondary()),
            ),
            state_span,
            Span::styled(
                format!(" {state_label}"),
                Style::default().fg(theme::text_secondary()),
            ),
            Span::styled(
                "       Adopted     ",
                Style::default().fg(theme::text_secondary()),
            ),
            Span::styled(adopted, Style::default().fg(theme::accent_secondary())),
        ]),
        Line::from(vec![
            Span::styled(
                "  Firmware       ",
                Style::default().fg(theme::text_secondary()),
            ),
            Span::styled(
                format!("{firmware} ({fw_status})"),
                Style::default().fg(theme::accent_secondary()),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  Uptime         ",
                Style::default().fg(theme::text_secondary()),
            ),
            Span::styled(uptime, Style::default().fg(theme::accent_secondary())),
        ]),
        Line::from(vec![
            Span::styled(
                "  MAC            ",
                Style::default().fg(theme::text_secondary()),
            ),
            Span::styled(
                device.mac.to_string(),
                Style::default().fg(theme::accent_tertiary()),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  Type           ",
                Style::default().fg(theme::text_secondary()),
            ),
            Span::styled(
                format!("{:?}", device.device_type),
                Style::default().fg(theme::text_secondary()),
            ),
        ]),
    ];

    frame.render_widget(Paragraph::new(lines), area);
}

fn render_performance_tab(frame: &mut Frame, area: Rect, device: &Device) {
    let cpu = device
        .stats
        .cpu_utilization_pct
        .map_or_else(|| "─".into(), |value| format!("{value:.1}%"));
    let mem = device
        .stats
        .memory_utilization_pct
        .map_or_else(|| "─".into(), |value| format!("{value:.1}%"));
    let load = device
        .stats
        .load_average_1m
        .map_or_else(|| "─".into(), |value| format!("{value:.2}"));

    let cpu_color = device
        .stats
        .cpu_utilization_pct
        .map_or(theme::text_secondary(), |value| {
            if value < 50.0 {
                theme::success()
            } else if value < 75.0 {
                theme::accent_secondary()
            } else if value < 90.0 {
                theme::warning()
            } else {
                theme::error()
            }
        });

    let mem_color = device
        .stats
        .memory_utilization_pct
        .map_or(theme::text_secondary(), |value| {
            if value < 50.0 {
                theme::success()
            } else if value < 75.0 {
                theme::accent_secondary()
            } else if value < 90.0 {
                theme::warning()
            } else {
                theme::error()
            }
        });

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  CPU     ", Style::default().fg(theme::text_secondary())),
            Span::styled(cpu, Style::default().fg(cpu_color)),
        ]),
        Line::from(vec![
            Span::styled("  Memory  ", Style::default().fg(theme::text_secondary())),
            Span::styled(mem, Style::default().fg(mem_color)),
        ]),
        Line::from(vec![
            Span::styled("  Load    ", Style::default().fg(theme::text_secondary())),
            Span::styled(load, Style::default().fg(theme::text_secondary())),
        ]),
    ];

    frame.render_widget(Paragraph::new(lines), area);
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

fn render_radios_tab(frame: &mut Frame, area: Rect, device: &Device) {
    let mut lines = vec![Line::from("")];

    if device.radios.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No radio data available",
            Style::default().fg(theme::border_unfocused()),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "  Band      Channel    Width      Standard       Ch Util    TX Retry",
            theme::table_header(),
        )));

        for radio in &device.radios {
            let band = radio_band_label(radio.frequency_ghz);
            let channel = radio.channel.map_or_else(|| "─".into(), |v| v.to_string());
            let width = radio
                .channel_width_mhz
                .map_or_else(|| "─".into(), |v| format!("{v} MHz"));
            let standard = radio.wlan_standard.as_deref().unwrap_or("─");

            let util = radio
                .channel_utilization_pct
                .map_or_else(|| "─".into(), |v| format!("{v:.0}%"));
            let util_color = radio
                .channel_utilization_pct
                .map_or(theme::text_secondary(), |v| {
                    if v < 30.0 {
                        theme::success()
                    } else if v < 60.0 {
                        theme::warning()
                    } else {
                        theme::error()
                    }
                });

            let retries = radio
                .tx_retries_pct
                .map_or_else(|| "─".into(), |v| format!("{v:.1}%"));
            let retries_color = radio.tx_retries_pct.map_or(theme::text_secondary(), |v| {
                if v < 5.0 {
                    theme::success()
                } else if v < 15.0 {
                    theme::warning()
                } else {
                    theme::error()
                }
            });

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
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn render_clients_tab(frame: &mut Frame, area: Rect, device: &Device, clients: &[&Arc<Client>]) {
    let mut lines = vec![Line::from("")];

    let count = device.client_count.unwrap_or(0);
    lines.push(Line::from(vec![
        Span::styled(
            "  Connected: ",
            Style::default().fg(theme::text_secondary()),
        ),
        Span::styled(
            count.to_string(),
            Style::default()
                .fg(theme::accent_secondary())
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(""));

    if clients.is_empty() {
        if count > 0 {
            lines.push(Line::from(Span::styled(
                "  Client details not yet available",
                Style::default().fg(theme::border_unfocused()),
            )));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "  Name                 IP               MAC                Type      Signal",
            theme::table_header(),
        )));

        for client in clients {
            let name = client
                .name
                .as_deref()
                .or(client.hostname.as_deref())
                .unwrap_or("─");
            let display_name: String = {
                let chars: Vec<char> = name.chars().collect();
                if chars.len() > 20 {
                    format!("{}…", chars[..19].iter().collect::<String>())
                } else {
                    name.to_owned()
                }
            };
            let ip = client
                .ip
                .map_or_else(|| "─".into(), |addr| addr.to_string());
            let mac = client.mac.to_string();
            let client_type = match client.client_type {
                unifly_api::model::ClientType::Wired => "Wired",
                unifly_api::model::ClientType::Wireless => "WiFi",
                unifly_api::model::ClientType::Vpn => "VPN",
                unifly_api::model::ClientType::Teleport => "Teleport",
                _ => "─",
            };

            let signal = client
                .wireless
                .as_ref()
                .and_then(|w| w.signal_dbm)
                .map_or_else(
                    || Span::styled("─", Style::default().fg(theme::text_secondary())),
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
                        Span::styled(format!("{dbm} dBm"), Style::default().fg(color))
                    },
                );

            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {display_name:<21}"),
                    Style::default().fg(theme::text_primary()),
                ),
                Span::styled(
                    format!("{ip:<17}"),
                    Style::default().fg(theme::accent_tertiary()),
                ),
                Span::styled(
                    format!("{mac:<19}"),
                    Style::default().fg(theme::text_secondary()),
                ),
                Span::styled(
                    format!("{client_type:<10}"),
                    Style::default().fg(theme::text_secondary()),
                ),
                signal,
            ]));
        }
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn fmt_speed(mbps: Option<u32>) -> String {
    mbps.map_or_else(
        || "─".into(),
        |v| {
            if v >= 1000 {
                format!("{}G", v / 1000)
            } else {
                format!("{v}M")
            }
        },
    )
}

fn fmt_connector(c: Option<&unifly_api::model::PortConnector>) -> &'static str {
    match c {
        Some(unifly_api::model::PortConnector::Rj45) => "RJ45",
        Some(unifly_api::model::PortConnector::Sfp) => "SFP",
        Some(unifly_api::model::PortConnector::SfpPlus) => "SFP+",
        Some(unifly_api::model::PortConnector::Sfp28) => "SFP28",
        Some(unifly_api::model::PortConnector::Qsfp28) => "QSFP28",
        None => "─",
    }
}

fn render_ports_tab(frame: &mut Frame, area: Rect, device: &Device) {
    if !device.ports.is_empty() && area.width >= 44 && area.height >= 9 {
        let layout = Layout::vertical([Constraint::Length(6), Constraint::Min(1)]).split(area);
        frame.render_widget(SwitchFaceplate::new(&device.ports), layout[0]);
        render_ports_table(frame, layout[1], device);
        return;
    }

    render_ports_table(frame, area, device);
}

fn render_ports_table(frame: &mut Frame, area: Rect, device: &Device) {
    let mut lines = vec![Line::from("")];

    if device.ports.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No port data available",
            Style::default().fg(theme::border_unfocused()),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "  Port    State   Speed   Max     Type    PoE",
            theme::table_header(),
        )));

        for port in &device.ports {
            let idx_str = port.index.to_string();
            let name = port.name.as_deref().unwrap_or(&idx_str);
            let state_color = match port.state {
                unifly_api::model::PortState::Up => theme::success(),
                unifly_api::model::PortState::Down => theme::error(),
                unifly_api::model::PortState::Unknown => theme::text_secondary(),
            };
            let state_str = format!("{:?}", port.state);
            let speed = fmt_speed(port.speed_mbps);
            let max_speed = fmt_speed(port.max_speed_mbps);
            let connector = fmt_connector(port.connector.as_ref());

            let poe_span = match &port.poe {
                Some(poe) if poe.enabled => {
                    let label = poe.standard.as_deref().unwrap_or("PoE");
                    let poe_color = match poe.state {
                        unifly_api::model::PortState::Up => theme::success(),
                        _ => theme::warning(),
                    };
                    Span::styled(format!("✓ {label}"), Style::default().fg(poe_color))
                }
                Some(_) => Span::styled("✗", Style::default().fg(theme::text_secondary())),
                None => Span::styled("─", Style::default().fg(theme::text_secondary())),
            };

            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {name:<8}"),
                    Style::default().fg(theme::accent_secondary()),
                ),
                Span::styled(format!("{state_str:<8}"), Style::default().fg(state_color)),
                Span::styled(
                    format!("{speed:<8}"),
                    Style::default().fg(theme::text_primary()),
                ),
                Span::styled(
                    format!("{max_speed:<8}"),
                    Style::default().fg(theme::text_secondary()),
                ),
                Span::styled(
                    format!("{connector:<8}"),
                    Style::default().fg(theme::text_secondary()),
                ),
                poe_span,
            ]));
        }
    }

    frame.render_widget(Paragraph::new(lines), area);
}
