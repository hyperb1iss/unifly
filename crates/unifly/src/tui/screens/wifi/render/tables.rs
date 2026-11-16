//! The WiFi screen's tables: AP health, wireless clients, SSIDs, neighbor
//! APs, and roam history, each sized to what its cells hold and drawn
//! straight into the screen's frame. A table with no rows draws one line
//! saying why instead of a bare header.

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Cell, Row, Table};
use std::sync::Arc;

use unifly_api::{Client, Device, DeviceState};

use super::panels::{bands_label, enabled_label, security_label};
use super::{
    band_label, health_color, health_label, kbps_to_mbps, render_notice, signal_color, signal_text,
    state_color, state_label,
};
use crate::tui::action::{WifiBand, WifiSortField};
use crate::tui::screens::wifi::WifiScreen;
use crate::tui::screens::wifi::ssid_detail::Setting;
use crate::tui::screens::wifi::state::{
    band_from_frequency, display_client_name, display_device_name, neighbor_name,
};
use crate::tui::table::{Column, plan_columns};
use crate::tui::theme;

/// Panel width below which the AP table folds its three band columns into
/// one compact Channels column.
const AP_BANDS_TABLE: u16 = 70;

/// Panel width below which the wireless client table keeps only the client,
/// signal, and health.
const CLIENT_DETAIL_TABLE: u16 = 72;

/// Panel width from which the wireless client table adds each client's
/// SSID, band, and channel.
const CLIENT_RADIO_TABLE: u16 = 100;

/// Panel width from which the SSID table adds each SSID's network and
/// whether it is hidden.
const SSID_DETAIL_TABLE: u16 = 72;

/// The wireless client table's text, a column at a time.
struct ClientCells {
    names: Vec<String>,
    aps: Vec<String>,
    ssids: Vec<String>,
    radios: Vec<String>,
    rates: Vec<String>,
}

/// A header with an arrow on the column the table is sorted by: down for
/// largest first, up for smallest first.
fn sorted_header(label: &str, sort: WifiSortField, field: WifiSortField) -> String {
    if sort != field {
        return label.to_owned();
    }
    let descending = matches!(
        field,
        WifiSortField::Health | WifiSortField::Clients | WifiSortField::Signal
    );
    format!(
        "{label} {}",
        if descending { '\u{25BE}' } else { '\u{25B4}' }
    )
}

/// `text` as it fits column `index` of a planned table: cut text ends in
/// an ellipsis instead of stopping mid-word.
fn fit(widths: &[Constraint], index: usize, text: String) -> String {
    let Some(Constraint::Length(width)) = widths.get(index) else {
        return text;
    };
    let width = usize::from(*width);
    if Span::raw(text.as_str()).width() <= width {
        return text;
    }
    let mut cut = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let cells = Span::raw(ch.encode_utf8(&mut [0; 4]).to_owned()).width();
        if used + cells + 1 > width {
            break;
        }
        cut.push(ch);
        used += cells;
    }
    cut.push('\u{2026}');
    cut
}

/// A count, or a dash when the controller cannot report it.
pub(super) fn count_label(count: Option<usize>) -> String {
    count.map_or_else(|| "─".to_owned(), |count| count.to_string())
}

/// How long ago a neighbor was last heard, from its age in seconds.
fn age_label(age: Option<i64>) -> String {
    match age {
        None => "─".to_owned(),
        Some(secs) if secs < 60 => format!("{}s", secs.max(0)),
        Some(secs) if secs < 3600 => format!("{}m", secs / 60),
        Some(secs) => format!("{}h", secs / 3600),
    }
}

impl WifiScreen {
    /// The Health cell of `ap`. An AP that is not online shows what it is
    /// doing instead of a score it cannot have.
    fn ap_health_cell(&self, ap: &Device) -> (String, Color) {
        if ap.state == DeviceState::Online {
            let health = self.ap_health(ap);
            (health_label(health), health_color(health))
        } else {
            (state_label(ap.state).to_owned(), state_color(ap.state))
        }
    }

    pub(super) fn render_ap_table(&self, frame: &mut Frame, inner: Rect) {
        let aps = self.ap_devices();
        if aps.is_empty() {
            render_notice(frame, inner, &self.ap_empty_reason());
            return;
        }

        let selected = self.ap_table_state.selected().unwrap_or(0);
        let compact = inner.width < AP_BANDS_TABLE;
        let sort = self.ap_sort;
        let focus = self.focused_ap.as_ref().map(|focused| &focused.id);

        let names: Vec<String> = aps.iter().map(|ap| display_device_name(ap)).collect();
        let clients: Vec<String> = aps
            .iter()
            .map(|ap| count_label(self.ap_client_count(ap)))
            .collect();
        let health: Vec<(String, Color)> = aps.iter().map(|ap| self.ap_health_cell(ap)).collect();
        let bands: Vec<[Vec<String>; 3]> = aps
            .iter()
            .map(|ap| radio_channels(ap, self.ap_channel_profile(ap)))
            .collect();

        let mut headers = vec![
            sorted_header("AP", sort, WifiSortField::Name),
            sorted_header("Clients", sort, WifiSortField::Clients),
            sorted_header("Health", sort, WifiSortField::Health),
        ];
        let mut columns = vec![
            Column::fit(&headers[0], &names, 32).pad(1).shrink_to(10),
            Column::fit(&headers[1], &clients, 9),
            Column::fit(&headers[2], health.iter().map(|(label, _)| label), 11),
        ];
        let band_cells: Vec<Vec<String>> = if compact {
            let header = sorted_header("Channels", sort, WifiSortField::Channel);
            let joined: Vec<String> = bands.iter().map(compact_channels).collect();
            columns.push(Column::fit(&header, &joined, 24).shrink_to(8));
            headers.push(header);
            joined.into_iter().map(|label| vec![label]).collect()
        } else {
            let labelled: Vec<Vec<String>> = bands
                .iter()
                .map(|bands| bands.iter().map(|labels| band_cell(labels)).collect())
                .collect();
            for (slot, band) in WifiBand::ALL.into_iter().enumerate() {
                // The channel sort orders by each AP's lowest channel,
                // which is its 2.4 GHz radio.
                let header = if slot == 0 {
                    sorted_header(band.label(), sort, WifiSortField::Channel)
                } else {
                    band.label().to_owned()
                };
                let values = labelled.iter().map(|row| row[slot].as_str());
                columns.push(Column::fit(&header, values, 32).shrink_to(7));
                headers.push(header);
            }
            labelled
        };

        let widths = plan_columns(&columns, inner.width);
        let rows: Vec<Row> = aps
            .iter()
            .zip(names)
            .zip(clients)
            .zip(health)
            .zip(band_cells)
            .enumerate()
            .map(
                |(index, ((((ap, name), clients), (health, color)), bands))| {
                    let is_selected = index == selected;
                    let name_style = if focus == Some(&ap.id) {
                        Style::default()
                            .fg(theme::accent_secondary())
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    let mut cells = vec![
                        Cell::from(fit(&widths, 0, format!("{}{name}", marker(is_selected))))
                            .style(name_style),
                        Cell::from(clients),
                        Cell::from(health).style(Style::default().fg(color)),
                    ];
                    cells.extend(bands.into_iter().map(|label| {
                        let style = if label == "─" {
                            Style::default().fg(theme::text_muted())
                        } else {
                            Style::default()
                        };
                        Cell::from(label).style(style)
                    }));
                    Row::new(cells).style(row_style(is_selected))
                },
            )
            .collect();

        let len = rows.len();
        let table = Table::new(rows, widths)
            .header(header_row(&headers))
            .row_highlight_style(theme::table_selected());

        let state = self
            .ap_scroll
            .render(frame, table, inner, &self.ap_table_state, len);
        self.hits.borrow_mut().push_table_rows(inner, &state, len);
    }

    /// The text of each client row, a column at a time.
    fn client_cells(&self, clients: &[&Arc<Client>]) -> ClientCells {
        let names: Vec<String> = clients
            .iter()
            .map(|client| {
                let name = display_client_name(client);
                if client.blocked {
                    format!("{name} (blocked)")
                } else {
                    name
                }
            })
            .collect();
        let aps: Vec<String> = clients
            .iter()
            .map(|client| self.client_ap_name(client))
            .collect();
        let ssids: Vec<String> = clients
            .iter()
            .map(|client| {
                client
                    .wireless
                    .as_ref()
                    .and_then(|wireless| wireless.ssid.clone())
                    .unwrap_or_else(|| "─".into())
            })
            .collect();
        let radios: Vec<String> = clients
            .iter()
            .map(|client| {
                let wireless = client.wireless.as_ref();
                let channel = wireless.and_then(|wireless| wireless.channel);
                let band = wireless
                    .and_then(|wireless| wireless.frequency_ghz)
                    .map(band_from_frequency);
                match (band, channel) {
                    (Some(band), Some(channel)) => format!("{} · ch {channel}", band_label(band)),
                    (None, Some(channel)) => format!("ch {channel}"),
                    (Some(band), None) => band_label(band).to_owned(),
                    (None, None) => "─".into(),
                }
            })
            .collect();
        let rates: Vec<String> = clients
            .iter()
            .map(|client| {
                let wireless = client.wireless.as_ref();
                let tx = wireless.and_then(|wireless| wireless.tx_rate_kbps);
                let rx = wireless.and_then(|wireless| wireless.rx_rate_kbps);
                format!("{}/{} Mbps", kbps_to_mbps(tx), kbps_to_mbps(rx))
            })
            .collect();

        ClientCells {
            names,
            aps,
            ssids,
            radios,
            rates,
        }
    }

    pub(super) fn render_client_table(&self, frame: &mut Frame, inner: Rect) {
        let clients = self.wireless_clients();
        if clients.is_empty() {
            render_notice(frame, inner, &self.client_empty_reason());
            return;
        }

        let selected = self.client_table_state.selected().unwrap_or(0);
        let detailed = inner.width >= CLIENT_DETAIL_TABLE;
        let with_radio = inner.width >= CLIENT_RADIO_TABLE;
        let sort = self.client_sort;

        let ClientCells {
            names,
            aps,
            ssids,
            radios,
            rates,
        } = self.client_cells(&clients);

        let mut headers = vec![sorted_header("Client", sort, WifiSortField::Name)];
        let mut columns = vec![Column::fit(&headers[0], &names, 32).pad(1).shrink_to(10)];
        if detailed {
            headers.push("AP".to_owned());
            columns.push(Column::fit("AP", &aps, 24).shrink_to(8));
        }
        if with_radio {
            headers.extend(["SSID".to_owned(), "Radio".to_owned()]);
            columns.push(Column::fit("SSID", &ssids, 20).shrink_to(6));
            columns.push(Column::fit("Radio", &radios, 16));
        }
        headers.extend([
            sorted_header("Signal", sort, WifiSortField::Signal),
            sorted_header("Health", sort, WifiSortField::Health),
        ]);
        columns.extend([Column::fixed(8), Column::fixed(8)]);
        if detailed {
            headers.push("TX/RX".to_owned());
            columns.push(Column::fit("TX/RX", &rates, 16).at_least(14));
        }

        let widths = plan_columns(&columns, inner.width);
        let rows: Vec<Row> = clients
            .iter()
            .zip(names)
            .zip(aps.into_iter().zip(ssids).zip(radios).zip(rates))
            .enumerate()
            .map(|(index, ((client, name), (((ap, ssid), radio), rate)))| {
                let is_selected = index == selected;
                let wireless = client.wireless.as_ref();
                let signal = wireless.and_then(|wireless| wireless.signal_dbm);
                let health = wireless.and_then(|wireless| wireless.satisfaction);
                let name_style = if client.blocked {
                    Style::default().fg(theme::error())
                } else {
                    Style::default()
                };

                let name = fit(&widths, 0, format!("{}{name}", marker(is_selected)));
                let mut cells = vec![Cell::from(name).style(name_style)];
                if detailed {
                    cells.push(Cell::from(fit(&widths, 1, ap)));
                }
                if with_radio {
                    cells.push(Cell::from(fit(&widths, 2, ssid)));
                    cells.push(Cell::from(radio));
                }
                cells.extend([
                    Cell::from(signal_text(signal))
                        .style(Style::default().fg(signal_color(signal))),
                    Cell::from(health_label(health))
                        .style(Style::default().fg(health_color(health))),
                ]);
                if detailed {
                    cells.push(Cell::from(rate));
                }
                Row::new(cells).style(row_style(is_selected))
            })
            .collect();

        let len = rows.len();
        let table = Table::new(rows, widths)
            .header(header_row(&headers))
            .row_highlight_style(theme::table_selected());

        let state = self
            .client_scroll
            .render(frame, table, inner, &self.client_table_state, len);
        self.hits.borrow_mut().push_table_rows(inner, &state, len);
    }

    pub(super) fn render_ssid_table(&self, frame: &mut Frame, inner: Rect) {
        let ssids = self.visible_ssids();
        if ssids.is_empty() {
            render_notice(frame, inner, &self.ssid_empty_reason());
            return;
        }

        let selected = self.ssid_table_state.selected().unwrap_or(0);
        let cells: Vec<[String; 7]> = ssids
            .iter()
            .enumerate()
            .map(|(index, ssid)| {
                let network = ssid.network_id.as_ref().map_or_else(
                    || "─".to_owned(),
                    |id| {
                        self.network_names
                            .get(id)
                            .cloned()
                            .unwrap_or_else(|| "─".to_owned())
                    },
                );
                [
                    format!("{}{}", marker(index == selected), ssid.name),
                    enabled_label(ssid).to_owned(),
                    count_label(self.ssid_client_count(&ssid.name)),
                    security_label(ssid.security).to_owned(),
                    bands_label(ssid),
                    network,
                    match self.ssid_setting(ssid, |ssid| ssid.hidden) {
                        Setting::On => "hidden",
                        Setting::Off | Setting::Absent => "─",
                        Setting::Reading => "\u{2026}",
                        Setting::Unknown => "?",
                    }
                    .to_owned(),
                ]
            })
            .collect();

        // Beside its detail panel the table keeps what tells SSIDs apart;
        // the panel carries the rest.
        let shown = if inner.width >= SSID_DETAIL_TABLE {
            7
        } else {
            5
        };
        let headers = [
            "SSID", "State", "Clients", "Security", "Bands", "Network", "Hidden",
        ]
        .map(str::to_owned);
        let limits = [(32, 10), (5, 5), (7, 7), (20, 8), (16, 16), (24, 7), (6, 6)];
        let columns: Vec<Column> = limits
            .into_iter()
            .enumerate()
            .take(shown)
            .map(|(index, (max, floor))| {
                Column::fit(&headers[index], cells.iter().map(|c| &c[index]), max).shrink_to(floor)
            })
            .collect();
        let headers = &headers[..shown];

        let widths = plan_columns(&columns, inner.width);
        let rows: Vec<Row> = cells
            .into_iter()
            .zip(&ssids)
            .enumerate()
            .map(|(index, (cells, ssid))| {
                let state_style = Style::default().fg(if ssid.enabled {
                    theme::success()
                } else {
                    theme::text_muted()
                });
                let row = cells
                    .into_iter()
                    .take(shown)
                    .enumerate()
                    .map(|(column, text)| {
                        let muted = text == "─" || (!ssid.enabled && column != 1);
                        let style = if column == 1 {
                            state_style
                        } else if muted {
                            Style::default().fg(theme::text_muted())
                        } else {
                            Style::default()
                        };
                        Cell::from(fit(&widths, column, text)).style(style)
                    });
                Row::new(row).style(row_style(index == selected))
            })
            .collect();

        let len = rows.len();
        let table = Table::new(rows, widths)
            .header(header_row(headers))
            .row_highlight_style(theme::table_selected());

        let state = self
            .ssid_scroll
            .render(frame, table, inner, &self.ssid_table_state, len);
        self.hits.borrow_mut().push_table_rows(inner, &state, len);
    }

    pub(super) fn render_neighbors(&self, frame: &mut Frame, inner: Rect) {
        // Without a session the scan cannot be this controller's.
        let neighbors = self.visible_neighbors();
        if neighbors.is_empty() || self.session_missing() {
            render_notice(frame, inner, &self.neighbor_empty_reason());
            return;
        }

        let selected = self.neighbor_table_state.selected().unwrap_or(0);
        let sort = self.neighbor_sort;
        let cells: Vec<[String; 7]> = neighbors
            .iter()
            .enumerate()
            .map(|(index, neighbor)| {
                let signal = neighbor.signal.or(neighbor.rssi);
                let name = neighbor_name(neighbor).unwrap_or("(hidden)");
                // Ahead of the name, where a narrow column cannot cut it.
                let rogue = if neighbor.is_rogue { "[rogue] " } else { "" };
                [
                    format!("{}{rogue}{name}", marker(index == selected)),
                    neighbor
                        .channel
                        .map_or_else(|| "─".to_string(), |channel| channel.to_string()),
                    signal.map_or_else(|| "─".to_string(), |dbm| format!("{dbm} dBm")),
                    neighbor.security.clone().unwrap_or_else(|| "─".to_string()),
                    self.neighbor_observer(neighbor),
                    age_label(neighbor.age),
                    neighbor.bssid.clone(),
                ]
            })
            .collect();

        // The BSSID is the widest and least read, so it is the first to go
        // when the panel is narrow.
        let headers = [
            sorted_header("SSID", sort, WifiSortField::Name),
            sorted_header("Ch", sort, WifiSortField::Channel),
            sorted_header("Signal", sort, WifiSortField::Signal),
            sorted_header("Security", sort, WifiSortField::Security),
            "Heard by".to_owned(),
            "Age".to_owned(),
            "BSSID".to_owned(),
        ];
        let column = |index: usize, max: u16| {
            Column::fit(&headers[index], cells.iter().map(|c| &c[index]), max)
        };
        let columns = [
            column(0, 32).shrink_to(10),
            column(1, 5),
            column(2, 9),
            column(3, 24).shrink_to(8),
            column(4, 20).shrink_to(8),
            column(5, 4),
            Column::fixed(17),
        ];

        let widths = plan_columns(&columns, inner.width);
        let rows: Vec<Row> = cells
            .into_iter()
            .zip(&neighbors)
            .enumerate()
            .map(|(index, (cells, neighbor))| {
                let hidden = neighbor_name(neighbor).is_none();
                let row = cells.into_iter().enumerate().map(|(column, text)| {
                    let style = if column == 0 && neighbor.is_rogue {
                        Style::default().fg(theme::warning())
                    } else if column == 0 && hidden {
                        Style::default().fg(theme::text_muted())
                    } else {
                        Style::default()
                    };
                    Cell::from(fit(&widths, column, text)).style(style)
                });
                Row::new(row).style(row_style(index == selected))
            })
            .collect();

        let len = rows.len();
        let table = Table::new(rows, widths)
            .header(header_row(&headers))
            .row_highlight_style(theme::table_selected());

        let state =
            self.neighbor_scroll
                .render(frame, table, inner, &self.neighbor_table_state, len);
        self.hits.borrow_mut().push_table_rows(inner, &state, len);
    }

    pub(super) fn render_roaming(&self, frame: &mut Frame, inner: Rect) {
        if let Some(reason) = self.roam_empty_reason() {
            render_notice(frame, inner, &reason);
            return;
        }
        let rows = &self.roam_rows;

        let headers = ["Time", "Event", "From", "To", "Signal", "Radio"].map(str::to_owned);
        let columns = [
            Column::fit("Time", rows.iter().map(|r| &r.time), 14),
            Column::fit("Event", rows.iter().map(|r| &r.event), 24).shrink_to(8),
            Column::fit("From", rows.iter().map(|r| &r.from_ap), 24).shrink_to(8),
            Column::fit("To", rows.iter().map(|r| &r.to_ap), 24).shrink_to(8),
            Column::fit("Signal", rows.iter().map(|r| &r.signal), 8),
            Column::fit("Radio", rows.iter().map(|r| &r.radio), 16),
        ];

        let widths = plan_columns(&columns, inner.width);
        let selected = self.roam_table_state.selected().unwrap_or(0);
        let table_rows: Vec<Row> = rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                Row::new(vec![
                    Cell::from(row.time.clone()),
                    Cell::from(fit(&widths, 1, row.event.clone())),
                    Cell::from(fit(&widths, 2, row.from_ap.clone())),
                    Cell::from(fit(&widths, 3, row.to_ap.clone())),
                    Cell::from(row.signal.clone()),
                    Cell::from(row.radio.clone()),
                ])
                .style(row_style(index == selected))
            })
            .collect();

        let len = table_rows.len();
        let table = Table::new(table_rows, widths)
            .header(header_row(&headers))
            .row_highlight_style(theme::table_selected());

        let state = self
            .roam_scroll
            .render(frame, table, inner, &self.roam_table_state, len);
        self.hits.borrow_mut().push_table_rows(inner, &state, len);
    }
}

/// Each band's channels on `ap`, slotted as [2.4 GHz, 5 GHz, 6 GHz], as
/// "channel · width" labels from its radio table. An AP without radio data
/// falls back to `profile`, the channel its clients report.
fn radio_channels(ap: &Device, profile: Option<(u32, WifiBand)>) -> [Vec<String>; 3] {
    let mut bands: [Vec<String>; 3] = Default::default();
    for radio in &ap.radios {
        let Some(channel) = radio.channel else {
            continue;
        };
        let label = radio.channel_width_mhz.map_or_else(
            || channel.to_string(),
            |width| format!("{channel} · {width} MHz"),
        );
        bands[band_slot(band_from_frequency(radio.frequency_ghz))].push(label);
    }
    if bands.iter().all(Vec::is_empty)
        && let Some((channel, band)) = profile
    {
        bands[band_slot(band)].push(channel.to_string());
    }
    bands
}

fn band_slot(band: WifiBand) -> usize {
    match band {
        WifiBand::TwoGhz => 0,
        WifiBand::FiveGhz => 1,
        WifiBand::SixGhz => 2,
    }
}

/// One band column's cell: its channels, or a dash when the AP has no radio
/// on that band.
fn band_cell(labels: &[String]) -> String {
    if labels.is_empty() {
        "─".into()
    } else {
        labels.join(", ")
    }
}

/// Every radio's channel number on one line ("1/100/133"), for the compact
/// AP table.
fn compact_channels(bands: &[Vec<String>; 3]) -> String {
    let channels: Vec<&str> = bands
        .iter()
        .flatten()
        .map(|label| label.split(' ').next().unwrap_or(label))
        .collect();
    if channels.is_empty() {
        "─".into()
    } else {
        channels.join("/")
    }
}

fn marker(is_selected: bool) -> &'static str {
    if is_selected { "▸" } else { " " }
}

fn row_style(is_selected: bool) -> Style {
    if is_selected {
        theme::table_selected()
    } else {
        theme::table_row()
    }
}

fn header_row(labels: &[String]) -> Row<'static> {
    Row::new(
        labels
            .iter()
            .map(|label| Cell::from(label.clone()).style(theme::table_header())),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use unifly_api::Device;

    use std::sync::Arc;

    use super::{compact_channels, radio_channels};
    use crate::tui::action::{Action, WifiBand};
    use crate::tui::screens::wifi::WifiScreen;
    use crate::tui::table::render_lines;

    fn ap(radios: &serde_json::Value) -> Device {
        serde_json::from_value(json!({
            "id": "ap", "mac": "aa:bb:cc:dd:ee:01", "ip": null, "wan_ipv6": null,
            "name": "ap-atrium", "model": "U7 Pro", "device_type": "AccessPoint",
            "state": "Online", "firmware_version": null, "firmware_updatable": false,
            "adopted_at": null, "provisioned_at": null, "last_seen": null, "serial": null,
            "supported": true, "ports": [], "radios": radios, "uplink_device_id": null,
            "uplink_device_mac": null, "has_switching": false, "has_access_point": true,
            "stats": {}, "client_count": null, "origin": null
        }))
        .expect("access point fixture should deserialize")
    }

    fn radio(frequency_ghz: f32, channel: u32, width: u32) -> serde_json::Value {
        json!({
            "frequency_ghz": frequency_ghz, "channel": channel, "channel_width_mhz": width,
            "wlan_standard": "802.11be", "tx_retries_pct": null,
            "channel_utilization_pct": null
        })
    }

    #[test]
    fn a_tap_inside_the_panel_under_the_table_changes_nothing() {
        use crate::tui::component::Component;
        use crate::tui::widgets::click::left_click;

        let named = |id: &str, name: &str| {
            let mut device = ap(&json!([radio(5.0, 36, 80)]));
            device.id = unifly_api::EntityId::Legacy(id.into());
            device.name = Some(name.into());
            Arc::new(device)
        };
        let mut screen = WifiScreen::new();
        screen.apply_action(&Action::DevicesUpdated(Arc::new(
            (0..8)
                .map(|index| named(&format!("ap-{index}"), &format!("ap-{index}")))
                .collect(),
        )));
        screen.overview_panel = crate::tui::screens::wifi::OverviewPanel::Detail;
        let selected = screen.ap_table_state.selected();

        // Under 110 columns the AP detail sits under the table.
        let rows = render_lines(80, 22, |frame, area| screen.render(frame, area));
        let panel = rows
            .iter()
            .position(|row| row.contains("AP Detail"))
            .expect("the panel is drawn");
        let inside = u16::try_from(panel + 2).expect("a row on screen");
        let action = screen.handle_mouse_input(left_click(40, inside));

        assert!(action.is_none(), "{action:?}");
        assert_eq!(screen.ap_table_state.selected(), selected);
        assert_eq!(
            screen.overview_panel,
            crate::tui::screens::wifi::OverviewPanel::Detail
        );
    }

    #[test]
    fn every_radio_lands_in_its_band() {
        let tri_band = ap(&json!([
            radio(2.4, 6, 20),
            radio(5.0, 116, 40),
            radio(6.0, 165, 160)
        ]));
        let bands = radio_channels(&tri_band, None);
        assert_eq!(
            bands,
            [
                vec!["6 · 20 MHz".to_owned()],
                vec!["116 · 40 MHz".to_owned()],
                vec!["165 · 160 MHz".to_owned()],
            ]
        );
        assert_eq!(compact_channels(&bands), "6/116/165");
    }

    #[test]
    fn dual_band_aps_leave_six_ghz_empty() {
        let dual_band = ap(&json!([radio(2.4, 11, 20), radio(5.0, 36, 80)]));
        let bands = radio_channels(&dual_band, Some((11, WifiBand::TwoGhz)));
        assert_eq!(bands[2], Vec::<String>::new());
        assert_eq!(bands[1], vec!["36 · 80 MHz".to_owned()]);
    }

    #[test]
    fn ap_table_lists_every_band_of_a_tri_band_ap() {
        let tri_band = ap(&json!([
            radio(2.4, 6, 20),
            radio(5.0, 116, 40),
            radio(6.0, 165, 160)
        ]));
        let mut screen = WifiScreen::new();
        screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![Arc::new(tri_band)])));

        let text =
            render_lines(120, 8, |frame, area| screen.render_ap_table(frame, area)).join("\n");
        for expected in [
            "2.4 GHz",
            "5 GHz",
            "6 GHz",
            "6 · 20 MHz",
            "116 · 40 MHz",
            "165 · 160 MHz",
        ] {
            assert!(text.contains(expected), "missing {expected:?}:\n{text}");
        }
    }

    #[test]
    fn aps_without_radio_data_fall_back_to_the_client_channel() {
        let bare = ap(&json!([]));
        let bands = radio_channels(&bare, Some((36, WifiBand::FiveGhz)));
        assert_eq!(bands, [vec![], vec!["36".to_owned()], vec![]]);
        assert_eq!(compact_channels(&radio_channels(&bare, None)), "─");
    }
}
