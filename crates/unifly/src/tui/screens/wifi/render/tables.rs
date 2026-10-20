//! The WiFi screen's tables: AP health, wireless clients, neighbor APs, and
//! roam history, each sized to what its cells hold.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Cell, Paragraph, Row, Table, Wrap};
use unifly_api::Device;

use super::{
    band_label, health_color, health_label, kbps_to_mbps, panel_block, signal_color, signal_text,
};
use crate::tui::action::WifiBand;
use crate::tui::screens::wifi::WifiScreen;
use crate::tui::screens::wifi::state::{
    band_from_frequency, band_from_neighbor, display_client_name, display_device_name,
    display_neighbor_name,
};
use crate::tui::table::{Column, plan_columns};
use crate::tui::theme;

/// Panel width below which the AP table folds its three band columns into
/// one compact Channels column.
const AP_BANDS_TABLE: u16 = 70;

/// Panel width below which the wireless client table keeps only the client,
/// signal, and health.
const CLIENT_DETAIL_TABLE: u16 = 72;

/// Panel width from which the wireless client table adds each client's band
/// and channel.
const CLIENT_RADIO_TABLE: u16 = 100;

impl WifiScreen {
    pub(super) fn render_ap_table(&self, frame: &mut Frame, area: Rect) {
        let block = panel_block(" AP Health ", self.focused);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let aps = self.ap_devices();
        let selected = self.ap_table_state.selected().unwrap_or(0);
        let compact = inner.width < AP_BANDS_TABLE;

        let names: Vec<String> = aps.iter().map(|ap| display_device_name(ap)).collect();
        let clients: Vec<String> = aps
            .iter()
            .map(|ap| self.ap_clients(ap).len().to_string())
            .collect();
        let health: Vec<Option<u8>> = aps.iter().map(|ap| self.ap_health(ap)).collect();
        let bands: Vec<[Vec<String>; 3]> = aps
            .iter()
            .map(|ap| radio_channels(ap, self.ap_channel_profile(ap)))
            .collect();

        let mut headers = vec!["AP", "Clients", "Health"];
        let mut columns = vec![
            Column::fit("AP", &names, 32).pad(1).shrink_to(10),
            Column::fit("Clients", &clients, 7),
            Column::fit("Health", health.iter().map(|h| health_label(*h)), 6),
        ];
        let band_cells: Vec<Vec<String>> = if compact {
            headers.push("Channels");
            let joined: Vec<String> = bands.iter().map(compact_channels).collect();
            columns.push(Column::fit("Channels", &joined, 24).shrink_to(8));
            joined.into_iter().map(|label| vec![label]).collect()
        } else {
            headers.extend(WifiBand::ALL.map(WifiBand::label));
            let labelled: Vec<Vec<String>> = bands
                .iter()
                .map(|bands| bands.iter().map(|labels| band_cell(labels)).collect())
                .collect();
            for (slot, band) in WifiBand::ALL.into_iter().enumerate() {
                let values = labelled.iter().map(|row| row[slot].as_str());
                columns.push(Column::fit(band.label(), values, 32).shrink_to(7));
            }
            labelled
        };

        let rows: Vec<Row> = names
            .into_iter()
            .zip(clients)
            .zip(health)
            .zip(band_cells)
            .enumerate()
            .map(|(index, (((name, clients), health), bands))| {
                let is_selected = index == selected;
                let mut cells = vec![
                    Cell::from(format!("{}{name}", marker(is_selected))),
                    Cell::from(clients),
                    Cell::from(health_label(health))
                        .style(Style::default().fg(health_color(health))),
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
            })
            .collect();

        let len = rows.len();
        let table = Table::new(rows, plan_columns(&columns, inner.width))
            .header(header_row(&headers))
            .row_highlight_style(theme::table_selected());

        let mut state = self.ap_table_state;
        frame.render_stateful_widget(table, inner, &mut state);
        self.hits.borrow_mut().push_table_rows(inner, &state, len);
    }

    pub(super) fn render_client_table(&self, frame: &mut Frame, area: Rect) {
        let block = panel_block(" Wireless Clients ", self.focused);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let clients = self.wireless_clients();
        let selected = self.client_table_state.selected().unwrap_or(0);
        let detailed = inner.width >= CLIENT_DETAIL_TABLE;
        let with_radio = inner.width >= CLIENT_RADIO_TABLE;

        let names: Vec<String> = clients
            .iter()
            .map(|client| display_client_name(client))
            .collect();
        let aps: Vec<String> = clients
            .iter()
            .map(|client| self.client_ap_name(client))
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

        let mut headers = vec!["Client"];
        let mut columns = vec![Column::fit("Client", &names, 32).pad(1).shrink_to(10)];
        if detailed {
            headers.push("AP");
            columns.push(Column::fit("AP", &aps, 24).shrink_to(8));
        }
        if with_radio {
            headers.push("Radio");
            columns.push(Column::fit("Radio", &radios, 14));
        }
        headers.extend(["Signal", "Health"]);
        columns.extend([Column::fixed(6), Column::fixed(6)]);
        if detailed {
            headers.push("TX/RX");
            columns.push(Column::fit("TX/RX", &rates, 16).at_least(14));
        }

        let rows: Vec<Row> = clients
            .iter()
            .zip(names)
            .zip(aps.into_iter().zip(radios).zip(rates))
            .enumerate()
            .map(|(index, ((client, name), ((ap, radio), rate)))| {
                let is_selected = index == selected;
                let wireless = client.wireless.as_ref();
                let signal = wireless.and_then(|wireless| wireless.signal_dbm);
                let health = wireless.and_then(|wireless| wireless.satisfaction);

                let mut cells = vec![Cell::from(format!("{}{name}", marker(is_selected)))];
                if detailed {
                    cells.push(Cell::from(ap));
                }
                if with_radio {
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
        let table = Table::new(rows, plan_columns(&columns, inner.width))
            .header(header_row(&headers))
            .row_highlight_style(theme::table_selected());

        let mut state = self.client_table_state;
        frame.render_stateful_widget(table, inner, &mut state);
        self.hits.borrow_mut().push_table_rows(inner, &state, len);
    }

    pub(super) fn render_neighbors(&self, frame: &mut Frame, area: Rect) {
        let block = panel_block(
            format!(" Neighbor APs [{}] ", self.selected_band.short_label()),
            self.focused,
        );
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let neighbors = self.visible_neighbors();
        let selected = self.neighbor_table_state.selected().unwrap_or(0);
        let cells: Vec<[String; 6]> = neighbors
            .iter()
            .enumerate()
            .map(|(index, neighbor)| {
                let signal = neighbor.signal.or(neighbor.rssi);
                [
                    format!(
                        "{}{}",
                        marker(index == selected),
                        display_neighbor_name(neighbor)
                    ),
                    neighbor.bssid.clone(),
                    neighbor
                        .channel
                        .map_or_else(|| "─".to_string(), |channel| channel.to_string()),
                    signal.map_or_else(|| "─".to_string(), |dbm| format!("{dbm} dBm")),
                    band_label(band_from_neighbor(neighbor).unwrap_or(WifiBand::TwoGhz)).to_owned(),
                    neighbor.security.clone().unwrap_or_else(|| "─".to_string()),
                ]
            })
            .collect();

        let headers = ["SSID", "BSSID", "Ch", "Signal", "Band", "Security"];
        let columns = [
            Column::fit("SSID", cells.iter().map(|c| &c[0]), 32).shrink_to(10),
            Column::fixed(17),
            Column::fit("Ch", cells.iter().map(|c| &c[2]), 4),
            Column::fit("Signal", cells.iter().map(|c| &c[3]), 8),
            Column::fit("Band", cells.iter().map(|c| &c[4]), 5),
            Column::fit("Security", cells.iter().map(|c| &c[5]), 24).shrink_to(8),
        ];

        let rows: Vec<Row> = cells
            .into_iter()
            .enumerate()
            .map(|(index, cells)| {
                Row::new(cells.map(Cell::from)).style(row_style(index == selected))
            })
            .collect();

        let len = rows.len();
        let table = Table::new(rows, plan_columns(&columns, inner.width))
            .header(header_row(&headers))
            .row_highlight_style(theme::table_selected());

        let mut state = self.neighbor_table_state;
        frame.render_stateful_widget(table, inner, &mut state);
        self.hits.borrow_mut().push_table_rows(inner, &state, len);
    }

    pub(super) fn render_roaming(&self, frame: &mut Frame, area: Rect) {
        let block = panel_block(" Roaming History ", self.focused);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.focused_client_mac().is_none() {
            frame.render_widget(
                Paragraph::new(
                    " Select a wireless client on the Clients tab to inspect roam history. ",
                )
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(theme::text_muted())),
                inner,
            );
            return;
        }

        let rows = self.parsed_roam_rows();
        let headers = ["Time", "Event", "From", "To", "Signal", "Band"];
        let columns = [
            Column::fit("Time", rows.iter().map(|r| &r.timestamp), 10),
            Column::fit("Event", rows.iter().map(|r| &r.event), 24).shrink_to(8),
            Column::fit("From", rows.iter().map(|r| &r.from_ap), 24).shrink_to(8),
            Column::fit("To", rows.iter().map(|r| &r.to_ap), 24).shrink_to(8),
            Column::fit("Signal", rows.iter().map(|r| &r.signal), 8),
            Column::fit("Band", rows.iter().map(|r| &r.band), 8),
        ];

        let selected = self.roam_table_state.selected().unwrap_or(0);
        let table_rows: Vec<Row> = rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                Row::new(vec![
                    Cell::from(row.timestamp.clone()),
                    Cell::from(row.event.clone()),
                    Cell::from(row.from_ap.clone()),
                    Cell::from(row.to_ap.clone()),
                    Cell::from(row.signal.clone()),
                    Cell::from(row.band.clone()),
                ])
                .style(row_style(index == selected))
            })
            .collect();

        let len = table_rows.len();
        let table = Table::new(table_rows, plan_columns(&columns, inner.width))
            .header(header_row(&headers))
            .row_highlight_style(theme::table_selected());

        let mut state = self.roam_table_state;
        frame.render_stateful_widget(table, inner, &mut state);
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

fn header_row(labels: &[&'static str]) -> Row<'static> {
    Row::new(
        labels
            .iter()
            .map(|label| Cell::from(*label).style(theme::table_header())),
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
    fn a_tap_inside_a_narrow_overlay_leaves_the_rows_beneath_alone() {
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
        screen.detail_open = true;
        let selected = screen.ap_table_state.selected();

        // Under 110 columns the AP detail draws as an overlay over the table.
        render_lines(80, 22, |frame, area| screen.render(frame, area));
        let action = screen.handle_mouse_input(left_click(40, 11));

        assert!(action.is_none(), "{action:?}");
        assert_eq!(screen.ap_table_state.selected(), selected);
        assert!(screen.detail_open);
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
