use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use unifly_api::Client;

use crate::tui::action::Action;
use crate::tui::screens::events::render::clip;
use crate::tui::theme;
use crate::tui::widgets::bytes_fmt;
use crate::tui::widgets::click::{self, Click};

use super::super::DashboardScreen;
use super::super::focus::Panel;
use super::muted_line;

/// Most clients the panel ranks.
const MAX_ROWS: usize = 8;

fn traffic(client: &Client) -> u64 {
    client
        .tx_bytes
        .unwrap_or(0)
        .saturating_add(client.rx_bytes.unwrap_or(0))
}

fn display_name(client: &Client) -> &str {
    client
        .name
        .as_deref()
        .or(client.hostname.as_deref())
        .unwrap_or("unknown")
}

impl DashboardScreen {
    /// Top Clients panel with proportional traffic bars.
    pub(super) fn render_top_clients(
        &self,
        frame: &mut ratatui::Frame,
        area: ratatui::layout::Rect,
    ) {
        let block = self.panel_block(Panel::TopClients, "Top Clients");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // A client with no counters is not a top client at zero bytes: it
        // has no ranking at all.
        let mut ranked: Vec<_> = self
            .clients
            .iter()
            .filter(|client| traffic(client) > 0)
            .collect();
        if ranked.is_empty() {
            let why = if self.clients.is_empty() {
                self.empty_text("No clients")
            } else {
                "No client traffic reported"
            };
            frame.render_widget(Paragraph::new(muted_line(why)), inner);
            return;
        }
        ranked.sort_by_key(|client| std::cmp::Reverse(traffic(client)));
        ranked.truncate(usize::from(inner.height).min(MAX_ROWS));

        let max_traffic = ranked.first().map_or(1, |client| traffic(client)).max(1);
        let values: Vec<String> = ranked
            .iter()
            .map(|client| bytes_fmt::fmt_bytes_short(traffic(client)))
            .collect();
        let value_width = values.iter().map(String::len).max().unwrap_or(4);
        let width = usize::from(inner.width);
        // Leading space, name, gap, bar, gap, value.
        let name_room = width.saturating_sub(value_width + 3 + 4);
        let name_width = ranked
            .iter()
            .map(|client| Span::raw(display_name(client)).width())
            .max()
            .unwrap_or(8)
            .min(name_room)
            .max(4);
        let bar_width =
            u16::try_from(width.saturating_sub(name_width + value_width + 3)).unwrap_or(0);

        let mut lines = Vec::new();
        for (client, value) in ranked.iter().zip(&values) {
            let name = clip(display_name(client), name_width);
            let padding = name_width.saturating_sub(Span::raw(name.as_str()).width());
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {name}{} ", " ".repeat(padding)),
                    Style::default().fg(theme::accent_secondary()),
                ),
                Span::styled(
                    bytes_fmt::fmt_traffic_bar(traffic(client), max_traffic, bar_width),
                    Style::default().fg(theme::accent_primary()),
                ),
                Span::styled(
                    format!(" {value:>value_width$}"),
                    Style::default().fg(theme::text_secondary()),
                ),
            ]));
            self.hits.borrow_mut().push(
                click::paragraph_row(inner, lines.len() - 1),
                Click::Action(Action::ShowClient(client.id.clone())),
            );
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }
}
