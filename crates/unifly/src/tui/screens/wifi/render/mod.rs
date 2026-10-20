mod tables;

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};

use crate::tui::action::{WifiBand, WifiSubTab};
use crate::tui::theme;
use crate::tui::widgets::click::{self, Click, ClickLine, Hint};
use crate::tui::widgets::hyperchart::{HeatmapCell, HyperHeatmap};
use crate::tui::widgets::signal_bars;

use super::WifiScreen;
use super::state::{display_client_name, display_device_name, json_i32, json_string, json_u64};

impl WifiScreen {
    pub(super) fn render_screen(&self, frame: &mut Frame, area: Rect) {
        self.hits.borrow_mut().clear();
        let block = Block::default()
            .title(" WiFi ")
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(if self.focused {
                theme::border_focused()
            } else {
                theme::border_default()
            });

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let layout = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

        let line = self
            .tab_strip()
            .draw(layout[0], &mut self.hits.borrow_mut());
        frame.render_widget(Paragraph::new(line), layout[0]);
        frame.render_widget(Paragraph::new(self.health_banner()), layout[1]);

        match self.sub_tab {
            WifiSubTab::Overview => self.render_overview(frame, layout[2]),
            WifiSubTab::Clients => self.render_clients(frame, layout[2]),
            WifiSubTab::Neighbors => self.render_neighbors(frame, layout[2]),
            WifiSubTab::Roaming => self.render_roaming(frame, layout[2]),
        }

        let line = self
            .hint_line()
            .draw(layout[3], &mut self.hits.borrow_mut());
        frame.render_widget(Paragraph::new(line), layout[3]);
    }

    fn render_overview(&self, frame: &mut Frame, area: Rect) {
        let can_split = area.width >= 110 && area.height >= 18;
        let show_side_panel = self.detail_open || self.channel_map_open;
        let chunks = if can_split && show_side_panel {
            Layout::horizontal([Constraint::Percentage(56), Constraint::Percentage(44)]).split(area)
        } else {
            Layout::horizontal([Constraint::Percentage(100)]).split(area)
        };

        self.render_ap_table(frame, chunks[0]);
        if can_split && let Some(side_area) = chunks.get(1).copied() {
            if self.detail_open {
                self.render_ap_detail(frame, side_area);
            } else if self.channel_map_open {
                self.render_channel_map(frame, side_area);
            }
        } else if show_side_panel {
            let overlay = centered_rect(area, 82, 78);
            // The overlay covers rows registered beneath it.
            self.hits.borrow_mut().push(overlay, Click::Absorb);
            frame.render_widget(Clear, overlay);
            if self.detail_open {
                self.render_ap_detail(frame, overlay);
            } else if self.channel_map_open {
                self.render_channel_map(frame, overlay);
            }
        }
    }

    fn render_clients(&self, frame: &mut Frame, area: Rect) {
        let can_split = self.detail_open && area.height >= 16;
        let chunks = if can_split {
            Layout::vertical([Constraint::Percentage(58), Constraint::Percentage(42)]).split(area)
        } else {
            Layout::vertical([Constraint::Percentage(100)]).split(area)
        };

        self.render_client_table(frame, chunks[0]);
        if can_split && let Some(detail_area) = chunks.get(1).copied() {
            self.render_client_detail(frame, detail_area);
        } else if self.detail_open {
            let overlay = centered_rect(area, 86, 74);
            self.hits.borrow_mut().push(overlay, Click::Absorb);
            frame.render_widget(Clear, overlay);
            self.render_client_detail(frame, overlay);
        }
    }

    fn render_ap_detail(&self, frame: &mut Frame, area: Rect) {
        let block = panel_block(" AP Detail ", self.focused);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let Some(ap) = self.selected_ap() else {
            frame.render_widget(
                Paragraph::new(" No AP selected ").style(Style::default().fg(theme::text_muted())),
                inner,
            );
            return;
        };

        let health = self.ap_health(ap).unwrap_or_default();
        let signal = self.ap_signal(ap);
        let summary = health_summary(health);
        let mut lines = vec![
            Line::from(vec![
                Span::styled(display_device_name(ap), theme::title_style()),
                Span::styled(
                    format!("  {} clients", self.ap_clients(ap).len()),
                    Style::default().fg(theme::text_secondary()),
                ),
            ]),
            Line::from(vec![
                Span::styled(" Health ", Style::default().fg(theme::text_muted())),
                Span::styled(
                    gauge_fill(health, 18),
                    Style::default().fg(health_color(Some(health))),
                ),
                Span::styled(
                    format!(" {health}/100"),
                    Style::default().fg(theme::text_secondary()),
                ),
            ]),
            Line::from(vec![
                Span::styled(" Signal ", Style::default().fg(theme::text_muted())),
                signal_bars::signal_span(signal),
                Span::styled(
                    format!(
                        "  {}",
                        signal.map_or_else(|| "─".to_string(), |dbm| format!("{dbm} dBm"))
                    ),
                    Style::default().fg(theme::text_secondary()),
                ),
            ]),
            Line::from(vec![Span::styled(
                summary.0,
                Style::default().fg(summary.1),
            )]),
            Line::from(""),
            Line::from(Span::styled(" Clients", theme::table_header())),
        ];

        for client in self.ap_clients(ap).iter().take(6) {
            let signal = client
                .wireless
                .as_ref()
                .and_then(|wireless| wireless.signal_dbm);
            let health = client
                .wireless
                .as_ref()
                .and_then(|wireless| wireless.satisfaction)
                .unwrap_or_default();
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {}", display_client_name(client)),
                    Style::default().fg(theme::text_secondary()),
                ),
                Span::styled("  ", Style::default()),
                signal_bars::signal_span(signal),
                Span::styled(
                    format!("  {health}%"),
                    Style::default().fg(health_color(Some(health))),
                ),
            ]));
        }

        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }

    #[allow(clippy::too_many_lines)]
    fn render_client_detail(&self, frame: &mut Frame, area: Rect) {
        let block = panel_block(" WiFi Experience ", self.focused);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let Some(client) = self.selected_client() else {
            frame.render_widget(
                Paragraph::new(" No client selected ")
                    .style(Style::default().fg(theme::text_muted())),
                inner,
            );
            return;
        };

        let Some(detail) = self.client_detail.as_ref() else {
            frame.render_widget(
                Paragraph::new(" Loading WiFi detail... ")
                    .style(Style::default().fg(theme::text_muted())),
                inner,
            );
            return;
        };

        let signal = json_i32(detail, &["signal"]);
        let noise = json_i32(detail, &["noise"]);
        let channel = json_u64(detail, &["channel"]);
        let width = json_u64(detail, &["channel_width"]);
        let band = json_string(detail, &["wlan_band", "band"]).unwrap_or_else(|| "─".to_string());
        let protocol = json_string(detail, &["radio_protocol"]).unwrap_or_else(|| "─".to_string());
        let experience = json_u64(detail, &["experience", "wifi_experience"]);
        let tx = json_u64(detail, &["link_upload_rate_kbps"]);
        let rx = json_u64(detail, &["link_download_rate_kbps"]);

        let mut lines = vec![
            Line::from(vec![
                Span::styled(display_client_name(client), theme::title_style()),
                Span::styled(
                    format!("  {}", self.client_ap_name(client)),
                    Style::default().fg(theme::text_secondary()),
                ),
            ]),
            Line::from(vec![
                Span::styled(" Health ", Style::default().fg(theme::text_muted())),
                Span::styled(
                    experience.map_or_else(|| "─".to_string(), |value| format!("{value}/100")),
                    Style::default().fg(health_color(
                        experience.and_then(|value| u8::try_from(value).ok()),
                    )),
                ),
                Span::styled("  SSID ", Style::default().fg(theme::text_muted())),
                Span::styled(
                    client
                        .wireless
                        .as_ref()
                        .and_then(|wireless| wireless.ssid.clone())
                        .unwrap_or_else(|| "─".to_string()),
                    Style::default().fg(theme::text_secondary()),
                ),
            ]),
            Line::from(vec![
                Span::styled(" Signal ", Style::default().fg(theme::text_muted())),
                signal_bars::signal_span(signal),
                Span::styled(
                    format!(
                        "  {}  Noise {}",
                        signal.map_or_else(|| "─".to_string(), |dbm| format!("{dbm} dBm")),
                        noise.map_or_else(|| "─".to_string(), |dbm| format!("{dbm} dBm"))
                    ),
                    Style::default().fg(theme::text_secondary()),
                ),
            ]),
            Line::from(vec![
                Span::styled(" Radio ", Style::default().fg(theme::text_muted())),
                Span::styled(
                    format!(
                        "{band}  ch {}{}",
                        channel.map_or_else(|| "─".to_string(), |value| value.to_string()),
                        width.map_or_else(String::new, |value| format!(" ({value} MHz)"))
                    ),
                    Style::default().fg(theme::text_secondary()),
                ),
                Span::styled("  Proto ", Style::default().fg(theme::text_muted())),
                Span::styled(protocol, Style::default().fg(theme::text_secondary())),
            ]),
            Line::from(vec![
                Span::styled(" TX/RX ", Style::default().fg(theme::text_muted())),
                Span::styled(
                    format!("{}/{} Mbps", kbps_to_mbps(tx), kbps_to_mbps(rx)),
                    Style::default().fg(theme::text_secondary()),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(" Nearest neighbors", theme::table_header())),
        ];

        for neighbor in detail
            .get("nearest_neighbors")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .take(5)
        {
            let signal = json_i32(neighbor, &["signal"]);
            let label = json_string(neighbor, &["ssid", "display_name", "bssid"])
                .unwrap_or_else(|| "neighbor".to_string());
            let channel = json_u64(neighbor, &["channel"])
                .map_or_else(|| "─".to_string(), |value| value.to_string());
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {label}"),
                    Style::default().fg(theme::text_secondary()),
                ),
                Span::styled(
                    format!(
                        "  ch {channel}  {}",
                        signal.map_or_else(|| "─".to_string(), |dbm| format!("{dbm} dBm"))
                    ),
                    Style::default().fg(theme::text_muted()),
                ),
            ]));
        }

        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }

    fn render_channel_map(&self, frame: &mut Frame, area: Rect) {
        let block = panel_block(" Channels ", self.focused);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let occupancy = self.channel_occupancy();
        if occupancy.is_empty() {
            frame.render_widget(
                Paragraph::new(" Channel data unavailable ")
                    .style(Style::default().fg(theme::text_muted())),
                inner,
            );
            return;
        }

        let cells: Vec<HeatmapCell> = occupancy
            .iter()
            .map(|row| HeatmapCell {
                label: format!("ch{}", row.channel),
                your_count: row.your_count,
                neighbor_count: row.neighbor_count,
                signal: row.signal,
                conflict: row.conflict,
            })
            .collect();

        let layout = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).split(inner);
        let header = Line::from(vec![
            Span::styled(" Band ", Style::default().fg(theme::text_muted())),
            Span::styled(
                band_label(self.selected_band),
                Style::default()
                    .fg(theme::accent_secondary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "  ·  {} APs  {} neighbors",
                    occupancy.iter().map(|row| row.your_count).sum::<usize>(),
                    occupancy
                        .iter()
                        .map(|row| row.neighbor_count)
                        .sum::<usize>()
                ),
                Style::default().fg(theme::text_secondary()),
            ),
        ]);
        frame.render_widget(Paragraph::new(vec![header, Line::from("")]), layout[0]);
        frame.render_widget(
            HyperHeatmap::new(&cells).empty_message(" Channel data unavailable "),
            layout[1],
        );
    }

    fn tab_strip(&self) -> ClickLine<'static, Click> {
        let mut strip = ClickLine::default();
        for (index, tab) in WifiSubTab::ALL.iter().enumerate() {
            if index > 0 {
                strip.push_gap(Span::styled("  ", Style::default().fg(theme::text_muted())));
            }
            let label = tab.label();
            let style = if *tab == self.sub_tab {
                Style::default()
                    .fg(theme::accent_primary())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::text_secondary())
            };
            strip.push(
                Click::Tab(index),
                Span::styled(
                    if *tab == self.sub_tab {
                        format!("[{label}]")
                    } else {
                        label.to_string()
                    },
                    style,
                ),
            );
        }
        if let Some(ap) = self.focused_ap_label() {
            strip.push_gap(Span::styled(
                "  Focused AP: ",
                Style::default().fg(theme::text_muted()),
            ));
            strip.push_gap(Span::styled(
                ap,
                Style::default()
                    .fg(theme::accent_secondary())
                    .add_modifier(Modifier::BOLD),
            ));
        }
        if let Some(client) = self.focused_client_label() {
            strip.push_gap(Span::styled(
                "  Client: ",
                Style::default().fg(theme::text_muted()),
            ));
            strip.push_gap(Span::styled(
                client,
                Style::default()
                    .fg(theme::accent_secondary())
                    .add_modifier(Modifier::BOLD),
            ));
        }
        strip
    }

    fn health_banner(&self) -> Line<'static> {
        let aps = self.ap_devices();
        let poor = aps
            .iter()
            .filter_map(|ap| self.ap_health(ap))
            .filter(|health| *health < 50)
            .count();
        let attention = aps
            .iter()
            .filter_map(|ap| self.ap_health(ap))
            .filter(|health| (50..80).contains(health))
            .count();

        let (message, color) = if poor > 0 {
            (
                format!("● {poor} access point(s) have poor performance"),
                theme::error(),
            )
        } else if attention > 0 {
            (
                format!("● {attention} access point(s) need attention"),
                theme::warning(),
            )
        } else {
            ("● All access points healthy".to_string(), theme::success())
        };

        Line::from(Span::styled(
            format!(" {message} "),
            Style::default()
                .fg(theme::bg_base())
                .bg(color)
                .add_modifier(Modifier::BOLD),
        ))
    }

    fn hint_line(&self) -> ClickLine<'static, Click> {
        let hints = match self.sub_tab {
            WifiSubTab::Overview => vec![
                Hint::info("j/k", "select"),
                Hint::button("Tab", "sub-tab", KeyCode::Tab),
                Hint::button("Enter", "detail", KeyCode::Enter),
                Hint::button("c", "channels", click::char_key('c')),
                Hint::button("R", "restart", click::char_key('R')),
                Hint::button("L", "locate", click::char_key('L')),
            ],
            WifiSubTab::Clients => vec![
                Hint::info("j/k", "select"),
                Hint::button("Enter", "detail", KeyCode::Enter),
                Hint::button("b", "block", click::char_key('b')),
                Hint::button("u", "unblock", click::char_key('u')),
                Hint::button("x", "kick", click::char_key('x')),
            ],
            WifiSubTab::Neighbors => vec![
                Hint::info("j/k", "select"),
                Hint::button("s", "sort", click::char_key('s')),
                Hint::button("f", "filter band", click::char_key('f')),
            ],
            WifiSubTab::Roaming => {
                let mut hints = vec![
                    Hint::info("j/k", "scroll"),
                    Hint::button("r", "refresh", click::char_key('r')),
                ];
                if self.focused_client_id.is_some() {
                    hints.push(Hint::button("Esc", "clear focus", KeyCode::Esc));
                }
                hints
            }
        };
        click::key_hints_styled(&hints, key_style(), hint_style())
    }
}

fn panel_block<'a>(title: impl Into<Line<'a>>, focused: bool) -> Block<'a> {
    Block::default()
        .title(title)
        .title_style(theme::title_style())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(if focused {
            theme::border_focused()
        } else {
            theme::border_default()
        })
}

fn gauge_fill(value: u8, width: usize) -> String {
    let filled = (usize::from(value) * width) / 100;
    let empty = width.saturating_sub(filled);
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

fn health_color(value: Option<u8>) -> ratatui::style::Color {
    match value {
        Some(score) if score >= 80 => theme::success(),
        Some(score) if score >= 50 => theme::warning(),
        Some(_) => theme::error(),
        None => theme::text_muted(),
    }
}

fn health_label(value: Option<u8>) -> String {
    value.map_or_else(|| "─".to_string(), |score| format!("{score}%"))
}

fn health_summary(value: u8) -> (&'static str, ratatui::style::Color) {
    if value >= 80 {
        (
            "Signal: Good  Retries: Low  Satisfaction: High",
            theme::success(),
        )
    } else if value >= 50 {
        (
            "Signal: Fair  Retries: Moderate  Satisfaction: Fair",
            theme::warning(),
        )
    } else {
        (
            "Signal: Poor  Retries: High  Satisfaction: Low",
            theme::error(),
        )
    }
}

fn centered_rect(area: Rect, width_pct: u16, height_pct: u16) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - height_pct) / 2),
        Constraint::Percentage(height_pct),
        Constraint::Percentage((100 - height_pct) / 2),
    ])
    .split(area);

    Layout::horizontal([
        Constraint::Percentage((100 - width_pct) / 2),
        Constraint::Percentage(width_pct),
        Constraint::Percentage((100 - width_pct) / 2),
    ])
    .split(vertical[1])[1]
}

fn band_label(band: WifiBand) -> &'static str {
    match band {
        WifiBand::TwoGhz => "2.4G",
        WifiBand::FiveGhz => "5G",
        WifiBand::SixGhz => "6G",
    }
}

fn signal_text(signal: Option<i32>) -> String {
    match signal {
        Some(dbm) if dbm >= -50 => "▂▄▆█".to_string(),
        Some(dbm) if dbm >= -60 => "▂▄▆ ".to_string(),
        Some(dbm) if dbm >= -70 => "▂▄  ".to_string(),
        Some(dbm) if dbm >= -80 => "▂   ".to_string(),
        Some(_) => "·   ".to_string(),
        None => "····".to_string(),
    }
}

fn signal_color(signal: Option<i32>) -> ratatui::style::Color {
    match signal {
        Some(dbm) if dbm >= -50 => theme::success(),
        Some(dbm) if dbm >= -60 => theme::accent_secondary(),
        Some(dbm) if dbm >= -70 => theme::warning(),
        Some(dbm) if dbm >= -80 => theme::accent_tertiary(),
        Some(_) => theme::error(),
        None => theme::text_muted(),
    }
}

fn kbps_to_mbps(value: Option<u64>) -> String {
    value.map_or_else(|| "─".to_string(), |kbps| ((kbps + 500) / 1000).to_string())
}

fn key_style() -> Style {
    Style::default()
        .fg(theme::accent_secondary())
        .add_modifier(Modifier::BOLD)
}

fn hint_style() -> Style {
    Style::default().fg(theme::text_muted())
}
