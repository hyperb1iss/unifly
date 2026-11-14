use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use unifly_api::ClientType;

use crate::tui::screens::events::render::clip;
use crate::tui::theme;
use crate::tui::widgets::bytes_fmt;

use super::super::DashboardScreen;
use super::super::focus::Panel;

/// A utilization bar and its percentage, or a dash when it is unknown.
fn usage_line(label: &str, pct: Option<f64>, bar_width: u16) -> Line<'static> {
    let name = Span::styled(
        format!(" {label:<4}"),
        Style::default().fg(theme::text_secondary()),
    );
    let Some(pct) = pct else {
        return Line::from(vec![
            name,
            Span::styled("─", Style::default().fg(theme::border_unfocused())),
        ]);
    };
    let color = if pct > 80.0 {
        theme::error()
    } else if pct > 50.0 {
        theme::warning()
    } else {
        theme::accent_secondary()
    };
    let (filled, empty) = bytes_fmt::fmt_pct_bar(pct, bar_width);
    Line::from(vec![
        name,
        Span::styled(filled, Style::default().fg(color)),
        Span::styled(empty, Style::default().fg(theme::border_unfocused())),
        Span::styled(
            format!(" {pct:>5.1}%"),
            Style::default().fg(theme::text_secondary()),
        ),
    ])
}

impl DashboardScreen {
    /// Capacity card - CPU/MEM bars, load averages, and fleet counts.
    pub(super) fn render_capacity(&self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        let block = self.panel_block(Panel::Capacity, "Capacity");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let gateway = self.gateway();
        let w = usize::from(inner.width);
        let content_w = w.saturating_sub(7);
        let bar_width = u16::try_from(content_w.saturating_sub(9).clamp(6, 18)).unwrap_or(6);

        let mut lines = vec![
            usage_line(
                "CPU",
                gateway.and_then(|g| g.stats.cpu_utilization_pct),
                bar_width,
            ),
            usage_line(
                "MEM",
                gateway.and_then(|g| g.stats.memory_utilization_pct),
                bar_width,
            ),
        ];

        let total_devices = self.devices.len();
        let online = self
            .devices
            .iter()
            .filter(|d| d.state == unifly_api::DeviceState::Online)
            .count();
        let total_clients = self.clients.len();
        let count = |kind: ClientType| {
            self.clients
                .iter()
                .filter(|c| c.client_type == kind)
                .count()
        };
        let (wireless, wired) = (count(ClientType::Wireless), count(ClientType::Wired));

        // The fullest wording that fits, so a narrow panel shortens the
        // sentence instead of cutting a number.
        let summary = |label: &'static str, wordings: &[String]| {
            let text = wordings
                .iter()
                .find(|text| Span::raw(text.as_str()).width() <= content_w)
                .or(wordings.last())
                .map_or_else(String::new, |text| clip(text, content_w));
            Line::from(vec![
                Span::styled(label, Style::default().fg(theme::text_secondary())),
                Span::styled(text, Style::default().fg(theme::accent_secondary())),
            ])
        };
        lines.push(summary(
            " Dev  ",
            &[
                format!("{total_devices} total · {online} online"),
                format!("{online} of {total_devices} online"),
                format!("{online}/{total_devices} up"),
            ],
        ));
        lines.push(summary(
            " Cli  ",
            &[
                format!("{total_clients} total · {wireless} wifi · {wired} wired"),
                format!("{wireless} wifi · {wired} wired"),
                format!("{total_clients} ({wireless} wifi)"),
                total_clients.to_string(),
            ],
        ));
        if let Some(gw) = gateway
            && let (Some(l1), Some(l5), Some(l15)) = (
                gw.stats.load_average_1m,
                gw.stats.load_average_5m,
                gw.stats.load_average_15m,
            )
        {
            lines.push(summary(
                " Load ",
                &[
                    format!("{l1:.2} / {l5:.2} / {l15:.2}"),
                    format!("{l1:.2} {l5:.2} {l15:.2}"),
                    format!("{l1:.2}"),
                ],
            ));
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }
}
