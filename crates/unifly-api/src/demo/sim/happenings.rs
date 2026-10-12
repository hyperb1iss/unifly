// ── Demo happenings: roams, arrivals, departures, alerts, upgrades ──

use std::net::IpAddr;

use chrono::{DateTime, Utc};
use serde_json::json;

use super::{Delta, Pending, Simulation, client_label};
use crate::demo::catalog::{self, AP_FIRMWARE};
use crate::demo::fleet::clients::attach_to_ap;
use crate::demo::fleet::devices::pick_ap;
use crate::demo::fleet::history::{event, security_event};
use crate::demo::fleet::{DemoClient, SWITCH_BASE, lan_ip, uuid_from};
use crate::model::common::DataSource;
use crate::model::{
    Client, ClientType, DeviceState, EntityId, Event, EventCategory, EventSeverity, MacAddress,
    PoeInfo, PortState,
};

const INFO_CLIENT: (EventCategory, EventSeverity) = (EventCategory::Client, EventSeverity::Info);
const INFO_DEVICE: (EventCategory, EventSeverity) = (EventCategory::Device, EventSeverity::Info);
/// Roam log entries kept for history queries.
const ROAM_LOG_CAP: usize = 400;

impl Simulation {
    /// Complete upgrades, restarts, and other transitions that are due.
    pub(super) fn finish_pending(&mut self, now: DateTime<Utc>, delta: &mut Delta) {
        let tick = self.tick;
        let due: Vec<Pending> = self
            .pending
            .iter()
            .filter(|(at, _)| *at <= tick)
            .map(|(_, pending)| *pending)
            .collect();
        self.pending.retain(|(at, _)| *at > tick);

        for pending in due {
            match pending {
                Pending::Upgrade { device } => {
                    let name = self.device_name(device);
                    if let Some(device) = self.fleet.devices.get_mut(device) {
                        device.state = DeviceState::Online;
                        device.firmware_version = Some(AP_FIRMWARE.into());
                        device.firmware_updatable = false;
                        device.stats.uptime_secs = Some(0);
                        device.last_seen = Some(now);
                    }
                    let message = format!("{name} upgraded to {AP_FIRMWARE} and is back online");
                    let event = self.event(now, INFO_DEVICE, "EVT_AP_Upgraded", message);
                    delta.events.push(event);
                }
                Pending::Restart { device } => {
                    let name = self.device_name(device);
                    if let Some(device) = self.fleet.devices.get_mut(device) {
                        device.state = DeviceState::Online;
                        device.stats.uptime_secs = Some(0);
                    }
                    let message = format!("{name} finished restarting");
                    delta
                        .events
                        .push(self.event(now, INFO_DEVICE, "EVT_SW_Restarted", message));
                }
                Pending::CameraRevive => self.revive_camera(now, delta),
            }
        }
    }

    /// Fire whichever periodic happenings are due this tick.
    pub(super) fn schedule_happenings(&mut self, now: DateTime<Utc>, delta: &mut Delta) {
        let tick = self.tick;

        let returning: Vec<MacAddress> = self
            .returns
            .iter()
            .filter(|(at, _)| *at <= tick)
            .map(|(_, mac)| mac.clone())
            .collect();
        self.returns.retain(|(at, _)| *at > tick);
        for mac in returning {
            if let Some(index) = self
                .fleet
                .away
                .iter()
                .position(|client| client.client.mac == mac)
            {
                self.reconnect(index, now, delta);
            }
        }

        if tick >= self.next_roam {
            self.next_roam = tick + self.rng.range(7, 16);
            self.roam(now, delta);
        }
        if tick >= self.next_churn {
            self.next_churn = tick + self.rng.range(8, 18);
            self.churn(now, delta);
        }
        if tick >= self.next_security {
            self.next_security = tick + self.rng.range(20, 45);
            let event = security_event(&self.fleet, &mut self.rng, now);
            delta.events.push(event);
        }
        if tick >= self.next_ambient {
            self.next_ambient = tick + self.rng.range(30, 60);
            let event = self.ambient_event(now);
            delta.events.push(event);
        }
    }

    pub(crate) fn event(
        &mut self,
        at: DateTime<Utc>,
        kind: (EventCategory, EventSeverity),
        key: &str,
        message: String,
    ) -> Event {
        event(&mut self.rng, &self.fleet, at, kind, key, message)
    }

    /// Move a mobile wireless client to a different AP.
    fn roam(&mut self, now: DateTime<Utc>, delta: &mut Delta) {
        let candidates: Vec<usize> = self
            .fleet
            .clients
            .iter()
            .enumerate()
            .filter(|(_, client)| {
                client.mobile && client.client.client_type == ClientType::Wireless
            })
            .map(|(index, _)| index)
            .collect();
        let Some(&index) = self.rng.pick(&candidates) else {
            return;
        };
        let Some(target) = pick_ap(&self.fleet.devices, &mut self.rng) else {
            return;
        };
        let target_mac = self.fleet.devices[target].mac.clone();
        let client = &self.fleet.clients[index].client;
        if client.uplink_device_mac.as_ref() == Some(&target_mac) {
            return;
        }
        let from = client
            .uplink_device_mac
            .as_ref()
            .and_then(|mac| self.device_index_by_mac(mac))
            .map(|from| self.device_name(from))
            .unwrap_or_default();
        let band = client
            .wireless
            .as_ref()
            .and_then(|wireless| wireless.frequency_ghz)
            .unwrap_or(5.0);

        let ap = self.fleet.devices[target].clone();
        let demo = &mut self.fleet.clients[index];
        demo.signal_base = attach_to_ap(&mut self.rng, &mut demo.client, &ap, band, true);
        let client = demo.client.clone();
        let to = ap.name.clone().unwrap_or_default();
        let label = client_label(&client);

        self.log_roam(&client, &from, &to, now);
        let message = format!("{label} roamed from {from} to {to}");
        let mut event = self.event(now, INFO_CLIENT, "EVT_WU_Roam", message);
        event.client_mac = Some(client.mac.clone());
        event.device_mac = Some(ap.mac.clone());
        delta.events.push(event);
    }

    fn log_roam(&mut self, client: &Client, from: &str, to: &str, now: DateTime<Utc>) {
        let wireless = client.wireless.as_ref();
        let band = wireless
            .and_then(|wireless| wireless.frequency_ghz)
            .map_or("5g", |freq| {
                if freq > 5.5 {
                    "6g"
                } else if freq > 4.5 {
                    "5g"
                } else {
                    "2.4g"
                }
            });
        let entry = json!({
            "key": "CLIENT_ROAMED",
            "event_type": "Roamed",
            "timestamp": now.timestamp_millis(),
            "parameters": {
                "DEVICE_FROM": { "name": from },
                "DEVICE_TO": { "name": to },
                "SIGNAL_STRENGTH": { "name": wireless.and_then(|w| w.signal_dbm).unwrap_or(-60).to_string() },
                "RADIO_BAND": { "name": band },
                "CHANNEL": { "name": wireless.and_then(|w| w.channel).unwrap_or(36).to_string() },
                "WLAN": { "name": wireless.and_then(|w| w.ssid.clone()).unwrap_or_default() },
            }
        });
        self.live_roams.push((client.mac.clone(), entry));
        if self.live_roams.len() > ROAM_LOG_CAP {
            self.live_roams.remove(0);
        }
    }

    /// Someone leaves or someone arrives, steering the population back
    /// toward its launch size so the fleet neither drains nor floods.
    fn churn(&mut self, now: DateTime<Utc>, delta: &mut Delta) {
        let connected = self.fleet.clients.len();
        let target = self.initial_clients();
        let arrive = if self.fleet.away.is_empty() {
            false
        } else if connected + 6 < target {
            true
        } else if connected > target + 4 {
            false
        } else {
            self.rng.chance(0.5)
        };

        if arrive {
            let index = self.rng.index(self.fleet.away.len());
            self.reconnect(index, now, delta);
        } else {
            self.depart(now, delta);
        }
    }

    fn depart(&mut self, now: DateTime<Utc>, delta: &mut Delta) {
        let candidates: Vec<usize> = self
            .fleet
            .clients
            .iter()
            .enumerate()
            .filter(|(_, client)| client.mobile && !client.client.blocked)
            .map(|(index, _)| index)
            .collect();
        let Some(&index) = self.rng.pick(&candidates) else {
            return;
        };
        self.disconnect(index, now, delta, "EVT_WU_Disconnected");
    }

    /// Take a client off the air and park it in the away pool.
    pub(crate) fn disconnect(
        &mut self,
        index: usize,
        now: DateTime<Utc>,
        delta: &mut Delta,
        key: &str,
    ) -> Option<MacAddress> {
        if index >= self.fleet.clients.len() {
            return None;
        }
        let mut demo = self.fleet.clients.remove(index);
        let from = demo
            .client
            .uplink_device_mac
            .as_ref()
            .and_then(|mac| self.device_index_by_mac(mac))
            .map(|ap| self.device_name(ap))
            .unwrap_or_default();
        let label = client_label(&demo.client);
        let message = if key == "EVT_WU_Kicked" {
            format!("{label} was disconnected by an admin")
        } else {
            format!("{label} disconnected from {from}")
        };
        let mut event = self.event(now, INFO_CLIENT, key, message);
        event.client_mac = Some(demo.client.mac.clone());
        event.device_mac.clone_from(&demo.client.uplink_device_mac);
        delta.events.push(event);

        let mac = demo.client.mac.clone();
        demo.client.bandwidth = None;
        delta.removed.push(mac.clone());
        self.fleet.away.push(demo);
        Some(mac)
    }

    /// Bring an away client back on a fresh AP.
    fn reconnect(&mut self, away_index: usize, now: DateTime<Utc>, delta: &mut Delta) {
        let Some(ap_index) = pick_ap(&self.fleet.devices, &mut self.rng) else {
            return;
        };
        if away_index >= self.fleet.away.len() {
            return;
        }
        let mut demo = self.fleet.away.remove(away_index);
        let ap = self.fleet.devices[ap_index].clone();
        let band = demo
            .client
            .wireless
            .as_ref()
            .and_then(|wireless| wireless.frequency_ghz)
            .unwrap_or(5.0);
        demo.signal_base = attach_to_ap(&mut self.rng, &mut demo.client, &ap, band, true);
        demo.client.connected_at = Some(now);

        let label = client_label(&demo.client);
        let ssid = demo
            .client
            .wireless
            .as_ref()
            .and_then(|wireless| wireless.ssid.clone())
            .unwrap_or_default();
        let ap_name = ap.name.clone().unwrap_or_default();
        let message = format!("{label} connected to \"{ssid}\" on {ap_name}");
        let mut event = self.event(now, INFO_CLIENT, "EVT_WU_Connected", message);
        event.client_mac = Some(demo.client.mac.clone());
        event.device_mac = Some(ap.mac.clone());
        delta.events.push(event);
        delta.clients.push(demo.client.clone());
        self.fleet.clients.push(demo);
    }

    fn ambient_event(&mut self, now: DateTime<Utc>) -> Event {
        let roll = self.rng.unit();
        if roll < 0.35 {
            let peer = self.rng.range(1, 4);
            let message = format!("WireGuard peer crew-laptop-0{peer} connected to Crew VPN");
            self.event(
                now,
                (EventCategory::Vpn, EventSeverity::Info),
                "EVT_VPN_PeerConnected",
                message,
            )
        } else if roll < 0.45 {
            let host = self.rng.range(50, 250);
            let message =
                format!("Admin crew-lead@neonharbor.example logged in from 10.40.10.{host}");
            self.event(
                now,
                (EventCategory::Admin, EventSeverity::Info),
                "EVT_AD_Login",
                message,
            )
        } else if roll < 0.8 {
            let shot = self.rng.range(10, 99);
            let message =
                format!("Render farm finished shot NH_0{shot}, frames synced to nas-vault");
            self.event(
                now,
                (EventCategory::System, EventSeverity::Info),
                "EVT_SYS_RenderComplete",
                message,
            )
        } else {
            self.event(
                now,
                (EventCategory::Firewall, EventSeverity::Info),
                "EVT_IPS_SignatureUpdate",
                "Threat signatures updated (54,112 rules)".into(),
            )
        }
    }

    /// The dark camera comes back after its PoE port is power cycled.
    fn revive_camera(&mut self, now: DateTime<Utc>, delta: &mut Delta) {
        let (name, port_index) = catalog::OFFLINE_CAMERA;
        if self
            .fleet
            .clients
            .iter()
            .any(|client| client.client.hostname.as_deref() == Some(name))
        {
            return;
        }
        let switch = &mut self.fleet.devices[SWITCH_BASE];
        if let Some(port) = switch
            .ports
            .iter_mut()
            .find(|port| port.index == port_index)
        {
            port.state = PortState::Up;
            port.speed_mbps = Some(1_000);
            port.poe = Some(PoeInfo {
                standard: Some("802.3bt".into()),
                enabled: true,
                state: PortState::Up,
            });
        }
        let (switch_id, switch_mac) = (switch.id.clone(), switch.mac.clone());
        let switch_name = switch.name.clone().unwrap_or_default();
        let cameras = self.fleet.network("cameras").clone();
        let ip = lan_ip(50, 24);
        let client = Client {
            id: EntityId::Uuid(uuid_from(&mut self.rng)),
            mac: MacAddress::new(self.rng.mac()),
            ip: Some(IpAddr::V4(ip)),
            name: Some(name.into()),
            hostname: Some(name.into()),
            client_type: ClientType::Wired,
            connected_at: Some(now),
            uplink_device_id: Some(switch_id),
            uplink_device_mac: Some(switch_mac),
            switch_port: Some(port_index),
            network_id: Some(cameras.id.clone()),
            network_name: Some(cameras.name.clone()),
            vlan: cameras.vlan_id,
            wireless: None,
            guest_auth: None,
            is_guest: false,
            tx_bytes: Some(0),
            rx_bytes: Some(0),
            bandwidth: None,
            os_name: Some("Embedded".into()),
            device_class: Some("Camera".into()),
            use_fixedip: true,
            fixed_ip: Some(ip),
            blocked: false,
            source: DataSource::Merged,
            updated_at: now,
        };
        let message = format!("{name} is back online on {switch_name} port {port_index}");
        let mut event = self.event(now, INFO_DEVICE, "EVT_LU_Connected", message);
        event.client_mac = Some(client.mac.clone());
        delta.events.push(event);
        delta.clients.push(client.clone());
        self.fleet.clients.push(DemoClient {
            client,
            rate: 300_000.0,
            mobile: false,
            phase: 1.3,
            signal_base: 0,
        });
    }
}
