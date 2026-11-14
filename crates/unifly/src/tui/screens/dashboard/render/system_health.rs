use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use unifly_api::{DeviceType, HealthSummary};

use crate::tui::screens::events::render::clip;
use crate::tui::theme;
use crate::tui::widgets::bytes_fmt;

use super::super::DashboardScreen;
use super::super::focus::Panel;
use super::super::helpers::fmt_rate_compact;
use super::muted_line;

/// The subsystems the controller reports health for, in display order.
const SUBSYSTEMS: [&str; 5] = ["wan", "www", "wlan", "lan", "vpn"];

/// A subsystem's traffic in both directions, when its health carries one.
fn rates(health: Option<&HealthSummary>) -> Option<(u64, u64)> {
    let health = health?;
    (health.tx_bytes_r.is_some() || health.rx_bytes_r.is_some()).then(|| {
        (
            health.tx_bytes_r.unwrap_or(0),
            health.rx_bytes_r.unwrap_or(0),
        )
    })
}

impl DashboardScreen {
    fn subsystem(&self, name: &str) -> Option<&HealthSummary> {
        self.health.iter().find(|health| health.subsystem == name)
    }

    /// A subsystem's state as a word and its color.
    fn status(&self, name: &str) -> (&'static str, ratatui::style::Color) {
        match self.subsystem(name).map(|health| health.status.as_str()) {
            Some("ok") => ("ok", theme::success()),
            Some("warn" | "warning") => ("warn", theme::warning()),
            Some("error") => ("err", theme::error()),
            _ => ("─", theme::border_unfocused()),
        }
    }

    /// The subsystem states packed into rows of `width`: each with its
    /// state spelled out when that takes two rows at most, else as its
    /// colored dot alone. A subsystem is never cut at the panel's edge.
    fn status_lines(&self, width: usize) -> Vec<Line<'static>> {
        let pack = |worded: bool| {
            let mut rows: Vec<Vec<Span<'static>>> = Vec::new();
            let mut used = usize::MAX;
            for name in SUBSYSTEMS {
                let (word, color) = self.status(name);
                let state = if worded {
                    format!(" ● {word}")
                } else {
                    " ●".to_owned()
                };
                let cells = name.len() + Span::raw(state.as_str()).width();
                if used.saturating_add(2 + cells) > width {
                    rows.push(vec![Span::raw(" ")]);
                    used = 1;
                } else if let Some(row) = rows.last_mut() {
                    row.push(Span::raw("  "));
                    used += 2;
                }
                if let Some(row) = rows.last_mut() {
                    row.push(Span::styled(
                        name.to_uppercase(),
                        Style::default().fg(theme::text_secondary()),
                    ));
                    row.push(Span::styled(state, Style::default().fg(color)));
                }
                used += cells;
            }
            rows
        };
        let worded = pack(true);
        let rows = if worded.len() <= 2 {
            worded
        } else {
            pack(false)
        };
        rows.into_iter().map(Line::from).collect()
    }

    /// Connectivity card - subsystem state and per-subsystem activity.
    pub(super) fn render_system_health(
        &self,
        frame: &mut ratatui::Frame,
        area: ratatui::layout::Rect,
    ) {
        let block = self.panel_block(Panel::Connectivity, "Connectivity");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let w = usize::from(inner.width);
        let content_w = w.saturating_sub(8);
        let mut lines = self.status_lines(w);

        if self.health.is_empty() {
            let why = if self.access.is_some_and(|access| !access.session) {
                "Site health needs a controller session"
            } else {
                self.empty_text("No health reported yet")
            };
            lines.push(muted_line(why));
        }

        // Only subsystems that report a rate get a bar: a missing one is
        // unknown, not idle.
        let rated: Vec<(&str, &str, (u64, u64))> =
            [("wan", "WAN"), ("wlan", "WLAN"), ("lan", "LAN")]
                .into_iter()
                .filter_map(|(name, label)| Some((name, label, rates(self.subsystem(name))?)))
                .collect();
        let max_total = rated
            .iter()
            .map(|(_, _, (tx, rx))| tx.saturating_add(*rx))
            .max()
            .unwrap_or(0);
        // Label, bar, gap, and the widest rate, which is never cut.
        let rate_width = rated
            .iter()
            .map(|(_, _, (tx, rx))| fmt_rate_compact(tx.saturating_add(*rx)).len())
            .max()
            .unwrap_or(0);
        let bar_width =
            u16::try_from(w.saturating_sub(6 + 1 + rate_width + 1).min(14)).unwrap_or(0);
        for (name, label, (tx, rx)) in &rated {
            let total = tx.saturating_add(*rx);
            let (_, color) = self.status(name);
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {label:<5}"),
                    Style::default().fg(theme::text_secondary()),
                ),
                Span::styled(
                    bytes_fmt::fmt_traffic_bar(total, max_total, bar_width),
                    Style::default().fg(color),
                ),
                Span::raw(" "),
                Span::styled(
                    fmt_rate_compact(total),
                    Style::default()
                        .fg(theme::accent_secondary())
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        if !rated.is_empty() {
            let (tx, rx) = rated.iter().fold((0_u64, 0_u64), |(tx, rx), (_, _, link)| {
                (tx.saturating_add(link.0), rx.saturating_add(link.1))
            });
            let aggregate = format!("↑{}  ↓{}", fmt_rate_compact(tx), fmt_rate_compact(rx));
            lines.push(Line::from(vec![
                Span::styled(" Total", Style::default().fg(theme::text_secondary())),
                Span::styled(
                    format!(" {}", clip(&aggregate, content_w)),
                    Style::default().fg(theme::info()),
                ),
            ]));
        }

        // Health counts adopted devices; without it the device list does.
        let adopted = |subsystem: &str, device_type: DeviceType| {
            self.subsystem(subsystem)
                .and_then(|health| health.num_adopted)
                .map_or_else(
                    || {
                        self.devices
                            .iter()
                            .filter(|device| device.device_type == device_type)
                            .count()
                            .to_string()
                    },
                    |count| count.to_string(),
                )
        };
        let infra = format!(
            "AP {} · SW {}",
            adopted("wlan", DeviceType::AccessPoint),
            adopted("lan", DeviceType::Switch)
        );
        lines.push(Line::from(vec![
            Span::styled(" Infra", Style::default().fg(theme::text_secondary())),
            Span::styled(
                format!(" {}", clip(&infra, content_w)),
                Style::default().fg(theme::accent_secondary()),
            ),
        ]));

        frame.render_widget(Paragraph::new(lines), inner);
    }
}
