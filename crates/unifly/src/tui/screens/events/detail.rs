//! The detail pane under the list: everything the selected event carries.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use chrono::{Local, Utc};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use unifly_api::{Client, Device, Event, MacAddress};

use super::EventsScreen;
use super::render::{category_label, severity_color, severity_label};
use crate::tui::theme;
use crate::tui::widgets::wrap;

/// Columns the field labels take.
const LABEL_COLUMNS: usize = 10;
/// Most lines of message the pane shows before it cuts with an ellipsis.
const MESSAGE_LINES: usize = 6;

/// How long ago `seconds` was, in the largest unit that fits.
fn age(seconds: i64) -> String {
    match seconds {
        ..60 => "just now".into(),
        60..3_600 => format!("{} min ago", seconds / 60),
        3_600..86_400 => format!("{} h ago", seconds / 3_600),
        _ => format!("{} d ago", seconds / 86_400),
    }
}

impl EventsScreen {
    /// The detail pane's lines for the selected event, wrapped to `width`
    /// columns of pane.
    pub(super) fn detail_lines(&self, width: u16) -> Option<Vec<Line<'static>>> {
        let event = self.selected()?;
        let value_columns = usize::from(width).saturating_sub(LABEL_COLUMNS + 4).max(8);
        let label = |name: &str| {
            Span::styled(
                format!(" {name:<LABEL_COLUMNS$}"),
                Style::default().fg(theme::text_secondary()),
            )
        };
        let plain = Style::default().fg(theme::text_primary());
        let muted = Style::default().fg(theme::text_muted());

        let local = event.timestamp.with_timezone(&Local);
        let elapsed = Utc::now().signed_duration_since(event.timestamp);
        let mut lines = vec![
            Line::from(vec![
                label("Time"),
                Span::styled(local.format("%Y-%m-%d %H:%M:%S").to_string(), plain),
                Span::styled(format!("  {}", age(elapsed.num_seconds())), muted),
            ]),
            Line::from(vec![
                label("Category"),
                Span::styled(format!("{:<10}", category_label(event.category)), plain),
                Span::styled("Severity  ", Style::default().fg(theme::text_secondary())),
                Span::styled(
                    severity_label(event.severity),
                    Style::default().fg(severity_color(event.severity)),
                ),
            ]),
            Line::from(vec![
                label("Type"),
                Span::styled(super::render::clip(&event.event_type, value_columns), plain),
            ]),
        ];

        if let Some(mac) = &event.device_mac {
            let name = self
                .devices
                .iter()
                .find(|device| &device.mac == mac)
                .map(|device| device.name.clone().unwrap_or_else(|| mac.to_string()));
            lines.push(entity_line(
                label("Device"),
                mac,
                name,
                "not on this controller",
            ));
        }
        if let Some(mac) = &event.client_mac {
            let name = self
                .clients
                .iter()
                .find(|client| &client.mac == mac)
                .map(|client| {
                    client
                        .name
                        .clone()
                        .or_else(|| client.hostname.clone())
                        .unwrap_or_else(|| mac.to_string())
                });
            lines.push(entity_line(label("Client"), mac, name, "not connected"));
        }
        // A message that spells out an address leaves the reader to work
        // out whose it is; the lists already say.
        for (device, address) in self.devices_by_address(event) {
            let name = device
                .name
                .clone()
                .unwrap_or_else(|| device.mac.to_string());
            lines.push(address_line(label("Device"), name, address));
        }
        for (client, address) in self.clients_by_address(event) {
            let name = client
                .name
                .clone()
                .or_else(|| client.hostname.clone())
                .unwrap_or_else(|| client.mac.to_string());
            lines.push(address_line(label("Client"), name, address));
        }

        let message = wrap::wrap_clamped(&event.message, value_columns, MESSAGE_LINES);
        for (index, text) in message.into_iter().enumerate() {
            lines.push(Line::from(vec![
                label(if index == 0 { "Message" } else { "" }),
                Span::styled(text, Style::default().fg(severity_color(event.severity))),
            ]));
        }
        Some(lines)
    }

    /// The devices whose addresses `event`'s message spells out, leaving
    /// out the one the event already names by MAC.
    pub(super) fn devices_by_address(&self, event: &Event) -> Vec<(&Arc<Device>, IpAddr)> {
        identifying_addresses(&event.message)
            .filter_map(|address| {
                let device = self
                    .devices
                    .iter()
                    .find(|device| device.has_address(address))?;
                (event.device_mac.as_ref() != Some(&device.mac)).then_some((device, address))
            })
            .collect()
    }

    /// The connected clients whose addresses `event`'s message spells out,
    /// leaving out the one the event already names by MAC.
    pub(super) fn clients_by_address(&self, event: &Event) -> Vec<(&Arc<Client>, IpAddr)> {
        identifying_addresses(&event.message)
            .filter_map(|address| {
                let client = self
                    .clients
                    .iter()
                    .find(|client| client.has_address(address))?;
                (event.client_mac.as_ref() != Some(&client.mac)).then_some((client, address))
            })
            .collect()
    }

    pub(super) fn render_detail(frame: &mut Frame, area: Rect, lines: Vec<Line<'static>>) {
        let block = Block::default()
            .title(" Event ")
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_focused());
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

/// A device or client line: its name and MAC, or its MAC and why it
/// cannot be opened. The status bar offers the key that opens it.
fn entity_line(
    label: Span<'static>,
    mac: &MacAddress,
    name: Option<String>,
    missing: &'static str,
) -> Line<'static> {
    let plain = Style::default().fg(theme::text_primary());
    let muted = Style::default().fg(theme::text_muted());
    match name {
        Some(name) if name != mac.to_string() => Line::from(vec![
            label,
            Span::styled(name, Style::default().fg(theme::accent_secondary())),
            Span::styled(format!("  {mac}"), muted),
        ]),
        Some(_) => Line::from(vec![label, Span::styled(mac.to_string(), plain)]),
        None => Line::from(vec![
            label,
            Span::styled(mac.to_string(), plain),
            Span::styled(format!("  ({missing})"), muted),
        ]),
    }
}

/// A device or client the message names by address: its name, then the
/// address that ties it to the message.
fn address_line(label: Span<'static>, name: String, address: IpAddr) -> Line<'static> {
    Line::from(vec![
        label,
        Span::styled(name, Style::default().fg(theme::accent_secondary())),
        Span::styled(
            format!("  {address}"),
            Style::default().fg(theme::text_muted()),
        ),
    ])
}

/// The addresses in `message` that can name one host. Every link has its
/// own `fe80::/10`, so a link-local address could belong to a host on any
/// of them.
fn identifying_addresses(message: &str) -> impl Iterator<Item = IpAddr> {
    addresses(message)
        .into_iter()
        .filter(|ip| !matches!(ip, IpAddr::V6(v6) if v6.is_unicast_link_local()))
}

/// The IP addresses `message` spells out, in order, each once. An IPv4
/// address may carry a port.
fn addresses(message: &str) -> Vec<IpAddr> {
    let mut found = Vec::new();
    let tokens = message.split(|c: char| !(c.is_ascii_hexdigit() || c == '.' || c == ':'));
    for token in tokens.filter(|token| !token.is_empty()) {
        // Punctuation that ends a clause is not part of the address.
        let trimmed = token.trim_end_matches(['.', ':']);
        let address = token.parse().or_else(|_| trimmed.parse()).ok().or_else(|| {
            let (host, _port) = trimmed.rsplit_once(':')?;
            host.parse::<Ipv4Addr>().ok().map(IpAddr::V4)
        });
        if let Some(address) = address
            && !found.contains(&address)
        {
            found.push(address);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::{addresses, identifying_addresses};

    fn found(message: &str) -> Vec<String> {
        addresses(message)
            .into_iter()
            .map(|address| address.to_string())
            .collect()
    }

    #[test]
    fn addresses_are_read_out_of_a_sentence() {
        assert_eq!(
            found(
                "A network intrusion attempt from 171.25.193.9 to 10.4.20.198 has been detected."
            ),
            ["171.25.193.9", "10.4.20.198"]
        );
        // The full stop that ends the sentence, a port, and a repeat.
        assert_eq!(
            found("Blocked 10.0.0.7:443 -> 10.0.0.9. Again: 10.0.0.7"),
            ["10.0.0.7", "10.0.0.9"]
        );
        assert_eq!(
            found("lease for fd00::1a2b, was [2001:db8::1]:8443"),
            ["fd00::1a2b", "2001:db8::1"]
        );
    }

    #[test]
    fn a_link_local_address_names_no_one() {
        let named: Vec<String> = identifying_addresses("from fe80::1 to 2001:db8::9")
            .map(|address| address.to_string())
            .collect();
        assert_eq!(named, ["2001:db8::9"]);
    }

    #[test]
    fn words_and_versions_are_not_addresses() {
        assert_eq!(found("ap-loft upgraded to 8.1.113"), Vec::<String>::new());
        assert_eq!(found("bad cafe: face added at 10:42"), Vec::<String>::new());
    }
}
