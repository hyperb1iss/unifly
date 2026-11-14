use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use unifly_api::Client;
use unifly_api::model::WirelessInfo;

use super::table::command_hints;
use crate::tui::screens::clients::ClientsScreen;
use crate::tui::screens::clients::view::{client_name, type_label};
use crate::tui::screens::devices::grid::{self, Field};
use crate::tui::theme;
use crate::tui::widgets::bytes_fmt;
use crate::tui::widgets::click::{self, Hint};

pub(super) fn render_detail(
    screen: &ClientsScreen,
    frame: &mut Frame,
    area: Rect,
    client: &Client,
) {
    let kind = if client.is_guest {
        format!("{} guest", type_label(client.client_type))
    } else {
        type_label(client.client_type).to_owned()
    };
    let block = Block::default()
        .title(format!(" {}  ·  {kind} ", client_name(client)))
        .title_style(theme::title_style())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border_focused());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(inner);
    let fields = fields(screen, client);
    frame.render_widget(
        Paragraph::new(grid::lines(&fields, layout[0].width, layout[0].height)),
        layout[0],
    );

    let mut hints = vec![(3, Hint::info("j/k", "client"))];
    hints.extend(command_hints(client));
    hints.push((0, Hint::button("Esc", "close", KeyCode::Esc)));
    let hints = grid::fit_hints(hints, layout[1].width);
    let line = click::key_hints(&hints).draw(layout[1], &mut screen.hits.borrow_mut());
    frame.render_widget(Paragraph::new(line), layout[1]);
}

/// Everything the detail says about `client`, most useful first.
fn fields(screen: &ClientsScreen, client: &Client) -> Vec<Field> {
    let mut fields = vec![
        Field::optional("IP", client.ip.map(|ip| ip.to_string())).color(theme::accent_tertiary()),
        Field::new("MAC", client.mac.to_string()).color(theme::accent_tertiary()),
        Field::optional("Network", network(screen, client)),
        Field::optional("Uplink", screen.links.uplink_label(client)),
        Field::optional("Connected", client.connected_at.map(connected_for)),
        Field::optional("Since", client.connected_at.map(grid::local_time)),
        fixed_ip(client),
    ];
    if let Some(hostname) = client.hostname.as_deref()
        && client.name.as_deref().is_some_and(|name| name != hostname)
    {
        fields.push(Field::new("Hostname", hostname));
    }
    if let Some(wireless) = &client.wireless {
        fields.extend(wireless_fields(wireless));
    }
    let pair = |tx: Option<String>, rx: Option<String>| match (tx, rx) {
        (None, None) => None,
        (tx, rx) => {
            let dash = || grid::DASH.to_owned();
            Some(format!(
                "{} / {}",
                tx.unwrap_or_else(dash),
                rx.unwrap_or_else(dash)
            ))
        }
    };
    fields.push(Field::optional(
        "Data TX/RX",
        pair(
            client.tx_bytes.map(bytes_fmt::fmt_bytes_short),
            client.rx_bytes.map(bytes_fmt::fmt_bytes_short),
        ),
    ));
    if let Some(bandwidth) = &client.bandwidth {
        fields.push(Field::optional(
            "Rate TX/RX",
            pair(
                Some(bytes_fmt::fmt_rate(bandwidth.tx_bytes_per_sec)),
                Some(bytes_fmt::fmt_rate(bandwidth.rx_bytes_per_sec)),
            ),
        ));
    }
    if let Some(system) = system(client) {
        fields.push(Field::new("System", system));
    }
    if client.is_guest {
        fields.push(guest(client));
    }
    fields.push(if client.blocked {
        Field::new("Blocked", "yes, cannot connect").color(theme::error())
    } else {
        Field::new("Blocked", "no").color(theme::text_secondary())
    });
    fields
}

/// The client's network by name, with its VLAN when both are known.
fn network(screen: &ClientsScreen, client: &Client) -> Option<String> {
    let name = screen.links.network_label(client)?;
    Some(match client.vlan {
        Some(vlan) if !name.starts_with("VLAN ") => format!("{name} (VLAN {vlan})"),
        _ => name,
    })
}

/// How long the client has been connected.
fn connected_for(since: chrono::DateTime<chrono::Utc>) -> String {
    let secs = chrono::Utc::now()
        .signed_duration_since(since)
        .num_seconds()
        .max(0);
    bytes_fmt::fmt_uptime(u64::try_from(secs).unwrap_or_default())
}

fn fixed_ip(client: &Client) -> Field {
    match (client.use_fixedip, client.fixed_ip) {
        (true, Some(ip)) => Field::new("Fixed IP", ip.to_string()),
        (true, None) => Field::new("Fixed IP", "yes"),
        (false, _) => Field::new("Fixed IP", "no, from DHCP").color(theme::text_secondary()),
    }
}

fn wireless_fields(wireless: &WirelessInfo) -> Vec<Field> {
    let channel = wireless
        .channel
        .map(|channel| match wireless.frequency_ghz {
            Some(ghz) => format!("{channel} ({ghz} GHz)"),
            None => channel.to_string(),
        });
    let mbps = |kbps: Option<u64>| {
        kbps.map_or_else(|| grid::DASH.to_owned(), |kbps| (kbps / 1000).to_string())
    };
    let rates = match (wireless.tx_rate_kbps, wireless.rx_rate_kbps) {
        (None, None) => None,
        (tx, rx) => Some(format!("{} / {} Mbps", mbps(tx), mbps(rx))),
    };
    let signal = wireless.signal_dbm.map(|dbm| match wireless.noise_dbm {
        Some(noise) => format!("{dbm} dBm (noise {noise})"),
        None => format!("{dbm} dBm"),
    });
    vec![
        Field::optional("SSID", wireless.ssid.clone()),
        Field::optional("Signal", signal),
        Field::optional("Channel", channel),
        Field::optional("Link TX/RX", rates),
        Field::optional(
            "Experience",
            wireless.satisfaction.map(|score| format!("{score}%")),
        ),
    ]
}

/// The operating system and device class the controller fingerprinted.
fn system(client: &Client) -> Option<String> {
    match (client.os_name.as_deref(), client.device_class.as_deref()) {
        (Some(os), Some(class)) => Some(format!("{os}, {class}")),
        (Some(one), None) | (None, Some(one)) => Some(one.to_owned()),
        (None, None) => None,
    }
}

/// Where a guest stands with the portal.
fn guest(client: &Client) -> Field {
    let Some(auth) = client.guest_auth.as_ref().filter(|auth| auth.authorized) else {
        return Field::new("Guest", "not authorized").color(theme::warning());
    };
    match auth.expires_at {
        Some(expires) => Field::new("Guest", format!("until {}", grid::local_time(expires))),
        None => Field::new("Guest", "authorized"),
    }
}
