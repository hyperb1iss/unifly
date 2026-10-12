// ── Demo simulation ──
//
// Advances the fleet about once a second: WAN waves and bursts, device
// telemetry, client traffic and signal, roams, arrivals and departures,
// security noise, and pending upgrades. Every draw comes from the seeded
// generator and the tick counter, never the wall clock, so the same seed
// replays the same animation. `now` only stamps timestamps.

mod happenings;
mod telemetry;

use chrono::{DateTime, Utc};

use super::fleet::{AP_BASE, Fleet};
use super::rng::DemoRng;
use crate::model::{Client, Device, Event, FirewallPolicy, HealthSummary, MacAddress};
use crate::store::RefreshSnapshot;

/// Ticks until the AP that is mid-upgrade at launch comes back.
const LAUNCH_UPGRADE_TICKS: u64 = 75;

/// Everything one tick (or one command) changed, ready to apply to the
/// store and broadcast.
#[derive(Debug, Default)]
pub(crate) struct Delta {
    pub devices: Vec<Device>,
    pub clients: Vec<Client>,
    pub removed: Vec<MacAddress>,
    pub policies: Vec<FirewallPolicy>,
    pub events: Vec<Event>,
    pub health: Option<Vec<HealthSummary>>,
}

/// A device transition that completes on a later tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pending {
    /// Firmware upgrade finishes and the device returns online.
    Upgrade { device: usize },
    /// A restart finishes.
    Restart { device: usize },
    /// The dark camera port comes back after a PoE power cycle.
    CameraRevive,
}

/// A temporary WAN surge.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Burst {
    pub start: u64,
    pub length: u64,
    /// Peak added bytes/s on the WAN transmit side (render uploads).
    pub tx: f64,
    /// Peak added bytes/s on the WAN receive side (asset syncs).
    pub rx: f64,
}

impl Burst {
    /// Smooth ramp up and down over the burst window, `0..=1`.
    pub(crate) fn envelope(&self, tick: u64) -> f64 {
        if tick < self.start || tick >= self.start + self.length {
            return 0.0;
        }
        let elapsed = super::rng::from_u64(tick - self.start);
        let remaining = super::rng::from_u64(self.start + self.length - tick);
        let ramp = 3.0;
        (elapsed / ramp).min(remaining / ramp).min(1.0)
    }
}

pub(crate) struct Simulation {
    pub(crate) fleet: Fleet,
    pub(crate) rng: DemoRng,
    pub(crate) tick: u64,
    pub(crate) burst: Option<Burst>,
    pub(crate) next_burst: u64,
    pub(crate) next_roam: u64,
    pub(crate) next_churn: u64,
    pub(crate) next_security: u64,
    pub(crate) next_ambient: u64,
    pub(crate) pending: Vec<(u64, Pending)>,
    /// Clients kicked by an admin, due back at the given tick.
    pub(crate) returns: Vec<(u64, MacAddress)>,
    /// Roams that happened during the session, newest last, for roam
    /// history queries.
    pub(crate) live_roams: Vec<(MacAddress, serde_json::Value)>,
    /// Gateway WAN rates from the latest tick: `(tx, rx)` bytes/s.
    pub(crate) wan: (u64, u64),
    initial_clients: usize,
}

impl Simulation {
    pub(crate) fn new(fleet: Fleet) -> Self {
        let rng = DemoRng::new(fleet.seed).fork(0x51A1);
        let initial_clients = fleet.clients.len();
        let wan = fleet.devices[super::fleet::GATEWAY]
            .stats
            .uplink_bandwidth
            .map_or((0, 0), |bandwidth| {
                (bandwidth.tx_bytes_per_sec, bandwidth.rx_bytes_per_sec)
            });
        Self {
            fleet,
            rng,
            tick: 0,
            burst: None,
            next_burst: 18,
            next_roam: 6,
            next_churn: 9,
            next_security: 14,
            next_ambient: 23,
            pending: vec![(
                LAUNCH_UPGRADE_TICKS,
                Pending::Upgrade {
                    device: AP_BASE + super::catalog::UPGRADING_AP,
                },
            )],
            returns: Vec::new(),
            live_roams: Vec::new(),
            wan,
            initial_clients,
        }
    }

    /// Advance one tick.
    pub(crate) fn step(&mut self, now: DateTime<Utc>) -> Delta {
        self.tick += 1;
        let mut delta = Delta::default();

        self.finish_pending(now, &mut delta);
        self.schedule_happenings(now, &mut delta);
        self.update_clients();
        self.update_wan();
        self.update_devices(now);

        delta.devices.clone_from(&self.fleet.devices);
        delta.clients = self
            .fleet
            .clients
            .iter()
            .map(|client| client.client.clone())
            .collect();
        delta.health = Some(self.health());
        delta
    }

    /// Current state in the shape a full refresh applies, for seeding the
    /// store on connect (and again on every reconnect).
    pub(crate) fn refresh_snapshot(&self) -> RefreshSnapshot {
        let fleet = &self.fleet;
        RefreshSnapshot {
            devices: fleet.devices.clone(),
            clients: fleet
                .clients
                .iter()
                .map(|client| client.client.clone())
                .collect(),
            networks: fleet.networks.clone(),
            wifi: fleet.wifi.clone(),
            policies: Some(fleet.policies.clone()),
            zones: Some(fleet.zones.clone()),
            acls: Some(fleet.acls.clone()),
            nat: Some(fleet.nat.clone()),
            dns: Some(Vec::new()),
            vouchers: Some(Vec::new()),
            sites: vec![fleet.site.clone()],
            events: Some(fleet.events.clone()),
            traffic_matching_lists: Vec::new(),
            firewall_groups: Some(Vec::new()),
        }
    }

    pub(crate) fn device_index_by_mac(&self, mac: &MacAddress) -> Option<usize> {
        self.fleet
            .devices
            .iter()
            .position(|device| &device.mac == mac)
    }

    pub(crate) fn client_index_by_mac(&self, mac: &MacAddress) -> Option<usize> {
        self.fleet
            .clients
            .iter()
            .position(|client| &client.client.mac == mac)
    }

    pub(crate) fn device_name(&self, index: usize) -> String {
        self.fleet
            .devices
            .get(index)
            .and_then(|device| device.name.clone())
            .unwrap_or_default()
    }

    /// Schedule a transition `ticks` from now.
    pub(crate) fn schedule(&mut self, ticks: u64, pending: Pending) {
        self.pending.retain(|(_, existing)| *existing != pending);
        self.pending.push((self.tick + ticks, pending));
    }

    /// The connected-client population the churn logic steers back toward.
    pub(crate) fn initial_clients(&self) -> usize {
        self.initial_clients
    }
}

/// Display label for a client: alias, then hostname, then MAC.
pub(crate) fn client_label(client: &Client) -> String {
    client
        .name
        .clone()
        .or_else(|| client.hostname.clone())
        .unwrap_or_else(|| client.mac.to_string())
}
