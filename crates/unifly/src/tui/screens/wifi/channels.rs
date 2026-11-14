//! Which channels the site's own radios and their neighbors sit on.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use unifly_api::Device;

use super::WifiScreen;
use super::state::{band_from_channel, band_from_frequency, band_from_neighbor};
use crate::tui::action::WifiBand;

/// One channel of the channel map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ChannelOccupancy {
    pub channel: i32,
    /// The site's own radios on this channel.
    pub your_count: usize,
    pub neighbor_count: usize,
    /// The strongest neighbor on this channel.
    pub signal: Option<i32>,
    pub conflict: bool,
    /// Whether radar detection applies here.
    pub dfs: bool,
}

impl WifiScreen {
    /// The APs the channel map counts: the focused one, or every AP.
    fn map_aps(&self) -> impl Iterator<Item = &Arc<Device>> {
        let focus = self.focused_ap.as_ref().map(|focused| &focused.id);
        self.all_aps()
            .filter(move |ap| focus.is_none_or(|id| &ap.id == id))
    }

    /// The channel of every radio `ap` runs on `band`. An AP that reports
    /// no radios falls back to the channel its clients are on.
    fn ap_band_channels(&self, ap: &Device, band: WifiBand) -> Vec<i32> {
        let radios: Vec<i32> = ap
            .radios
            .iter()
            .filter(|radio| band_from_frequency(radio.frequency_ghz) == band)
            .filter_map(|radio| i32::try_from(radio.channel?).ok())
            .collect();
        if !ap.radios.is_empty() {
            return radios;
        }
        self.client_channel_profile(ap)
            .filter(|(_, client_band)| *client_band == band)
            .and_then(|(channel, _)| i32::try_from(channel).ok())
            .into_iter()
            .collect()
    }

    /// One channel to stand for `ap`, for sorting and for an AP whose
    /// radio table is missing: its lowest radio channel, else the channel
    /// most of its clients report.
    pub(super) fn ap_channel_profile(&self, ap: &Device) -> Option<(u32, WifiBand)> {
        ap.radios
            .iter()
            .filter_map(|radio| Some((radio.channel?, band_from_frequency(radio.frequency_ghz))))
            .min_by_key(|(channel, _)| *channel)
            .or_else(|| self.client_channel_profile(ap))
    }

    /// The channel and band most of the clients on `ap` report, lowest
    /// channel on a tie.
    fn client_channel_profile(&self, ap: &Device) -> Option<(u32, WifiBand)> {
        let mut counts: BTreeMap<u32, (usize, WifiBand)> = BTreeMap::new();
        for wireless in self
            .ap_clients(ap)
            .iter()
            .filter_map(|client| client.wireless.as_ref())
        {
            let Some(channel) = wireless.channel else {
                continue;
            };
            let band = wireless
                .frequency_ghz
                .map_or_else(|| band_from_channel(channel), band_from_frequency);
            counts.entry(channel).or_insert((0, band)).0 += 1;
        }
        counts
            .into_iter()
            .max_by_key(|(channel, (count, _))| (*count, std::cmp::Reverse(*channel)))
            .map(|(channel, (_, band))| (channel, band))
    }

    /// The regulatory channel list for `band`, empty until it has loaded.
    fn regulatory_channels(&self, band: WifiBand) -> Vec<i32> {
        let Some(entry) = self.channels.first() else {
            return Vec::new();
        };
        match band {
            WifiBand::TwoGhz => entry.channels_ng.clone(),
            WifiBand::FiveGhz => entry.channels_na.clone(),
            WifiBand::SixGhz => entry.channels_6e.clone(),
        }
        .unwrap_or_default()
    }

    /// The channel map for the selected band, in channel order.
    ///
    /// 2.4 GHz shows every channel. The wider bands have dozens, so they
    /// show only the channels in use, and the full list when none is.
    pub(super) fn channel_occupancy(&self) -> Vec<ChannelOccupancy> {
        let band = self.selected_band;

        let mut cells: BTreeMap<i32, ChannelOccupancy> = BTreeMap::new();
        for ap in self.map_aps() {
            for channel in self.ap_band_channels(ap, band) {
                cells.entry(channel).or_default().your_count += 1;
            }
        }
        for neighbor in self
            .neighbors
            .iter()
            .filter(|neighbor| self.neighbor_seen_by_focus(neighbor))
            .filter(|neighbor| band_from_neighbor(neighbor) == Some(band))
        {
            let Some(channel) = neighbor.channel else {
                continue;
            };
            let cell = cells.entry(channel).or_default();
            cell.neighbor_count += 1;
            cell.signal = cell.signal.max(neighbor.signal.or(neighbor.rssi));
        }

        let mut listed = self.regulatory_channels(band);
        if band == WifiBand::TwoGhz && listed.is_empty() {
            listed = vec![1, 6, 11];
        }
        if band == WifiBand::TwoGhz || cells.is_empty() {
            for channel in listed {
                cells.entry(channel).or_default();
            }
        }

        let dfs: BTreeSet<i32> = if band == WifiBand::FiveGhz {
            self.channels
                .first()
                .and_then(|entry| entry.channels_na_dfs.clone())
                .unwrap_or_default()
                .into_iter()
                .collect()
        } else {
            BTreeSet::new()
        };

        cells
            .into_iter()
            .map(|(channel, cell)| ChannelOccupancy {
                channel,
                conflict: cell.your_count > 0 && cell.neighbor_count > 0,
                dfs: dfs.contains(&channel),
                ..cell
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use unifly_api::Device;
    use unifly_api::session_models::{ChannelAvailability, RogueAp};

    use super::ChannelOccupancy;
    use crate::tui::action::{Action, WifiBand};
    use crate::tui::screens::wifi::WifiScreen;

    fn ap(id: &str, radios: &[(f32, u32)]) -> Arc<Device> {
        let radios: Vec<_> = radios
            .iter()
            .map(|(frequency_ghz, channel)| {
                json!({
                    "frequency_ghz": frequency_ghz, "channel": channel,
                    "channel_width_mhz": 20, "wlan_standard": null,
                    "tx_retries_pct": null, "channel_utilization_pct": null
                })
            })
            .collect();
        Arc::new(
            serde_json::from_value(json!({
                "id": id, "mac": format!("aa:bb:cc:dd:ee:{id}"), "ip": null,
                "wan_ipv6": null, "name": format!("ap-{id}"), "model": "U7 Pro",
                "device_type": "AccessPoint", "state": "Online",
                "firmware_version": null, "firmware_updatable": false,
                "adopted_at": null, "provisioned_at": null, "last_seen": null,
                "serial": null, "supported": true, "ports": [], "radios": radios,
                "uplink_device_id": null, "uplink_device_mac": null,
                "has_switching": false, "has_access_point": true, "stats": {},
                "client_count": null, "origin": null
            }))
            .expect("access point fixture should deserialize"),
        )
    }

    fn neighbor(bssid: &str, radio: &str, channel: i32, signal: i32) -> RogueAp {
        serde_json::from_value(json!({
            "bssid": bssid, "essid": "next-door", "channel": channel,
            "signal": signal, "radio": radio, "ap_mac": "aa:bb:cc:dd:ee:01"
        }))
        .expect("neighbor fixture should deserialize")
    }

    fn channel_list() -> ChannelAvailability {
        serde_json::from_value(json!({
            "channels_ng": [1, 6, 11],
            "channels_na": [36, 40, 52, 100],
            "channels_na_dfs": [52, 100],
            "channels_6e": [1, 5, 9]
        }))
        .expect("channel fixture should deserialize")
    }

    fn screen(band: WifiBand) -> WifiScreen {
        let mut screen = WifiScreen::new();
        screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![
            ap("01", &[(2.4, 6), (5.0, 36), (6.0, 5)]),
            ap("02", &[(2.4, 11), (5.0, 52)]),
            ap("03", &[(2.4, 6), (5.0, 36)]),
        ])));
        screen.apply_action(&Action::WifiChannelsUpdated(Arc::new(vec![channel_list()])));
        screen.apply_action(&Action::WifiBandSelect(band));
        screen
    }

    fn cell(cells: &[ChannelOccupancy], channel: i32) -> ChannelOccupancy {
        cells
            .iter()
            .find(|cell| cell.channel == channel)
            .cloned()
            .unwrap_or_else(|| panic!("no channel {channel} in {cells:?}"))
    }

    #[test]
    fn every_radio_counts_on_its_own_band() {
        let two = screen(WifiBand::TwoGhz).channel_occupancy();
        assert_eq!(cell(&two, 6).your_count, 2);
        assert_eq!(cell(&two, 11).your_count, 1);
        assert_eq!(
            cell(&two, 1).your_count,
            0,
            "an idle channel is still listed"
        );

        let five = screen(WifiBand::FiveGhz).channel_occupancy();
        assert_eq!(cell(&five, 36).your_count, 2);
        assert_eq!(cell(&five, 52).your_count, 1);
        assert_eq!(
            five.iter().map(|cell| cell.channel).collect::<Vec<_>>(),
            [36, 52],
            "5 GHz lists the channels in use"
        );

        let six = screen(WifiBand::SixGhz).channel_occupancy();
        assert_eq!(cell(&six, 5).your_count, 1);
        assert_eq!(six.len(), 1);
    }

    #[test]
    fn a_neighbor_on_one_of_your_channels_is_a_conflict() {
        let mut screen = screen(WifiBand::FiveGhz);
        screen.apply_action(&Action::WifiNeighborsUpdated(Arc::new(vec![
            neighbor("11:11:11:11:11:11", "na", 36, -70),
            neighbor("22:22:22:22:22:22", "na", 36, -52),
            neighbor("33:33:33:33:33:33", "na", 100, -80),
            neighbor("44:44:44:44:44:44", "ng", 6, -40),
        ])));

        let cells = screen.channel_occupancy();
        let busy = cell(&cells, 36);
        assert_eq!((busy.your_count, busy.neighbor_count), (2, 2));
        assert_eq!(busy.signal, Some(-52), "the strongest neighbor");
        assert!(busy.conflict);

        let theirs = cell(&cells, 100);
        assert_eq!((theirs.your_count, theirs.neighbor_count), (0, 1));
        assert!(!theirs.conflict);
        assert!(!cell(&cells, 52).conflict);
    }

    #[test]
    fn dfs_channels_are_marked_on_five_ghz_only() {
        let five = screen(WifiBand::FiveGhz).channel_occupancy();
        assert!(cell(&five, 52).dfs);
        assert!(!cell(&five, 36).dfs);

        let two = screen(WifiBand::TwoGhz).channel_occupancy();
        assert!(two.iter().all(|cell| !cell.dfs));
    }

    #[test]
    fn a_band_nothing_uses_lists_every_allowed_channel() {
        let mut screen = WifiScreen::new();
        screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![ap(
            "01",
            &[(2.4, 6)],
        )])));
        screen.apply_action(&Action::WifiChannelsUpdated(Arc::new(vec![channel_list()])));
        screen.apply_action(&Action::WifiBandSelect(WifiBand::FiveGhz));

        let channels: Vec<_> = screen
            .channel_occupancy()
            .iter()
            .map(|cell| cell.channel)
            .collect();
        assert_eq!(channels, [36, 40, 52, 100]);
    }

    #[test]
    fn the_map_narrows_to_the_focused_ap() {
        let mut screen = screen(WifiBand::FiveGhz);
        let focus = screen.devices[1].id.clone();
        screen.apply_action(&Action::WifiFocusAp(Some(focus)));

        let cells = screen.channel_occupancy();
        assert_eq!(
            cells.iter().map(|cell| cell.channel).collect::<Vec<_>>(),
            [52]
        );
    }
}
