use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use unifly_api::Network;
use unifly_api::model::Ipv6Mode;

use crate::tui::screens::events::render::clip;
use crate::tui::theme;

use super::super::DashboardScreen;
use super::super::focus::Panel;
use super::muted_line;

/// The IPv6 line under a network that has IPv6 on.
fn ipv6_line(net: &Network, width: usize) -> Line<'static> {
    let mode = match net.ipv6_mode {
        Some(Ipv6Mode::PrefixDelegation) => "PD",
        Some(Ipv6Mode::Static) => "Static",
        Some(_) | None => "On",
    };
    let prefix = net.ipv6_prefix.as_deref().unwrap_or("─");
    let mut text = format!("{mode} {prefix}");
    if net.slaac_enabled {
        text.push_str(" SLAAC");
    }
    if net.dhcpv6_enabled {
        text.push_str(if net.slaac_enabled {
            "+DHCPv6"
        } else {
            " DHCPv6"
        });
    }
    Line::from(vec![
        Span::styled(" ⬡ ", Style::default().fg(theme::border_unfocused())),
        Span::styled(
            clip(&text, width.saturating_sub(3)),
            Style::default().fg(theme::info()),
        ),
    ])
}

impl DashboardScreen {
    /// Networks panel: one line per network, with its IPv6 setup beneath
    /// when every network still fits that way.
    pub(super) fn render_networks(&self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        let block = self.panel_block(Panel::Networks, "Networks");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.networks.is_empty() {
            let why = if self.access.is_some_and(|access| !access.integration) {
                "Networks need the Integration API (an API key)"
            } else {
                self.empty_text("No networks")
            };
            frame.render_widget(Paragraph::new(muted_line(why)), inner);
            return;
        }

        let mut sorted: Vec<_> = self.networks.iter().collect();
        sorted.sort_by_key(|n| n.vlan_id.unwrap_or(0));

        let rows = usize::from(inner.height);
        let width = usize::from(inner.width);
        let with_ipv6 = sorted.len() + sorted.iter().filter(|net| net.ipv6_enabled).count();
        let show_ipv6 = with_ipv6 <= rows;
        // When the list runs past the panel, its last row counts the rest.
        let listed = if sorted.len() > rows {
            rows.saturating_sub(1)
        } else {
            sorted.len()
        };

        // Columns are sized to what is listed. When a row cannot hold all
        // four, the client count goes first and then the VLAN, so the name
        // and subnet are never cut.
        let shown = &sorted[..listed];
        let cells = |text: &str| Span::raw(text).width();
        let name_width = shown
            .iter()
            .map(|net| cells(&net.name))
            .max()
            .unwrap_or(4)
            .min(10);
        let vlan_of = |net: &Network| net.vlan_id.map_or_else(|| "─".into(), |v| v.to_string());
        let vlan_width = shown
            .iter()
            .map(|net| vlan_of(net).len())
            .max()
            .unwrap_or(1);
        let subnet_width = shown
            .iter()
            .map(|net| cells(net.subnet.as_deref().unwrap_or("─")))
            .max()
            .unwrap_or(1);
        let base = 1 + name_width + 1 + subnet_width;
        let show_vlan = base + vlan_width < width;
        let show_clients = show_vlan && base + vlan_width + 1 + 5 <= width;

        let mut lines = Vec::new();
        for net in shown {
            let name = clip(&net.name, name_width);
            let padding = name_width.saturating_sub(cells(&name));
            let mut spans = vec![Span::styled(
                format!(" {name}{} ", " ".repeat(padding)),
                Style::default()
                    .fg(theme::accent_secondary())
                    .add_modifier(Modifier::BOLD),
            )];
            if show_vlan {
                spans.push(Span::styled(
                    format!("{:<vlan_width$} ", vlan_of(net)),
                    Style::default().fg(theme::accent_tertiary()),
                ));
            }
            spans.push(Span::styled(
                clip(
                    net.subnet.as_deref().unwrap_or("─"),
                    width.saturating_sub(name_width + 2),
                ),
                Style::default().fg(theme::text_secondary()),
            ));
            let client_count = self
                .clients
                .iter()
                .filter(|c| c.vlan.is_some_and(|v| Some(v) == net.vlan_id))
                .count();
            if show_clients && client_count > 0 {
                spans.push(Span::styled(
                    format!(" {client_count}c"),
                    Style::default().fg(theme::warning()),
                ));
            }
            lines.push(Line::from(spans));
            if show_ipv6 && net.ipv6_enabled {
                lines.push(ipv6_line(net, width));
            }
        }
        if listed < sorted.len() {
            lines.push(muted_line(&format!("+{} more", sorted.len() - listed)));
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }
}
