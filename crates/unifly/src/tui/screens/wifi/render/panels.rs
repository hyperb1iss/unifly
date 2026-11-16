//! The WiFi screen's detail panels: an AP, a client's WiFi experience, the
//! channel map, and an SSID.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use unifly_api::model::{EntityOrigin, WifiBroadcast, WifiBroadcastType, WifiSecurityMode};
use unifly_api::{Device, DeviceState};

use super::empty::{NO_SESSION_CHANNELS, NO_SESSION_EXPERIENCE};
use super::tables::count_label;
use super::{
    NOTE_PANEL_ROWS, health_color, kbps_to_mbps, panel_block, render_notice, state_color,
    state_label,
};
use crate::tui::action::WifiBand;
use crate::tui::screens::wifi::WifiScreen;
use crate::tui::screens::wifi::roam::{band_name, json_i32, json_string, json_u64};
use crate::tui::screens::wifi::ssid_detail::{Pick, Setting};
use crate::tui::screens::wifi::state::{
    band_from_frequency, display_client_name, display_device_name,
};
use crate::tui::theme;
use crate::tui::widgets::hyperchart::{HeatmapCell, HyperHeatmap};
use crate::tui::widgets::signal_bars;

/// Rows a panel of `lines` takes with its frame.
fn framed(lines: usize) -> u16 {
    u16::try_from(lines).unwrap_or(u16::MAX).saturating_add(2)
}

fn muted() -> Style {
    Style::default().fg(theme::text_muted())
}

fn secondary() -> Style {
    Style::default().fg(theme::text_secondary())
}

fn gauge_fill(value: u8, width: usize) -> String {
    let filled = (usize::from(value.min(100)) * width) / 100;
    format!(
        "{}{}",
        "█".repeat(filled),
        "░".repeat(width.saturating_sub(filled))
    )
}

fn percent(value: Option<f64>) -> String {
    value.map_or_else(|| "─".to_owned(), |value| format!("{value:.0}%"))
}

impl WifiScreen {
    /// Rows the AP detail takes with all the room it wants, frame
    /// included: every line it has, down to its last client.
    pub(super) fn ap_detail_height(&self) -> u16 {
        self.selected_ap().map_or(NOTE_PANEL_ROWS, |ap| {
            framed(self.ap_detail_lines(ap, usize::MAX).len())
        })
    }

    pub(super) fn render_ap_detail(&self, frame: &mut Frame, area: Rect) {
        let block = panel_block(" AP Detail ", self.focused);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let Some(ap) = self.selected_ap() else {
            render_notice(frame, inner, "No access point selected");
            return;
        };
        let lines = self.ap_detail_lines(ap, usize::from(inner.height));
        frame.render_widget(Paragraph::new(lines), inner);
    }

    /// The AP detail's lines for a panel `rows` tall inside its frame.
    /// The client list is cut to what is left under the rest.
    fn ap_detail_lines(&self, ap: &Device, rows: usize) -> Vec<Line<'static>> {
        let clients = self.ap_clients(ap);
        let mut lines = vec![ap_heading(ap)];
        lines.push(match self.ap_health(ap) {
            Some(health) => Line::from(vec![
                Span::styled(" Health  ", muted()),
                Span::styled(
                    gauge_fill(health, 18),
                    Style::default().fg(health_color(Some(health))),
                ),
                Span::styled(format!(" {health}/100"), secondary()),
            ]),
            None if self.session_missing() => Line::from(vec![
                Span::styled(" Health  ", muted()),
                Span::styled("needs a direct controller session", secondary()),
            ]),
            None if ap.state == DeviceState::Online => Line::from(vec![
                Span::styled(" Health  ", muted()),
                Span::styled("no clients connected, nothing to score", secondary()),
            ]),
            None => Line::from(vec![
                Span::styled(" Health  ", muted()),
                Span::styled("not online", secondary()),
            ]),
        });
        if let Some(signal) = self.ap_signal(ap) {
            lines.push(Line::from(vec![
                Span::styled(" Signal  ", muted()),
                signal_bars::signal_span(Some(signal)),
                Span::styled(format!("  {signal} dBm average"), secondary()),
            ]));
        }
        if let Some(version) = &ap.firmware_version {
            let mut spans = vec![
                Span::styled(" Firmware ", muted()),
                Span::styled(version.clone(), secondary()),
            ];
            if ap.firmware_updatable {
                spans.push(Span::styled(
                    "  update available",
                    Style::default().fg(theme::warning()),
                ));
            }
            lines.push(Line::from(spans));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(" Radios", theme::table_header())));
        if ap.radios.is_empty() {
            lines.push(Line::from(Span::styled(
                "  The controller reports no radio data for this AP",
                muted(),
            )));
        }
        lines.extend(ap.radios.iter().map(radio_line));

        // A gap, the list's heading, and at least one row of it: a panel
        // without room for all three leaves the list to the table above,
        // whose Clients column counts it.
        let room = rows.saturating_sub(lines.len() + 2);
        if !self.session_missing() && room > 0 {
            lines.push(Line::from(""));
            lines.extend(client_lines(clients, room));
        }
        lines
    }

    /// Rows the experience panel takes, frame included: its lines, or
    /// room for the one note it shows instead.
    pub(super) fn client_detail_height(&self) -> u16 {
        self.client_detail_lines()
            .map_or(NOTE_PANEL_ROWS, |lines| framed(lines.len()))
    }

    pub(super) fn render_client_detail(&self, frame: &mut Frame, area: Rect) {
        let block = panel_block(" WiFi Experience ", self.focused);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        match self.client_detail_lines() {
            Ok(lines) => {
                frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
            }
            Err(note) => render_notice(frame, inner, &note),
        }
    }

    /// The experience panel's lines, or the note it shows in their place:
    /// that nothing is selected, or why the detail is not on hand.
    fn client_detail_lines(&self) -> Result<Vec<Line<'static>>, String> {
        let Some(client) = self.selected_client() else {
            return Err("No client selected".to_owned());
        };
        if self.session_missing() {
            return Err(NO_SESSION_EXPERIENCE.to_owned());
        }

        // The detail on hand may belong to the client selected before.
        let ip = client.ip.map(|ip| ip.to_string());
        let detail = self
            .client_detail
            .as_ref()
            .filter(|detail| ip.as_deref() == Some(detail.ip.as_str()));
        let Some(detail) = detail.map(|detail| &detail.data) else {
            let name = display_client_name(client);
            return Err(match ip.as_deref() {
                None => format!("{name} has no IP address, so its WiFi detail cannot be read"),
                Some(ip) => match self.client_detail_error(ip) {
                    Some(error) => format!("WiFi detail unavailable: {error}. Retrying."),
                    None => format!("Loading WiFi detail for {name}\u{2026}"),
                },
            });
        };

        let ssid = client
            .wireless
            .as_ref()
            .and_then(|wireless| wireless.ssid.clone())
            .unwrap_or_else(|| "─".to_string());
        let mut lines = vec![Line::from(vec![
            Span::styled(
                format!(" {}", display_client_name(client)),
                theme::title_style(),
            ),
            Span::styled(format!("  on {}", self.client_ap_name(client)), secondary()),
            Span::styled("  SSID ", muted()),
            Span::styled(ssid, secondary()),
        ])];
        lines.extend(experience_lines(detail));
        Ok(lines)
    }

    /// Rows the channel map takes, frame included: its summary and the
    /// rows its cells wrap to at `width`.
    pub(super) fn channel_map_height(&self, width: u16) -> u16 {
        self.channel_map_content()
            .map_or(NOTE_PANEL_ROWS, |(header, cells)| {
                let map = HyperHeatmap::new(&cells).rows(width.saturating_sub(2));
                framed(header.len()).saturating_add(map)
            })
    }

    pub(super) fn render_channel_map(&self, frame: &mut Frame, area: Rect) {
        // The band is on the toolbar, with the keys that change it.
        let block = panel_block(" Channels ", self.focused);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let (header, cells) = match self.channel_map_content() {
            Ok(content) => content,
            Err(note) => {
                render_notice(frame, inner, &note);
                return;
            }
        };
        let rows = u16::try_from(header.len()).unwrap_or(u16::MAX);
        let [top, map] =
            Layout::vertical([Constraint::Length(rows), Constraint::Min(1)]).areas(inner);
        frame.render_widget(Paragraph::new(header), top);
        frame.render_widget(HyperHeatmap::new(&cells), map);
    }

    /// The channel map's summary lines and cells, or the note it shows in
    /// their place: why the channel list is not on hand.
    fn channel_map_content(&self) -> Result<(Vec<Line<'static>>, Vec<HeatmapCell>), String> {
        if self.session_missing() {
            return Err(NO_SESSION_CHANNELS.to_owned());
        }

        let occupancy = self.channel_occupancy();
        if occupancy.is_empty() {
            return Err(match &self.channels_error {
                Some(error) => format!("Channel list unavailable: {error}"),
                None => "Loading the channel list\u{2026}".to_owned(),
            });
        }

        let cells: Vec<HeatmapCell> = occupancy
            .iter()
            .map(|row| HeatmapCell {
                label: format!("{}{}", row.channel, if row.dfs { "*" } else { "" }),
                your_count: row.your_count,
                neighbor_count: row.neighbor_count,
                signal: row.signal,
                conflict: row.conflict,
            })
            .collect();
        let yours: usize = occupancy.iter().map(|row| row.your_count).sum();
        let theirs: usize = occupancy.iter().map(|row| row.neighbor_count).sum();
        let conflicts = occupancy.iter().filter(|row| row.conflict).count();

        let count = |count: usize, one: &str, many: &str| {
            format!("{count} {}", if count == 1 { one } else { many })
        };
        let scope = self.focused_ap.as_ref().map_or_else(
            || format!(" {}", count(yours, "radio of yours", "of your radios")),
            |focused| format!(" {} · {}", focused.label, count(yours, "radio", "radios")),
        );
        let mut header = vec![Line::from(vec![
            Span::styled(scope, secondary()),
            Span::styled(
                format!(" · {}", count(theirs, "neighbor", "neighbors")),
                secondary(),
            ),
            Span::styled(
                if conflicts > 0 {
                    format!(" · {conflicts} shared")
                } else {
                    String::new()
                },
                Style::default()
                    .fg(theme::warning())
                    .add_modifier(Modifier::BOLD),
            ),
        ])];
        if let Some(error) = &self.neighbors_error {
            header.push(Line::from(Span::styled(
                format!(" Neighbor scan unavailable: {error}"),
                muted(),
            )));
        } else if occupancy.iter().any(|row| row.dfs) {
            header.push(Line::from(Span::styled(
                " * DFS channel: the radio moves off it when it hears radar",
                muted(),
            )));
        }
        header.push(Line::from(""));
        Ok((header, cells))
    }

    /// What the SSID authenticates with. A passphrase itself is never
    /// drawn.
    fn passphrase_label(&self, ssid: &WifiBroadcast) -> String {
        let keys = self.ssid_preshared_keys(ssid);
        match ssid.security {
            WifiSecurityMode::Open => "none".to_owned(),
            WifiSecurityMode::Wpa2Enterprise
            | WifiSecurityMode::Wpa3Enterprise
            | WifiSecurityMode::Wpa2Wpa3Enterprise => "RADIUS sign-in".to_owned(),
            _ if keys == 1 => "1 per-network key".to_owned(),
            _ if keys > 1 => format!("{keys} per-network keys"),
            _ => "not shown (P to set)".to_owned(),
        }
    }

    /// The SSID detail's rows: what the list says, then the settings only
    /// the full record carries.
    fn ssid_detail_rows(&self, ssid: &WifiBroadcast) -> Vec<(&'static str, String)> {
        let yes_no = |value: bool| if value { "yes" } else { "no" }.to_owned();
        // A setting the list leaves out is drawn as what is known of it,
        // never as "no" by default.
        let setting = |pick: Pick| {
            match self.ssid_setting(ssid, pick) {
                Setting::On => "yes",
                Setting::Off => "no",
                Setting::Absent => "─",
                Setting::Reading => "\u{2026}",
                Setting::Unknown => "unknown",
            }
            .to_owned()
        };
        let network = ssid.network_id.as_ref().map_or_else(
            || "─".to_owned(),
            |id| {
                self.network_names
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| id.to_string())
            },
        );
        let mut rows = vec![
            ("State", enabled_label(ssid).to_owned()),
            ("Clients", count_label(self.ssid_client_count(&ssid.name))),
            ("Security", security_label(ssid.security).to_owned()),
            ("Passphrase", self.passphrase_label(ssid)),
            ("Bands", bands_label(ssid)),
            ("Network", network),
            (
                "Type",
                match ssid.broadcast_type {
                    WifiBroadcastType::Standard => "Standard",
                    WifiBroadcastType::IotOptimized => "IoT optimized",
                }
                .to_owned(),
            ),
            ("Hidden", setting(|ssid| ssid.hidden)),
            ("Isolation", setting(|ssid| ssid.client_isolation)),
            ("Steering", setting(|ssid| ssid.band_steering)),
            ("Fast roam", setting(|ssid| ssid.fast_roaming)),
            ("MLO", setting(|ssid| ssid.mlo_enabled)),
            ("Hotspot", yes_no(ssid.hotspot_enabled)),
        ];
        // Controllers older than these two locks never report them.
        let locks: [(&str, Pick); 2] = [
            ("Ch 6 lock", |ssid| ssid.channel2g_locked_to6),
            ("DTIM 3 lock", |ssid| ssid.dtim_period2g_locked_to3),
        ];
        for (label, pick) in locks {
            if matches!(self.ssid_setting(ssid, pick), Setting::On | Setting::Off) {
                rows.push((label, setting(pick)));
            }
        }
        if let Some(origin) = &ssid.origin {
            let origin = match origin {
                EntityOrigin::UserDefined => "user defined",
                EntityOrigin::SystemDefined => "system defined",
                EntityOrigin::Derived => "derived",
                EntityOrigin::Orchestrated => "orchestrated",
            };
            rows.push(("Origin", origin.to_owned()));
        }
        rows
    }

    /// Rows the SSID detail takes in a panel `width` wide, frame
    /// included: its record in one column, or `folded` into two where
    /// the panel has the width for them.
    pub(super) fn ssid_detail_height(&self, width: u16, folded: bool) -> u16 {
        self.selected_ssid().map_or(NOTE_PANEL_ROWS, |ssid| {
            let inner = usize::from(width.saturating_sub(2));
            let rows = if folded { 0 } else { usize::MAX };
            framed(self.ssid_detail_lines(ssid, inner, rows).len())
        })
    }

    pub(super) fn render_ssid_detail(&self, frame: &mut Frame, area: Rect) {
        let block = panel_block(" SSID Detail ", self.focused);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let Some(ssid) = self.selected_ssid() else {
            render_notice(frame, inner, "No SSID selected");
            return;
        };
        let lines =
            self.ssid_detail_lines(ssid, usize::from(inner.width), usize::from(inner.height));
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }

    /// The SSID detail's lines for a panel `width` by `height` inside its
    /// frame.
    fn ssid_detail_lines(
        &self,
        ssid: &WifiBroadcast,
        width: usize,
        height: usize,
    ) -> Vec<Line<'static>> {
        let rows = self.ssid_detail_rows(ssid);

        let pair = |(label, value): &(&str, String)| {
            [
                Span::styled(format!(" {label:<12}"), muted()),
                Span::styled(value.clone(), secondary()),
            ]
        };
        let mut lines = vec![Line::from(Span::styled(
            format!(" {}", ssid.name),
            theme::title_style(),
        ))];
        // Under the name, where a short panel cannot push it out of view.
        if let Some(error) = self.ssid_detail_error(ssid) {
            lines.push(Line::from(Span::styled(
                format!(" Settings unavailable: {error}. Retrying."),
                Style::default().fg(theme::warning()),
            )));
        }
        // A short panel folds the record into two columns, so none of it
        // falls off the bottom.
        let room = height.saturating_sub(lines.len() + 1);
        let half = width / 2;
        if rows.len() > room && half >= 28 {
            let (left, right) = rows.split_at(rows.len().div_ceil(2));
            for (index, left) in left.iter().enumerate() {
                let mut spans = pair(left).to_vec();
                if let Some(right) = right.get(index) {
                    let used: usize = spans.iter().map(Span::width).sum();
                    spans.push(Span::raw(" ".repeat(half.saturating_sub(used))));
                    spans.extend(pair(right));
                }
                lines.push(Line::from(spans));
            }
        } else {
            lines.extend(rows.iter().map(|row| Line::from(pair(row).to_vec())));
        }
        lines.push(Line::from(pair(&("ID", ssid.id.to_string())).to_vec()));
        lines
    }
}

/// The clients of an AP, as many as fit in `room` rows under a heading,
/// then how many did not.
fn client_lines(clients: &[std::sync::Arc<unifly_api::Client>], room: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(
        format!(" Clients ({})", clients.len()),
        theme::table_header(),
    ))];
    if clients.is_empty() {
        lines.push(Line::from(Span::styled("  None connected", muted())));
        return lines;
    }
    let shown = if clients.len() > room {
        room.saturating_sub(1)
    } else {
        clients.len()
    };
    let names: Vec<String> = clients
        .iter()
        .take(shown)
        .map(|client| display_client_name(client))
        .collect();
    let name_width = names
        .iter()
        .map(|name| Span::raw(name.as_str()).width())
        .max()
        .unwrap_or(0)
        .min(24);
    for (client, name) in clients.iter().zip(names) {
        let wireless = client.wireless.as_ref();
        let signal = wireless.and_then(|wireless| wireless.signal_dbm);
        let health = wireless.and_then(|wireless| wireless.satisfaction);
        lines.push(Line::from(vec![
            Span::styled(format!("  {name:<name_width$}  "), secondary()),
            signal_bars::signal_span(signal),
            Span::styled(
                health.map_or_else(String::new, |health| format!("  {health}%")),
                Style::default().fg(health_color(health)),
            ),
        ]));
    }
    if shown < clients.len() {
        lines.push(Line::from(Span::styled(
            format!("  +{} more", clients.len() - shown),
            muted(),
        )));
    }
    lines
}

/// A client's live link, from the controller's WiFi experience record.
fn experience_lines(detail: &serde_json::Value) -> Vec<Line<'static>> {
    let signal = json_i32(detail, &["signal"]);
    let noise = json_i32(detail, &["noise"]);
    let channel = json_u64(detail, &["channel"]);
    let width = json_u64(detail, &["channel_width"]);
    let band = json_string(detail, &["wlan_band", "band"])
        .map_or_else(|| "─".to_string(), |band| band_name(&band));
    let protocol = json_string(detail, &["radio_protocol"]).unwrap_or_else(|| "─".to_string());
    let experience = json_u64(detail, &["experience", "wifi_experience"]);
    let tx = json_u64(detail, &["link_upload_rate_kbps"]);
    let rx = json_u64(detail, &["link_download_rate_kbps"]);
    let dbm = |value: Option<i32>| value.map_or_else(|| "─".to_string(), |v| format!("{v} dBm"));

    let mut lines = vec![
        Line::from(vec![
            Span::styled(" Health ", muted()),
            Span::styled(
                experience.map_or_else(|| "─".to_string(), |value| format!("{value}/100")),
                Style::default().fg(health_color(
                    experience.and_then(|value| u8::try_from(value).ok()),
                )),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Signal ", muted()),
            signal_bars::signal_span(signal),
            Span::styled(
                format!("  {}  Noise {}", dbm(signal), dbm(noise)),
                secondary(),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Radio  ", muted()),
            Span::styled(
                format!(
                    "{band} · ch {}{}",
                    channel.map_or_else(|| "─".to_string(), |value| value.to_string()),
                    width.map_or_else(String::new, |value| format!(" ({value} MHz)"))
                ),
                secondary(),
            ),
            Span::styled("  Proto ", muted()),
            Span::styled(protocol, secondary()),
        ]),
        Line::from(vec![
            Span::styled(" TX/RX  ", muted()),
            Span::styled(
                format!("{}/{} Mbps", kbps_to_mbps(tx), kbps_to_mbps(rx)),
                secondary(),
            ),
        ]),
    ];

    lines.extend(uplink_line(detail));

    let neighbors: Vec<_> = detail
        .get("nearest_neighbors")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .take(5)
        .collect();
    if !neighbors.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " Nearest neighbors",
            theme::table_header(),
        )));
    }
    for neighbor in neighbors {
        let label = json_string(neighbor, &["ssid", "display_name", "bssid"])
            .unwrap_or_else(|| "neighbor".to_string());
        let channel = json_u64(neighbor, &["channel"])
            .map_or_else(|| "─".to_string(), |value| value.to_string());
        lines.push(Line::from(vec![
            Span::styled(format!("  {label}"), secondary()),
            Span::styled(
                format!("  ch {channel}  {}", dbm(json_i32(neighbor, &["signal"]))),
                muted(),
            ),
        ]));
    }
    lines
}

/// The devices between the client and the gateway, nearest first, each
/// with the experience score the controller gives its link.
fn uplink_line(detail: &serde_json::Value) -> Option<Line<'static>> {
    let hops: Vec<String> = detail
        .get("uplink_devices")
        .and_then(|value| value.as_array())?
        .iter()
        .filter_map(|hop| {
            let name = json_string(hop, &["display_name", "name", "mac"])?;
            Some(match json_u64(hop, &["experience", "wifi_experience"]) {
                Some(score) => format!("{name} ({score})"),
                None => name,
            })
        })
        .collect();
    (!hops.is_empty()).then(|| {
        Line::from(vec![
            Span::styled(" Uplink ", muted()),
            Span::styled(hops.join(" \u{203A} "), secondary()),
        ])
    })
}

fn ap_heading(ap: &Device) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!(" {}", display_device_name(ap)),
        theme::title_style(),
    )];
    if let Some(model) = &ap.model {
        spans.push(Span::styled(format!("  {model}"), secondary()));
    }
    spans.push(Span::styled(
        format!("  {}", state_label(ap.state)),
        Style::default().fg(state_color(ap.state)),
    ));
    Line::from(spans)
}

/// One radio: its band and channel, then how busy the air is and how
/// often it has to resend.
fn radio_line(radio: &unifly_api::model::Radio) -> Line<'static> {
    let band = band_from_frequency(radio.frequency_ghz).label();
    let channel = radio.channel.map_or_else(
        || "no channel".to_owned(),
        |channel| match radio.channel_width_mhz {
            Some(width) => format!("ch {channel} · {width} MHz"),
            None => format!("ch {channel}"),
        },
    );
    let busy = radio.channel_utilization_pct;
    let busy_style = match busy {
        Some(pct) if pct >= 70.0 => Style::default().fg(theme::error()),
        Some(pct) if pct >= 40.0 => Style::default().fg(theme::warning()),
        _ => secondary(),
    };
    let retries = radio.tx_retries_pct;
    let retry_style = match retries {
        Some(pct) if pct >= 20.0 => Style::default().fg(theme::error()),
        Some(pct) if pct >= 10.0 => Style::default().fg(theme::warning()),
        _ => secondary(),
    };
    let mut spans = vec![
        Span::styled(
            format!("  {band:<8}"),
            Style::default().fg(theme::accent_secondary()),
        ),
        Span::styled(format!("{channel:<18}"), secondary()),
        Span::styled(" busy ", muted()),
        Span::styled(format!("{:<5}", percent(busy)), busy_style),
        Span::styled("retries ", muted()),
        Span::styled(format!("{:<5}", percent(retries)), retry_style),
    ];
    if let Some(standard) = &radio.wlan_standard {
        spans.push(Span::styled(standard.clone(), muted()));
    }
    Line::from(spans)
}

pub(super) fn enabled_label(ssid: &WifiBroadcast) -> &'static str {
    if ssid.enabled { "On" } else { "Off" }
}

pub(super) fn security_label(mode: WifiSecurityMode) -> &'static str {
    match mode {
        WifiSecurityMode::Open => "Open",
        WifiSecurityMode::Wpa2Personal => "WPA2",
        WifiSecurityMode::Wpa3Personal => "WPA3",
        WifiSecurityMode::Wpa2Wpa3Personal => "WPA2/WPA3",
        WifiSecurityMode::Wpa2Enterprise => "WPA2 Enterprise",
        WifiSecurityMode::Wpa3Enterprise => "WPA3 Enterprise",
        WifiSecurityMode::Wpa2Wpa3Enterprise => "WPA2/WPA3 Enterprise",
        _ => "Other",
    }
}

/// The bands an SSID is broadcast on, as "2.4 · 5 · 6 GHz".
pub(super) fn bands_label(ssid: &WifiBroadcast) -> String {
    let numbers: Vec<&str> = WifiBand::ALL
        .into_iter()
        .filter(|band| {
            ssid.frequencies_ghz
                .iter()
                .any(|frequency| band_from_frequency(*frequency) == *band)
        })
        .map(|band| band.label().trim_end_matches(" GHz"))
        .collect();
    if numbers.is_empty() {
        // An IoT-optimized broadcast carries no band list: it is 2.4 GHz.
        return match ssid.broadcast_type {
            WifiBroadcastType::IotOptimized => WifiBand::TwoGhz.label().to_owned(),
            WifiBroadcastType::Standard => "─".to_owned(),
        };
    }
    format!("{} GHz", numbers.join(" · "))
}
