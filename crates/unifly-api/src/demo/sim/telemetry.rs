// ── Demo telemetry: WAN, client traffic, device stats, site health ──

use std::f64::consts::TAU;

use chrono::{DateTime, Utc};
use serde_json::json;

use super::{Burst, Simulation};
use crate::demo::catalog::{self, GATEWAY_FIRMWARE, WAN_RX_BASE, WAN_TX_BASE};
use crate::demo::fleet::devices::refresh_device_load;
use crate::demo::fleet::{GATEWAY, SWITCH_BASE, clients::satisfaction};
use crate::demo::rng::{from_u64, from_usize, to_i32, to_u64};
use crate::model::common::Bandwidth;
use crate::model::{ClientType, DeviceStats, DeviceType, HealthSummary};

/// Combined WAN rate that pins the gateway CPU near its ceiling.
const WAN_CPU_SCALE: f64 = 260_000_000.0;

impl Simulation {
    /// Per-client traffic waves, render-farm uploads, and signal drift.
    pub(super) fn update_clients(&mut self) {
        let t = from_u64(self.tick);
        let render_upload = self
            .burst
            .map_or(0.0, |burst| burst.tx * burst.envelope(self.tick) / 10.0);

        for demo in &mut self.fleet.clients {
            let client = &mut demo.client;
            if client.blocked {
                client.bandwidth = Some(Bandwidth::default());
                continue;
            }
            let wave = 0.38
                + 0.26 * (t / 23.0 + demo.phase).sin()
                + 0.16 * (t / 7.3 + 2.0 * demo.phase).sin();
            let activity = (wave + self.rng.between(-0.08, 0.08)).clamp(0.03, 1.1);
            let rx = demo.rate * activity * 0.7;
            let mut tx = demo.rate * activity * 0.3;
            if client.device_class.as_deref() == Some("Render Node") {
                tx += render_upload * self.rng.between(0.8, 1.2);
            }
            client.rx_bytes = Some(client.rx_bytes.unwrap_or(0).saturating_add(to_u64(rx)));
            client.tx_bytes = Some(client.tx_bytes.unwrap_or(0).saturating_add(to_u64(tx)));
            client.bandwidth = Some(Bandwidth {
                tx_bytes_per_sec: to_u64(tx),
                rx_bytes_per_sec: to_u64(rx),
            });

            if let Some(wireless) = client.wireless.as_mut() {
                let wave = 3.0 * (t / 17.0 + demo.phase).sin() + self.rng.between(-1.0, 1.0);
                let signal = (demo.signal_base + to_i32(wave)).clamp(-88, -35);
                wireless.signal_dbm = Some(signal);
                wireless.satisfaction = Some(satisfaction(signal));
            }
        }
    }

    /// Smooth WAN waves plus scheduled bursts (render uploads and asset
    /// syncs) that make the dashboard chart breathe.
    pub(super) fn update_wan(&mut self) {
        if self
            .burst
            .is_some_and(|burst| self.tick >= burst.start + burst.length)
        {
            self.burst = None;
        }
        if self.burst.is_none() && self.tick >= self.next_burst {
            let upload = self.rng.chance(0.55);
            let length = if upload {
                self.rng.range(14, 26)
            } else {
                self.rng.range(9, 18)
            };
            let burst = Burst {
                start: self.tick,
                length,
                tx: if upload {
                    self.rng.between(110e6, 210e6)
                } else {
                    self.rng.between(2e6, 8e6)
                },
                rx: if upload {
                    self.rng.between(4e6, 12e6)
                } else {
                    self.rng.between(70e6, 150e6)
                },
            };
            self.burst = Some(burst);
            self.next_burst = self.tick + length + self.rng.range(25, 60);
        }

        let t = from_u64(self.tick);
        let (burst_tx, burst_rx) = self.burst.map_or((0.0, 0.0), |burst| {
            let envelope = burst.envelope(self.tick);
            (burst.tx * envelope, burst.rx * envelope)
        });
        let rx = WAN_RX_BASE
            * (0.78
                + 0.18 * (t * TAU / 160.0).sin()
                + 0.12 * (t * TAU / 41.0 + 0.7).sin()
                + 0.06 * (t * TAU / 11.0).sin())
            + self.rng.between(-2.0e6, 2.0e6)
            + burst_rx;
        let tx = WAN_TX_BASE
            * (0.75 + 0.2 * (t * TAU / 120.0 + 2.1).sin() + 0.14 * (t * TAU / 29.0).sin())
            + self.rng.between(-0.9e6, 0.9e6)
            + burst_tx;
        self.wan = (to_u64(tx), to_u64(rx));
    }

    /// CPU, memory, load, uptime, client counts, uplink throughput, and
    /// radio utilization for every device.
    pub(super) fn update_devices(&mut self, now: DateTime<Utc>) {
        let t = from_u64(self.tick);
        let clients = &self.fleet.clients;
        let count = self.fleet.devices.len();

        // Children always sit after their parent, so a reverse sweep sees
        // every downstream total before it is needed.
        for index in (SWITCH_BASE..count).rev() {
            let (head, tail) = self.fleet.devices.split_at_mut(index + 1);
            let device = &mut head[index];
            refresh_device_load(device, clients);
            if !device.state.is_online() {
                continue;
            }
            let mut bandwidth = device.stats.uplink_bandwidth.unwrap_or_default();
            for child in tail
                .iter()
                .filter(|child| child.uplink_device_mac.as_ref() == Some(&device.mac))
            {
                let downstream = child.stats.uplink_bandwidth.unwrap_or_default();
                bandwidth.tx_bytes_per_sec += downstream.tx_bytes_per_sec;
                bandwidth.rx_bytes_per_sec += downstream.rx_bytes_per_sec;
            }
            device.stats.uplink_bandwidth = Some(bandwidth);

            let throughput =
                from_u64(bandwidth.tx_bytes_per_sec + bandwidth.rx_bytes_per_sec) / 50e6;
            let load = from_usize(usize::try_from(device.client_count.unwrap_or(0)).unwrap_or(0));
            let cpu = if device.device_type == DeviceType::AccessPoint {
                4.0 + load * 0.45 + throughput * 9.0
            } else {
                5.0 + throughput * 6.0
            } + self.rng.between(-1.5, 1.5);
            tick_stats(&mut device.stats, cpu.clamp(2.0, 85.0), 2.0, t, now);

            for radio in &mut device.radios {
                let (low, high) = if radio.frequency_ghz < 3.0 {
                    (22.0, 68.0)
                } else if radio.frequency_ghz < 5.5 {
                    (6.0, 42.0)
                } else {
                    (1.0, 16.0)
                };
                if let Some(util) = radio.channel_utilization_pct.as_mut() {
                    *util = (*util + self.rng.between(-1.6, 1.6)).clamp(low, high);
                }
            }
        }

        let (tx, rx) = self.wan;
        let gateway = &mut self.fleet.devices[GATEWAY];
        gateway.client_count = u32::try_from(clients.len()).ok();
        gateway.stats.uplink_bandwidth = Some(Bandwidth {
            tx_bytes_per_sec: tx,
            rx_bytes_per_sec: rx,
        });
        let cpu = 12.0 + 34.0 * from_u64(tx + rx) / WAN_CPU_SCALE + self.rng.between(-2.5, 2.5);
        tick_stats(&mut gateway.stats, cpu.clamp(5.0, 92.0), 4.0, t, now);
    }

    /// Site health in the Session `stat/health` shape the dashboard reads.
    pub(crate) fn health(&self) -> Vec<HealthSummary> {
        let t = from_u64(self.tick);
        let (wan_tx, wan_rx) = self.wan;
        let sum = |kind: ClientType| {
            self.fleet
                .clients
                .iter()
                .filter(|client| client.client.client_type == kind)
                .fold((0u32, 0u64, 0u64), |(count, tx, rx), client| {
                    let bandwidth = client.client.bandwidth.unwrap_or_default();
                    (
                        count + 1,
                        tx + bandwidth.tx_bytes_per_sec,
                        rx + bandwidth.rx_bytes_per_sec,
                    )
                })
        };
        let (wireless, wlan_tx, wlan_rx) = sum(ClientType::Wireless);
        let (wired, lan_tx, lan_rx) = sum(ClientType::Wired);
        let aps = self.fleet.aps();
        let aps_online = aps.iter().filter(|ap| ap.state.is_online()).count();
        let aps_offline = aps
            .iter()
            .filter(|ap| ap.state == crate::model::DeviceState::Offline)
            .count();
        let guests = self
            .fleet
            .clients
            .iter()
            .filter(|client| client.client.is_guest)
            .count();
        let gateway = &self.fleet.devices[GATEWAY];
        let latency = 10.5 + 2.0 * (t / 37.0).sin() + 1.2 * (t / 11.0).sin();

        vec![
            summary(
                "wan",
                "ok",
                Some(1),
                Some(wireless + wired),
                (wan_tx, wan_rx),
            )
            .with_wan(gateway.mac.as_str(), gateway.stats.uptime_secs),
            HealthSummary {
                latency: Some(latency),
                extra: json!({
                    "xput_up": 1_941.6,
                    "xput_down": 2_317.2,
                    "speedtest_status": "Success",
                    "speedtest_ping": 6,
                }),
                ..summary("www", "ok", None, None, (wan_tx, wan_rx))
            },
            HealthSummary {
                extra: json!({
                    "num_ap": aps_online,
                    "num_disconnected": aps_offline,
                    "num_user": usize::try_from(wireless).unwrap_or(0).saturating_sub(guests),
                    "num_guest": guests,
                }),
                ..summary(
                    "wlan",
                    if aps_offline > 0 { "warn" } else { "ok" },
                    u32::try_from(aps.len()).ok(),
                    Some(wireless),
                    (wlan_tx, wlan_rx),
                )
            },
            HealthSummary {
                extra: json!({ "num_sw": 3, "num_user": wired }),
                ..summary("lan", "ok", Some(3), Some(wired), (lan_tx, lan_rx))
            },
            HealthSummary {
                extra: json!({
                    "remote_user_enabled": true,
                    "remote_user_num_active": 2,
                    "site_to_site_enabled": false,
                }),
                ..summary("vpn", "ok", None, None, (0, 0))
            },
        ]
    }
}

/// Fold one tick into a device's stats: CPU, a slow memory drift, load
/// averages eased toward the CPU level, uptime, and heartbeat.
fn tick_stats(stats: &mut DeviceStats, cpu: f64, cores: f64, t: f64, now: DateTime<Utc>) {
    stats.cpu_utilization_pct = Some(cpu);
    let memory = stats.memory_utilization_pct.unwrap_or(50.0);
    stats.memory_utilization_pct = Some((memory + 0.04 * (t / 90.0).sin()).clamp(20.0, 90.0));
    let target = cpu / 100.0 * cores;
    for (slot, rate) in [
        (&mut stats.load_average_1m, 0.3),
        (&mut stats.load_average_5m, 0.07),
        (&mut stats.load_average_15m, 0.025),
    ] {
        let current = slot.unwrap_or(target);
        *slot = Some(current + (target - current) * rate);
    }
    stats.uptime_secs = Some(stats.uptime_secs.unwrap_or(0) + 1);
    stats.last_heartbeat = Some(now);
    stats.next_heartbeat = Some(now + chrono::Duration::seconds(10));
}

fn summary(
    subsystem: &str,
    status: &str,
    adopted: Option<u32>,
    stations: Option<u32>,
    (tx, rx): (u64, u64),
) -> HealthSummary {
    HealthSummary {
        subsystem: subsystem.into(),
        status: status.into(),
        num_adopted: adopted,
        num_sta: stations,
        tx_bytes_r: Some(tx),
        rx_bytes_r: Some(rx),
        latency: None,
        wan_ip: None,
        gateways: None,
        extra: json!({}),
    }
}

trait WanHealth {
    fn with_wan(self, gateway_mac: &str, uptime: Option<u64>) -> Self;
}

impl WanHealth for HealthSummary {
    fn with_wan(mut self, gateway_mac: &str, uptime: Option<u64>) -> Self {
        self.wan_ip = Some(catalog::WAN_IP.into());
        self.gateways = Some(vec![catalog::WAN_GATEWAY.into()]);
        self.extra = json!({
            "isp_name": catalog::ISP_NAME,
            "isp_organization": catalog::ISP_NAME,
            "nameservers": catalog::NAMESERVERS,
            "gateways": [catalog::WAN_GATEWAY],
            "gw_version": GATEWAY_FIRMWARE,
            "gw_name": "gw-harbor",
            "gw_mac": gateway_mac,
            "wan_ip": catalog::WAN_IP,
            "wan_ip6": catalog::WAN_IPV6,
            "uptime": uptime,
        });
        self
    }
}
