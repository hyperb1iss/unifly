use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use unifly_api::Client;

use crate::tui::theme;
use crate::tui::widgets::bytes_fmt;
use crate::tui::widgets::click::{self, Click, Hint, HitMap};

#[allow(clippy::too_many_lines, clippy::as_conversions)]
pub(super) fn render_detail(
    frame: &mut Frame,
    area: Rect,
    client: &Client,
    hits: &mut HitMap<Click>,
) {
    let name = client
        .name
        .as_deref()
        .or(client.hostname.as_deref())
        .unwrap_or("Unknown");
    let ip = client
        .ip
        .map_or_else(|| "─".into(), |client_ip| client_ip.to_string());
    let mac = client.mac.to_string();
    let type_str = format!("{:?}", client.client_type);

    let title = format!(" {name}  ·  {type_str}  ·  {ip}  ·  {mac} ");
    let block = Block::default()
        .title(title)
        .title_style(theme::title_style())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border_focused());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let network = client
        .network_id
        .as_ref()
        .map_or_else(|| "─".into(), std::string::ToString::to_string);
    let signal = client
        .wireless
        .as_ref()
        .and_then(|wireless| wireless.signal_dbm)
        .map_or_else(|| "─".into(), |dbm| format!("{dbm} dBm"));
    let channel = client
        .wireless
        .as_ref()
        .and_then(|wireless| wireless.channel)
        .map_or_else(|| "─".into(), |channel| channel.to_string());
    let ssid = client
        .wireless
        .as_ref()
        .and_then(|wireless| wireless.ssid.as_deref())
        .unwrap_or("─");
    let tx = client
        .tx_bytes
        .map_or_else(|| "─".into(), bytes_fmt::fmt_bytes_short);
    let rx = client
        .rx_bytes
        .map_or_else(|| "─".into(), bytes_fmt::fmt_bytes_short);
    let duration = client.connected_at.map_or_else(
        || "─".into(),
        |ts| {
            let dur = chrono::Utc::now().signed_duration_since(ts);
            #[allow(clippy::cast_sign_loss)]
            let secs = dur.num_seconds().max(0) as u64;
            bytes_fmt::fmt_uptime(secs)
        },
    );
    let guest = if client.is_guest { "yes" } else { "no" };
    let blocked = if client.blocked { "yes" } else { "no" };

    let detail_layout = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  Network        ",
                Style::default().fg(theme::text_secondary()),
            ),
            Span::styled(network, Style::default().fg(theme::accent_secondary())),
            Span::styled(
                "       SSID         ",
                Style::default().fg(theme::text_secondary()),
            ),
            Span::styled(ssid, Style::default().fg(theme::accent_secondary())),
        ]),
        Line::from(vec![
            Span::styled(
                "  Signal         ",
                Style::default().fg(theme::text_secondary()),
            ),
            Span::styled(&signal, Style::default().fg(theme::accent_secondary())),
            Span::styled(
                "       Channel      ",
                Style::default().fg(theme::text_secondary()),
            ),
            Span::styled(&channel, Style::default().fg(theme::accent_secondary())),
        ]),
        Line::from(vec![
            Span::styled(
                "  TX             ",
                Style::default().fg(theme::text_secondary()),
            ),
            Span::styled(&tx, Style::default().fg(theme::accent_tertiary())),
            Span::styled(
                "       RX           ",
                Style::default().fg(theme::text_secondary()),
            ),
            Span::styled(&rx, Style::default().fg(theme::accent_tertiary())),
        ]),
        Line::from(vec![
            Span::styled(
                "  Duration       ",
                Style::default().fg(theme::text_secondary()),
            ),
            Span::styled(&duration, Style::default().fg(theme::accent_secondary())),
        ]),
        Line::from(vec![
            Span::styled(
                "  Guest          ",
                Style::default().fg(theme::text_secondary()),
            ),
            Span::styled(guest, Style::default().fg(theme::text_secondary())),
            Span::styled(
                "       Blocked      ",
                Style::default().fg(theme::text_secondary()),
            ),
            Span::styled(
                blocked,
                Style::default().fg(if client.blocked {
                    theme::error()
                } else {
                    theme::text_secondary()
                }),
            ),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines), detail_layout[0]);

    let bar = click::key_hints(&[
        Hint::button("b", "block", click::char_key('b')),
        Hint::button("B", "unblock", click::char_key('B')),
        Hint::button("x", "kick", click::char_key('x')),
        Hint::button("Esc", "back", KeyCode::Esc),
    ]);
    let line = bar.draw(detail_layout[1], hits);
    frame.render_widget(Paragraph::new(line), detail_layout[1]);
}
