mod empty;
mod panels;
mod tables;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use unifly_api::DeviceState;

use crate::tui::action::{Action, WifiBand, WifiSubTab};
use crate::tui::theme;
use crate::tui::widgets::click::{self, Click, ClickLine};
use crate::tui::widgets::wrap::wrap;

use super::{OverviewPanel, WifiScreen};

/// Panel size from which a detail sits beside its table instead of over it.
const SIDE_PANEL_MIN: (u16, u16) = (110, 18);

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
            WifiSubTab::Ssids => self.render_ssids(frame, layout[2]),
            WifiSubTab::Neighbors => self.render_neighbors(frame, layout[2]),
            WifiSubTab::Roaming => self.render_roaming(frame, layout[2]),
        }

        // A hint that would be cut mid-word is left off, with the rest.
        let hints = self.hints();
        let bar =
            |count: usize| click::key_hints_styled(&hints[..count], key_style(), hint_style());
        let fitting = (1..=hints.len())
            .rev()
            .find(|count| bar(*count).width() <= usize::from(layout[3].width))
            .unwrap_or(1);
        let line = bar(fitting).draw(layout[3], &mut self.hits.borrow_mut());
        frame.render_widget(Paragraph::new(line), layout[3]);
    }

    fn render_overview(&self, frame: &mut Frame, area: Rect) {
        let panel = self.overview_panel;
        self.render_beside(
            frame,
            area,
            panel != OverviewPanel::None,
            |frame, area| self.render_ap_table(frame, area),
            |frame, area| match panel {
                OverviewPanel::Detail => self.render_ap_detail(frame, area),
                OverviewPanel::Channels => self.render_channel_map(frame, area),
                OverviewPanel::None => {}
            },
        );
    }

    fn render_ssids(&self, frame: &mut Frame, area: Rect) {
        self.render_beside(
            frame,
            area,
            self.ssid_detail_open,
            |frame, area| self.render_ssid_table(frame, area),
            |frame, area| self.render_ssid_detail(frame, area),
        );
    }

    /// Draw a table and, when `open`, its panel: beside it where there is
    /// room, and over it otherwise.
    fn render_beside(
        &self,
        frame: &mut Frame,
        area: Rect,
        open: bool,
        table: impl FnOnce(&mut Frame, Rect),
        panel: impl FnOnce(&mut Frame, Rect),
    ) {
        if !open {
            table(frame, area);
            return;
        }
        if area.width >= SIDE_PANEL_MIN.0 && area.height >= SIDE_PANEL_MIN.1 {
            let [left, right] =
                Layout::horizontal([Constraint::Percentage(56), Constraint::Percentage(44)])
                    .areas(area);
            table(frame, left);
            panel(frame, right);
            return;
        }
        table(frame, area);
        let overlay = centered_rect(area, 92, 100);
        // The overlay covers rows registered beneath it.
        self.hits.borrow_mut().push(overlay, Click::Absorb);
        frame.render_widget(Clear, overlay);
        panel(frame, overlay);
    }

    fn render_clients(&self, frame: &mut Frame, area: Rect) {
        if !self.client_detail_open {
            self.render_client_table(frame, area);
            return;
        }
        if area.height >= 16 {
            let [top, bottom] =
                Layout::vertical([Constraint::Percentage(58), Constraint::Percentage(42)])
                    .areas(area);
            self.render_client_table(frame, top);
            self.render_client_detail(frame, bottom);
            return;
        }
        self.render_client_table(frame, area);
        let overlay = centered_rect(area, 92, 100);
        self.hits.borrow_mut().push(overlay, Click::Absorb);
        frame.render_widget(Clear, overlay);
        self.render_client_detail(frame, overlay);
    }

    fn tab_strip(&self) -> ClickLine<'static, Click> {
        let mut strip = ClickLine::default();
        for (index, tab) in WifiSubTab::ALL.iter().enumerate() {
            if index > 0 {
                strip.push_gap(Span::raw("  "));
            }
            let label = tab.label();
            let (text, style) = if *tab == self.sub_tab {
                (
                    format!("[{label}]"),
                    Style::default()
                        .fg(theme::accent_primary())
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                (
                    label.to_string(),
                    Style::default().fg(theme::text_secondary()),
                )
            };
            strip.push(Click::Tab(index), Span::styled(text, style));
        }

        let chip = Style::default()
            .fg(theme::accent_secondary())
            .add_modifier(Modifier::BOLD);
        let muted = Style::default().fg(theme::text_muted());
        // A chip is drawn only where it narrows what the sub-tab lists.
        if self.focus_shown()
            && let Some(focused) = &self.focused_ap
        {
            let clear = Click::Action(Action::WifiFocusAp(None));
            strip.push_gap(Span::styled("   AP ", muted));
            strip.push(clear.clone(), Span::styled(focused.label.clone(), chip));
            strip.push(clear, Span::styled(" \u{2715}", muted));
        }
        if self.sub_tab == WifiSubTab::Roaming
            && let Some(target) = &self.roam_target
        {
            strip.push_gap(Span::styled("   Client ", muted));
            strip.push_gap(Span::styled(target.label.clone(), chip));
        }
        if let Some(query) = self.active_query().filter(|query| !query.is_empty()) {
            let clear = Click::Action(Action::CloseSearch);
            strip.push_gap(Span::styled("   Filter ", muted));
            strip.push(
                clear.clone(),
                Span::styled(
                    format!("\"{query}\""),
                    Style::default().fg(theme::warning()),
                ),
            );
            strip.push(clear, Span::styled(" \u{2715}", muted));
        }
        strip
    }

    /// One line on the state of the site's access points. It claims
    /// nothing the data does not show: an AP with no clients has no score.
    fn health_banner(&self) -> Line<'static> {
        let (message, color) = self.health_summary();
        let style = match color {
            Some(color) => Style::default()
                .fg(theme::bg_base())
                .bg(color)
                .add_modifier(Modifier::BOLD),
            None => Style::default().fg(theme::text_muted()),
        };
        Line::from(Span::styled(format!(" {message} "), style))
    }

    pub(super) fn health_summary(&self) -> (String, Option<Color>) {
        if !self.loaded.devices {
            return ("Waiting for access point data".to_owned(), None);
        }
        let total = self.all_aps().count();
        if total == 0 {
            return ("No access points on this site".to_owned(), None);
        }

        let down = self
            .all_aps()
            .filter(|ap| ap.state != DeviceState::Online)
            .count();
        let scores: Vec<u8> = self
            .all_aps()
            .filter(|ap| ap.state == DeviceState::Online)
            .filter_map(|ap| self.ap_health(ap))
            .collect();
        let poor = scores.iter().filter(|score| **score < 50).count();
        let fair = scores
            .iter()
            .filter(|score| (50..80).contains(*score))
            .count();

        let mut parts = Vec::new();
        if down > 0 {
            parts.push(format!("{down} not online"));
        }
        if poor > 0 {
            parts.push(format!("{poor} with poor client health"));
        }
        match fair {
            0 => {}
            1 => parts.push("1 needs attention".to_owned()),
            fair => parts.push(format!("{fair} need attention")),
        }
        let aps = if total == 1 {
            "access point"
        } else {
            "access points"
        };
        if !parts.is_empty() {
            let color = if down > 0 || poor > 0 {
                theme::error()
            } else {
                theme::warning()
            };
            return (
                format!("\u{25CF} {total} {aps}: {}", parts.join(", ")),
                Some(color),
            );
        }
        if scores.is_empty() {
            return (
                format!("{total} {aps} online, no client health to report yet"),
                None,
            );
        }
        (
            format!("\u{25CF} {total} {aps} online, client health good"),
            Some(theme::success()),
        )
    }
}

/// One muted line, centered in `area`, for a panel with nothing to list.
/// It wraps when the panel is narrower than the sentence.
fn render_notice(frame: &mut Frame, area: Rect, text: &str) {
    if area.is_empty() {
        return;
    }
    let width = usize::from(area.width.saturating_sub(2)).max(1);
    let lines = wrap(text, width);
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .min(area.height);
    let centered = Rect {
        y: area.y + (area.height - height) / 2,
        height,
        ..area
    };
    let lines: Vec<Line> = lines.into_iter().map(Line::from).collect();
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme::text_muted())),
        centered,
    );
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

fn health_color(value: Option<u8>) -> Color {
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

/// What a device that is not online is doing instead.
fn state_label(state: DeviceState) -> &'static str {
    match state {
        DeviceState::Online => "online",
        DeviceState::Offline => "offline",
        DeviceState::PendingAdoption => "pending",
        DeviceState::Updating => "updating",
        DeviceState::GettingReady => "starting",
        DeviceState::Adopting => "adopting",
        DeviceState::Deleting => "removing",
        DeviceState::ConnectionInterrupted => "interrupted",
        DeviceState::Isolated => "isolated",
        _ => "unknown",
    }
}

fn state_color(state: DeviceState) -> Color {
    match state {
        DeviceState::Online => theme::success(),
        DeviceState::Offline | DeviceState::ConnectionInterrupted | DeviceState::Isolated => {
            theme::error()
        }
        _ => theme::warning(),
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
    band.label()
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

fn signal_color(signal: Option<i32>) -> Color {
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
