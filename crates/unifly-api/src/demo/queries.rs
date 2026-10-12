// ── Demo on-demand queries ──
//
// Synthetic answers for the Session and Integration queries the TUI makes
// outside the reactive store: historical stats, DPI, neighbors, channels,
// roam history, and per-client Wi-Fi experience. Shapes follow the wire
// formats the real endpoints return, and every name, MAC, and channel comes
// from the same fleet the store holds.

use chrono::{DateTime, Duration, TimeZone, Utc};
use serde_json::{Value, json};

use super::catalog::{DPI_APPS, DPI_CATEGORIES};
use super::fleet::GATEWAY;
use super::fleet::history::activity;
use super::rng::{DemoRng, from_u64, from_usize, hash_str, hash_unit, mix, to_u64};
use super::sim::Simulation;
use crate::core_error::CoreError;
use crate::model::{ClientType, Device, DpiApplication, DpiCategory, MacAddress};
use crate::session::models::{ChannelAvailability, RogueAp};

/// Total bytes behind the DPI breakdown (about a month of studio traffic).
const DPI_TOTAL_BYTES: f64 = 41.0e12;
/// Oldest history a report will answer for.
const MAX_HISTORY_SECS: i64 = 400 * 86_400;
/// Most rows a single report returns.
const MAX_ROWS: i64 = 2_000;
/// Gateway LAN bytes relative to WAN bytes (WAN traffic plus routing).
const LAN_ROUTING_FACTOR: f64 = 1.12;

fn bucket_secs(interval: &str) -> i64 {
    match interval {
        "5minutes" => 300,
        "daily" => 86_400,
        "monthly" => 30 * 86_400,
        _ => 3_600,
    }
}

/// Report bounds arrive in milliseconds from the TUI (the controller's
/// unit) and in seconds from the CLI's `--start`/`--end`; accept both.
fn epoch_secs(value: i64) -> i64 {
    if value.unsigned_abs() >= 100_000_000_000 {
        value / 1_000
    } else {
        value
    }
}

fn default_span_secs(interval: &str) -> i64 {
    match interval {
        "5minutes" => 12 * 3_600,
        "daily" => 30 * 86_400,
        "monthly" => 365 * 86_400,
        _ => 7 * 86_400,
    }
}

/// Mean studio activity across a bucket (sampled hourly for long buckets).
fn bucket_activity(start: DateTime<Utc>, bucket: i64, anchor: DateTime<Utc>) -> f64 {
    if bucket <= 3_600 {
        return activity(start + Duration::seconds(bucket / 2), anchor);
    }
    let samples: Vec<f64> = (0..24)
        .map(|hour| activity(start + Duration::hours(hour), anchor))
        .collect();
    samples.iter().sum::<f64>() / 24.0
}

/// One report bucket: when it starts, and a stable noise key derived from
/// its offset from launch, so the same seed draws the same series no
/// matter what time the fleet was launched.
struct Bucket {
    at: DateTime<Utc>,
    key: u64,
}

impl Simulation {
    /// Report buckets between `start` and `end`, aligned to the launch time
    /// rather than the wall clock.
    fn buckets(
        &self,
        interval: &str,
        start: Option<i64>,
        end: Option<i64>,
        now: DateTime<Utc>,
    ) -> Vec<Bucket> {
        // Clamp before doing any arithmetic: the bounds come straight from
        // callers and may be anything an i64 holds.
        let bucket = bucket_secs(interval);
        let now_secs = now.timestamp();
        let floor = now_secs - MAX_HISTORY_SECS;
        let end_secs = end.map_or(now_secs, epoch_secs).clamp(floor, now_secs);
        let start_secs = start
            .map_or(end_secs - default_span_secs(interval), epoch_secs)
            .clamp(floor, end_secs);
        let anchor_secs = self.fleet.anchor.timestamp();
        let mut first_offset = (start_secs - anchor_secs).div_euclid(bucket);
        let mut count = (end_secs - (anchor_secs + first_offset * bucket)) / bucket;
        if count > MAX_ROWS {
            // Keep the newest buckets when the range holds too many.
            first_offset += count - MAX_ROWS;
            count = MAX_ROWS;
        }
        let salt = mix(bucket.cast_unsigned());
        (0..count.max(0))
            .filter_map(|index| {
                let offset = first_offset + index;
                let at = Utc
                    .timestamp_opt(anchor_secs + offset * bucket, 0)
                    .single()?;
                Some(Bucket {
                    at,
                    key: salt ^ offset.cast_unsigned(),
                })
            })
            .collect()
    }

    /// Scale that lines a historical series up with the live fleet, which
    /// runs at launch-time activity, so the newest bucket agrees with the
    /// dashboard.
    fn level_ratio(&self, level: f64, floor: f64) -> f64 {
        let anchor = self.fleet.anchor;
        (floor + (1.0 - floor) * level) / (floor + (1.0 - floor) * activity(anchor, anchor))
    }

    /// `stat/report/<interval>.site` rows: client counts per bucket.
    pub(crate) fn site_stats(
        &self,
        interval: &str,
        start: Option<i64>,
        end: Option<i64>,
        now: DateTime<Utc>,
    ) -> Vec<Value> {
        let bucket = bucket_secs(interval);
        // The launch population, not the live one, so history does not
        // shift as clients come and go.
        let live = from_usize(self.initial_clients());
        self.buckets(interval, start, end, now)
            .into_iter()
            .map(|Bucket { at, key }| {
                let level = bucket_activity(at, bucket, self.fleet.anchor);
                let noise = 0.94 + 0.12 * hash_unit(self.fleet.seed ^ 0x51, key);
                let total = live * self.level_ratio(level, 0.25) * noise;
                let wireless = to_u64(total * 0.79);
                let wired = to_u64(total).saturating_sub(wireless);
                json!({
                    "o": "site",
                    "site": "default",
                    "time": at.timestamp_millis(),
                    "num_sta": wireless + wired,
                    "wlan-num_sta": wireless,
                    "lan-num_sta": wired,
                    "bytes": to_u64(total * 1.6e9),
                })
            })
            .collect()
    }

    /// `stat/report/<interval>.gw` rows: WAN bytes per bucket, with the
    /// occasional render-farm upload standing out on the transmit side.
    pub(crate) fn gateway_stats(
        &self,
        interval: &str,
        start: Option<i64>,
        end: Option<i64>,
        now: DateTime<Utc>,
    ) -> Vec<Value> {
        let bucket = bucket_secs(interval);
        let bucket_f = from_u64(u64::try_from(bucket).unwrap_or(3_600));
        let gateway = &self.fleet.devices[GATEWAY];
        let seed = self.fleet.seed;
        self.buckets(interval, start, end, now)
            .into_iter()
            .map(|Bucket { at, key }| {
                let level = bucket_activity(at, bucket, self.fleet.anchor);
                let ratio_rx = self.level_ratio(level, 0.15);
                let ratio_tx = self.level_ratio(level, 0.2);
                let rx_rate = 21.0e6 * ratio_rx * (0.85 + 0.3 * hash_unit(seed ^ 0xA1, key));
                let mut tx_rate = 6.5e6 * ratio_tx * (0.85 + 0.3 * hash_unit(seed ^ 0xA2, key));
                let burst = hash_unit(seed ^ 0xB0, key);
                // Render uploads: sharp in 5-minute buckets, smeared across
                // an hour, invisible in daily totals.
                let upload_peak = match bucket {
                    300 => 55.0e6,
                    3_600 => 22.0e6,
                    _ => 0.0,
                };
                if level > 0.3 && burst < 0.09 {
                    tx_rate += upload_peak * (0.4 + 0.6 * hash_unit(seed ^ 0xB1, key));
                }
                let (wan_tx, wan_rx) = (tx_rate * bucket_f, rx_rate * bucket_f);
                // The LAN side carries the WAN traffic plus inter-VLAN
                // routing, so it runs a little hotter in both directions.
                json!({
                    "o": "gw",
                    "gw": gateway.mac.as_str(),
                    "time": at.timestamp_millis(),
                    "bytes": to_u64(wan_tx + wan_rx),
                    "wan-tx_bytes": to_u64(wan_tx),
                    "wan-rx_bytes": to_u64(wan_rx),
                    "lan-tx_bytes": to_u64(wan_rx * LAN_ROUTING_FACTOR),
                    "lan-rx_bytes": to_u64(wan_tx * LAN_ROUTING_FACTOR),
                    "lan-num_sta": to_u64(level * 40.0),
                })
            })
            .collect()
    }

    pub(crate) fn dpi_applications(&self) -> Vec<DpiApplication> {
        DPI_APPS
            .iter()
            .map(|(id, name, category, share)| {
                let jitter = 0.9 + 0.2 * hash_unit(self.fleet.seed ^ 0xD1, u64::from(*id));
                let total = DPI_TOTAL_BYTES * share * jitter;
                let upload_heavy = matches!(*id, 101 | 102 | 107);
                let tx_share = if upload_heavy { 0.62 } else { 0.12 };
                DpiApplication {
                    id: *id,
                    name: (*name).into(),
                    category_id: *category,
                    tx_bytes: to_u64(total * tx_share),
                    rx_bytes: to_u64(total * (1.0 - tx_share)),
                }
            })
            .collect()
    }

    pub(crate) fn dpi_categories(&self) -> Vec<DpiCategory> {
        let apps = self.dpi_applications();
        DPI_CATEGORIES
            .iter()
            .map(|(id, name)| {
                let members: Vec<DpiApplication> = apps
                    .iter()
                    .filter(|app| app.category_id == *id)
                    .cloned()
                    .collect();
                DpiCategory {
                    id: *id,
                    name: (*name).into(),
                    tx_bytes: members.iter().map(|app| app.tx_bytes).sum(),
                    rx_bytes: members.iter().map(|app| app.rx_bytes).sum(),
                    apps: members,
                }
            })
            .collect()
    }

    /// Session `stat/stadpi`-style DPI rows grouped by app or category.
    pub(crate) fn dpi_stats(&self, group_by: &str) -> Vec<Value> {
        if group_by == "by_cat" {
            let rows: Vec<Value> = self
                .dpi_categories()
                .iter()
                .map(|category| {
                    json!({
                        "cat": category.id,
                        "tx_bytes": category.tx_bytes,
                        "rx_bytes": category.rx_bytes,
                        "apps": category.apps.iter().map(|app| app.id).collect::<Vec<_>>(),
                    })
                })
                .collect();
            return vec![json!({ "by_cat": rows })];
        }
        let rows: Vec<Value> = self
            .dpi_applications()
            .iter()
            .map(|app| {
                json!({
                    "app": app.id,
                    "cat": app.category_id,
                    "tx_bytes": app.tx_bytes,
                    "rx_bytes": app.rx_bytes,
                })
            })
            .collect();
        vec![json!({ "by_app": rows })]
    }

    /// Neighbors heard within the last `within_secs`, ages drifting with
    /// the simulation clock.
    pub(crate) fn rogue_aps(&self, within_secs: Option<i64>) -> Vec<RogueAp> {
        let drift = i64::try_from(self.tick % 30).unwrap_or(0);
        self.fleet
            .neighbors
            .iter()
            .map(|neighbor| {
                let mut neighbor = neighbor.clone();
                neighbor.age = neighbor.age.map(|age| age + drift);
                neighbor
            })
            .filter(|neighbor| {
                within_secs.is_none_or(|limit| neighbor.age.is_none_or(|age| age <= limit))
            })
            .collect()
    }

    /// `stat/current-channel` for a US-regulated site.
    pub(crate) fn channels() -> Vec<ChannelAvailability> {
        let six_ghz: Vec<i32> = (0..59).map(|step| 1 + 4 * step).collect();
        let mut extra = serde_json::Map::new();
        extra.insert("channels_na_80".into(), json!([36, 52, 100, 116, 132, 149]));
        extra.insert(
            "channels_6e_160".into(),
            json!([37, 69, 101, 133, 165, 197]),
        );
        vec![ChannelAvailability {
            code: Some("840".into()),
            key: Some("US".into()),
            name: Some("United States".into()),
            channels_ng: Some((1..=11).collect()),
            channels_na: Some(vec![
                36, 40, 44, 48, 52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136,
                140, 144, 149, 153, 157, 161, 165,
            ]),
            channels_na_dfs: Some(vec![
                52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 144,
            ]),
            channels_6e: Some(six_ghz),
            extra,
        }]
    }

    /// `system-log/client-connection` rows for one client: a stable day of
    /// connects, roams, and disconnects, plus any roams seen live.
    pub(crate) fn client_roams(
        &self,
        mac: &str,
        limit: Option<u32>,
    ) -> Result<Vec<Value>, CoreError> {
        let mac = MacAddress::new(mac);
        let client = self
            .fleet
            .clients
            .iter()
            .chain(self.fleet.away.iter())
            .map(|demo| &demo.client)
            .find(|client| client.mac == mac)
            .ok_or_else(|| CoreError::ClientNotFound {
                identifier: mac.to_string(),
            })?;
        if client.client_type != ClientType::Wireless {
            return Ok(Vec::new());
        }

        // History is the past, so it draws on the whole AP inventory, not
        // whichever APs happen to be online right now.
        let inventory: Vec<String> = self
            .fleet
            .aps()
            .iter()
            .filter_map(|ap| ap.name.clone())
            .collect();
        let ssid = client
            .wireless
            .as_ref()
            .and_then(|wireless| wireless.ssid.clone())
            .unwrap_or_default();
        let mut rng = DemoRng::new(self.fleet.seed ^ hash_str(mac.as_str()));
        let mut at = self.fleet.anchor - Duration::hours(22);
        let Some(mut current) = rng.pick(&inventory).cloned() else {
            return Ok(Vec::new());
        };
        let mut rows = vec![roam_row(
            "CLIENT_CONNECTED_WIRELESS",
            at,
            ("", &current),
            &mut rng,
            &ssid,
        )];
        for _ in 0..rng.range(6, 14) {
            at += Duration::minutes(i64::try_from(rng.range(20, 110)).unwrap_or(45));
            if at >= self.fleet.anchor {
                break;
            }
            let next = rng.pick(&inventory).cloned().unwrap_or_default();
            if rng.chance(0.15) {
                rows.push(roam_row(
                    "CLIENT_DISCONNECTED_WIRELESS",
                    at,
                    (&current, ""),
                    &mut rng,
                    &ssid,
                ));
                at += Duration::minutes(i64::try_from(rng.range(5, 40)).unwrap_or(10));
                rows.push(roam_row(
                    "CLIENT_CONNECTED_WIRELESS",
                    at,
                    ("", &next),
                    &mut rng,
                    &ssid,
                ));
            } else if next != current {
                rows.push(roam_row(
                    "CLIENT_ROAMED",
                    at,
                    (&current, &next),
                    &mut rng,
                    &ssid,
                ));
            }
            current = next;
        }
        rows.extend(
            self.live_roams
                .iter()
                .filter(|(roamer, _)| roamer == &mac)
                .map(|(_, row)| row.clone()),
        );
        let limit = usize::try_from(limit.unwrap_or(100)).unwrap_or(100);
        let skip = rows.len().saturating_sub(limit);
        Ok(rows.into_iter().skip(skip).collect())
    }

    /// `wifiman/<ip>` live experience for a wireless client.
    pub(crate) fn client_wifi_experience(&self, ip: &str) -> Result<Value, CoreError> {
        let client = self
            .fleet
            .clients
            .iter()
            .map(|demo| &demo.client)
            .find(|client| client.ip.is_some_and(|addr| addr.to_string() == ip))
            .ok_or_else(|| CoreError::ClientNotFound {
                identifier: ip.to_owned(),
            })?;
        let wireless = client
            .wireless
            .as_ref()
            .ok_or_else(|| CoreError::Rejected {
                message: format!("{ip} is a wired client"),
            })?;
        let ap = client
            .uplink_device_mac
            .as_ref()
            .and_then(|mac| self.device_index_by_mac(mac))
            .and_then(|index| self.fleet.devices.get(index));
        let frequency = wireless.frequency_ghz.unwrap_or(5.0);
        let radio = ap.and_then(|ap| {
            ap.radios
                .iter()
                .find(|radio| (radio.frequency_ghz - frequency).abs() < 0.1)
        });
        let band = if frequency > 5.5 {
            "6g"
        } else if frequency > 4.5 {
            "5g"
        } else {
            "2.4g"
        };
        let protocol = radio
            .and_then(|radio| radio.wlan_standard.as_deref())
            .map_or("ax", |standard| standard.trim_start_matches("802.11"));
        let signal = wireless.signal_dbm.unwrap_or(-60);
        let mut rng = DemoRng::new(self.fleet.seed ^ hash_str(client.mac.as_str()));
        let neighbors = self.nearest_neighbors(ap, frequency, signal, &mut rng);
        let uplinks = self.uplink_chain(ap, wireless.satisfaction);

        Ok(json!({
            "ip": ip,
            "mac": client.mac.as_str(),
            "hostname": client.hostname,
            "ssid": wireless.ssid,
            "signal": signal,
            "noise": wireless.noise_dbm,
            "channel": wireless.channel,
            "channel_width": radio.and_then(|radio| radio.channel_width_mhz),
            "wlan_band": band,
            "radio_protocol": protocol,
            "wifi_experience": wireless.satisfaction,
            "link_upload_rate_kbps": wireless.tx_rate_kbps,
            "link_download_rate_kbps": wireless.rx_rate_kbps,
            "uplink_devices": uplinks,
            "nearest_neighbors": neighbors,
        }))
    }

    /// Our other APs as the client hears them, strongest first.
    fn nearest_neighbors(
        &self,
        ap: Option<&Device>,
        frequency: f32,
        signal: i32,
        rng: &mut DemoRng,
    ) -> Vec<Value> {
        let mut neighbors: Vec<Value> = self
            .fleet
            .aps()
            .iter()
            .filter(|other| other.state.is_online())
            .filter(|other| Some(&other.mac) != ap.map(|ap| &ap.mac))
            .map(|other| {
                let channel = other
                    .radios
                    .iter()
                    .find(|radio| (radio.frequency_ghz - frequency).abs() < 0.1)
                    .and_then(|radio| radio.channel);
                let loss = i32::try_from(rng.range(7, 26)).unwrap_or(12);
                json!({
                    "bssid": other.mac.as_str(),
                    "display_name": other.name,
                    "channel": channel,
                    "signal": signal - loss,
                })
            })
            .collect();
        neighbors.sort_by_key(|neighbor| {
            std::cmp::Reverse(
                neighbor
                    .get("signal")
                    .and_then(Value::as_i64)
                    .unwrap_or(-100),
            )
        });
        neighbors.truncate(4);
        neighbors
    }

    /// The client's AP and whatever that AP hangs off.
    fn uplink_chain(&self, ap: Option<&Device>, satisfaction: Option<u8>) -> Vec<Value> {
        let Some(ap) = ap else {
            return Vec::new();
        };
        let mut uplinks = vec![json!({
            "display_name": ap.name,
            "mac": ap.mac.as_str(),
            "experience": satisfaction.map_or(95, |value| value.saturating_add(2).min(99)),
        })];
        if let Some(parent) = ap
            .uplink_device_mac
            .as_ref()
            .and_then(|mac| self.device_index_by_mac(mac))
            .and_then(|index| self.fleet.devices.get(index))
        {
            uplinks.push(json!({
                "display_name": parent.name,
                "mac": parent.mac.as_str(),
                "experience": 99,
            }));
        }
        uplinks
    }
}

fn roam_row(
    key: &str,
    at: DateTime<Utc>,
    (from, to): (&str, &str),
    rng: &mut DemoRng,
    ssid: &str,
) -> Value {
    let band = rng.choose(&["5g", "5g", "6g", "2.4g"]);
    let channel = match band {
        "6g" => rng.choose(&[37, 69, 101, 133]),
        "2.4g" => rng.choose(&[1, 6, 11]),
        _ => rng.choose(&[36, 52, 100, 149]),
    };
    let signal = -i64::try_from(rng.range(44, 74)).unwrap_or(60);
    let label = match key {
        "CLIENT_ROAMED" => "Roamed",
        "CLIENT_DISCONNECTED_WIRELESS" => "Disconnected",
        _ => "Connected",
    };
    json!({
        "key": key,
        "event_type": label,
        "timestamp": at.timestamp_millis(),
        "parameters": {
            "DEVICE_FROM": { "name": from },
            "DEVICE_TO": { "name": to },
            "SIGNAL_STRENGTH": { "name": signal.to_string() },
            "RADIO_BAND": { "name": band },
            "CHANNEL": { "name": channel.to_string() },
            "WLAN": { "name": ssid },
        }
    })
}
