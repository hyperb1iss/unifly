use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::tui::screens::events::render::clip;
use crate::tui::theme;
use crate::tui::widgets::bytes_fmt;

use super::super::DashboardScreen;
use super::super::focus::Panel;
use super::super::helpers::{parse_ipv6_from_text, parse_ipv6_from_value};
use super::muted_line;

impl DashboardScreen {
    /// Gateway panel - WAN connection details with IPv6.
    #[allow(clippy::too_many_lines)]
    pub(super) fn render_gateway(&self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        let block = self.panel_block(Panel::Gateway, "Gateway");

        let inner = block.inner(area);
        frame.render_widget(block, area);
        let w = usize::from(inner.width);

        let gateway = self.gateway();
        let wan_health = self.health.iter().find(|h| h.subsystem == "wan");
        let www_health = self.health.iter().find(|h| h.subsystem == "www");

        let isp_name = wan_health
            .and_then(|h| h.extra.get("isp_name").and_then(|v| v.as_str()))
            .or_else(|| {
                wan_health.and_then(|h| h.extra.get("isp_organization").and_then(|v| v.as_str()))
            });
        let dns = wan_health
            .and_then(|h| h.extra.get("nameservers").and_then(|v| v.as_array()))
            .map(|ns| {
                let servers: Vec<_> = ns.iter().filter_map(|v| v.as_str()).collect();
                let shown = servers.iter().take(2).copied().collect::<Vec<_>>();
                let hidden = servers.len().saturating_sub(shown.len());
                if hidden > 0 {
                    format!("{}, +{hidden}", shown.join(", "))
                } else {
                    shown.join(", ")
                }
            });
        let gw_ip = wan_health
            .and_then(|h| h.extra.get("gateways").and_then(|v| v.as_array()))
            .and_then(|a| a.first().and_then(|v| v.as_str()));
        let wan_ipv6 = wan_health
            .and_then(|h| {
                const IPV6_KEYS: &[&str] = &[
                    "wan_ip6",
                    "wan_ip6s",
                    "wan_ipv6",
                    "wan_ipv6s",
                    "ipv6",
                    "ipv6Address",
                    "ipv6_address",
                ];

                for key in IPV6_KEYS {
                    if let Some(ipv6) = h.extra.get(*key).and_then(parse_ipv6_from_value) {
                        return Some(ipv6);
                    }
                }

                if let Some(ipv6) = h
                    .extra
                    .get("wan_ip")
                    .and_then(parse_ipv6_from_value)
                    .or_else(|| h.wan_ip.as_deref().and_then(parse_ipv6_from_text))
                {
                    return Some(ipv6);
                }

                h.gateways
                    .as_ref()
                    .and_then(|gateways| gateways.iter().find_map(|gw| parse_ipv6_from_text(gw)))
            })
            .or_else(|| gateway.and_then(|g| g.wan_ipv6.clone()));

        let gw_version =
            wan_health.and_then(|h| h.extra.get("gw_version").and_then(|v| v.as_str()));
        let latency = www_health.and_then(|h| h.latency);
        let uptime = gateway.and_then(|g| g.stats.uptime_secs);
        let wan_ip = gateway
            .and_then(|g| g.ip)
            .map(|ip| ip.to_string())
            .or_else(|| wan_health.and_then(|h| h.wan_ip.clone()));

        // Most telling first: a short panel shows as many as fit.
        let mut lines = Vec::new();

        if let Some(gw) = gateway {
            let model = gw.model.as_deref().unwrap_or("Gateway");
            let fw = gw_version.or(gw.firmware_version.as_deref()).unwrap_or("─");
            let header = clip(&format!("{model} ({fw})"), w.saturating_sub(4));
            lines.push(Line::from(vec![
                Span::styled(" ◈ ", Style::default().fg(theme::accent_primary())),
                Span::styled(
                    header,
                    Style::default()
                        .fg(theme::accent_secondary())
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        } else {
            lines.push(muted_line(self.empty_text("No UniFi gateway on this site")));
        }

        let kv = |label: &str, value: &str, color: ratatui::style::Color| -> Line<'static> {
            let shown = clip(value, w.saturating_sub(7));
            Line::from(vec![
                Span::styled(
                    format!(" {label:<5}"),
                    Style::default().fg(theme::text_secondary()),
                ),
                Span::styled(shown, Style::default().fg(color)),
            ])
        };

        lines.push(kv(
            "WAN",
            &wan_ip.unwrap_or_else(|| "─".into()),
            theme::accent_tertiary(),
        ));
        if let Some(isp) = isp_name {
            lines.push(kv("ISP", isp, theme::text_secondary()));
        }
        let lat_str = latency.map_or_else(|| "─".into(), |l| format!("{l:.0}ms"));
        let up_str = uptime.map_or_else(|| "─".into(), bytes_fmt::fmt_uptime);
        lines.push(Line::from(vec![
            Span::styled(" Lat  ", Style::default().fg(theme::text_secondary())),
            Span::styled(lat_str, Style::default().fg(theme::accent_secondary())),
            Span::styled("   Up ", Style::default().fg(theme::text_secondary())),
            Span::styled(up_str, Style::default().fg(theme::accent_secondary())),
        ]));
        if let Some(ref d) = dns {
            lines.push(kv("DNS", d, theme::text_secondary()));
        }
        if let Some(gw) = gw_ip {
            lines.push(kv("GW", gw, theme::text_secondary()));
        }
        lines.push(kv(
            "IPv6",
            wan_ipv6.as_deref().unwrap_or("─"),
            theme::info(),
        ));

        frame.render_widget(Paragraph::new(lines), inner);
    }
}
