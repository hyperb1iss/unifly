use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use unifly_api::Network;

use crate::tui::screens::events::render::clip;
use crate::tui::theme;

use super::super::DashboardScreen;
use super::super::focus::Panel;
use super::muted_line;

/// Widest a network's name is drawn.
const NAME_MAX: usize = 10;

/// The columns a row has room for. Each is dropped whole when the panel
/// is too narrow for it, least telling first, so no value is cut partway
/// to make room for another.
#[derive(Clone, Copy)]
struct Columns {
    name: usize,
    /// VLAN IDs, right-aligned under a header that may reach back over
    /// the unused end of the name column.
    vlan: Option<usize>,
    subnet: Option<usize>,
    /// Client counts, right-aligned under a header that may reach back
    /// over the unused end of the subnet column.
    clients: Option<usize>,
    ipv6: bool,
}

/// Cells the IPv6 mark takes past the client count: a gap and its own.
const IPV6_COLUMN: usize = 1 + "v6".len();

impl Columns {
    /// The fullest set of columns that fits `width`, sized to `names`,
    /// `vlans`, `subnets`, and `clients` as they will be drawn.
    fn fit(
        width: usize,
        names: &[String],
        vlans: &[String],
        subnets: &[&str],
        clients: &[String],
    ) -> Self {
        let widest = |cells: &mut dyn Iterator<Item = usize>| cells.max().unwrap_or(0);
        let cells = |text: &str| Span::raw(text).width();
        let name = widest(&mut names.iter().map(|name| cells(name))).max("Name".len());
        let subnet = widest(&mut subnets.iter().map(|subnet| cells(subnet))).max("Subnet".len());
        // A right-aligned header needs its own width past the header to
        // its left; the column is widened only by what the neighbor's
        // unused cells do not already give it.
        let vlan = widest(&mut vlans.iter().map(String::len))
            .max(("Name VLAN".len() - 1).saturating_sub(name));
        let count = widest(&mut clients.iter().map(String::len))
            .max(("Subnet Clients".len() - 1).saturating_sub(subnet));

        let names_only = Self {
            name,
            vlan: None,
            subnet: None,
            clients: None,
            ipv6: false,
        };
        let subnets = Self {
            subnet: Some(subnet),
            ..names_only
        };
        let vlans = Self {
            vlan: Some(vlan),
            ..subnets
        };
        let counts = Self {
            clients: Some(count),
            ..vlans
        };
        let full = Self {
            ipv6: true,
            ..counts
        };
        [full, counts, vlans, subnets]
            .into_iter()
            .find(|columns| columns.width() <= width)
            .unwrap_or(names_only)
    }

    fn width(self) -> usize {
        1 + self.name
            + self.vlan.map_or(0, |vlan| 1 + vlan)
            + self.subnet.map_or(0, |subnet| 1 + subnet)
            + self.clients.map_or(0, |clients| 1 + clients)
            + if self.ipv6 { IPV6_COLUMN } else { 0 }
    }

    /// The header row. A right-aligned label ends where its column does.
    fn header(self) -> Line<'static> {
        let name = match self.vlan {
            Some(vlan) => format!(" {:<pad$}VLAN", "Name", pad = self.name + 1 + vlan - 4),
            None => format!(" {:<pad$}", "Name", pad = self.name),
        };
        let subnet = match (self.subnet, self.clients) {
            (Some(subnet), Some(clients)) => {
                let pad = subnet + 1 + clients - "Clients".len();
                format!(" {:<pad$}Clients", "Subnet")
            }
            (Some(_), None) => " Subnet".to_owned(),
            (None, _) => String::new(),
        };
        let ipv6 = if self.ipv6 { " v6" } else { "" };
        let text = format!("{name}{subnet}{ipv6}");
        Line::from(Span::styled(
            text,
            Style::default()
                .fg(theme::border_unfocused())
                .add_modifier(Modifier::BOLD),
        ))
    }

    fn row(self, net: &Network, name: &str, vlan: &str, clients: &str) -> Line<'static> {
        let padding = self.name.saturating_sub(Span::raw(name).width());
        let mut spans = vec![Span::styled(
            format!(" {name}{}", " ".repeat(padding)),
            Style::default()
                .fg(theme::accent_secondary())
                .add_modifier(Modifier::BOLD),
        )];
        if let Some(width) = self.vlan {
            spans.push(Span::styled(
                format!(" {vlan:>width$}"),
                Style::default().fg(theme::accent_tertiary()),
            ));
        }
        if let Some(width) = self.subnet {
            let subnet = net.subnet.as_deref().unwrap_or("─");
            spans.push(Span::styled(
                format!(" {subnet:<width$}"),
                Style::default().fg(theme::text_secondary()),
            ));
        }
        if let Some(width) = self.clients {
            let color = if clients == "0" {
                theme::border_unfocused()
            } else {
                theme::warning()
            };
            spans.push(Span::styled(
                format!(" {clients:>width$}"),
                Style::default().fg(color),
            ));
        }
        if self.ipv6 && net.ipv6_enabled {
            spans.push(Span::styled(" on", Style::default().fg(theme::info())));
        }
        Line::from(spans)
    }
}

impl DashboardScreen {
    /// Networks panel: one line per network under a header. What a
    /// network's IPv6 setup is belongs to the Networks screen; here a
    /// mark says only that it has one.
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

        // The header takes a row once there are rows to head. When the
        // list runs past the panel, its last row counts the rest.
        let rows = usize::from(inner.height);
        let headed = rows >= 3;
        let room = rows - usize::from(headed);
        let listed = if sorted.len() > room {
            room.saturating_sub(1)
        } else {
            sorted.len()
        };
        let shown = &sorted[..listed];

        let names: Vec<String> = shown.iter().map(|net| clip(&net.name, NAME_MAX)).collect();
        let vlans: Vec<String> = shown
            .iter()
            .map(|net| net.vlan_id.map_or_else(|| "─".into(), |v| v.to_string()))
            .collect();
        let subnets: Vec<&str> = shown
            .iter()
            .map(|net| net.subnet.as_deref().unwrap_or("─"))
            .collect();
        let clients: Vec<String> = shown
            .iter()
            .map(|net| {
                self.clients
                    .iter()
                    .filter(|c| c.vlan.is_some_and(|v| Some(v) == net.vlan_id))
                    .count()
                    .to_string()
            })
            .collect();
        let columns = Columns::fit(usize::from(inner.width), &names, &vlans, &subnets, &clients);

        let mut lines = Vec::new();
        if headed {
            lines.push(columns.header());
        }
        for (index, net) in shown.iter().enumerate() {
            lines.push(columns.row(net, &names[index], &vlans[index], &clients[index]));
        }
        if listed < sorted.len() {
            lines.push(muted_line(&format!("+{} more", sorted.len() - listed)));
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }
}
