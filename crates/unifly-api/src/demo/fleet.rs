// ── Demo fleet generation ──
//
// Builds the whole "Neon Harbor Studios" network from a seed and an anchor
// time. The structure (names, MACs, IPs, channels, policies) depends only on
// the seed; timestamps are offsets from the anchor, so tests can pin it.

pub(crate) mod clients;
pub(crate) mod devices;
pub(crate) mod history;
mod policy;

use std::net::{IpAddr, Ipv4Addr};

use chrono::{DateTime, Utc};

use super::catalog::{self, NETWORKS, NetworkSpec, SITE_NAME};
use super::rng::DemoRng;
use crate::model::common::DataSource;
use crate::model::{
    AclRule, Client, Device, DhcpConfig, EntityId, EntityOrigin, Event, FirewallPolicy,
    FirewallZone, Ipv6Mode, NatPolicy, Network, NetworkManagement, NetworkPurpose, Site,
    WifiBroadcast,
};
use crate::session::models::RogueAp;

/// Index of the gateway in [`Fleet::devices`].
pub(crate) const GATEWAY: usize = 0;
/// Index of the first switch in [`Fleet::devices`].
pub(crate) const SWITCH_BASE: usize = 1;
/// Number of switches.
pub(crate) const SWITCH_COUNT: usize = 3;
/// Index of the first access point in [`Fleet::devices`].
pub(crate) const AP_BASE: usize = SWITCH_BASE + SWITCH_COUNT;

/// A connected (or temporarily away) client plus the traits that drive its
/// simulated traffic.
#[derive(Debug, Clone)]
pub(crate) struct DemoClient {
    pub client: Client,
    /// Typical sustained throughput in bytes per second when active.
    pub rate: f64,
    /// Whether the client roams between APs and comes and goes.
    pub mobile: bool,
    /// Per-client phase offset for the traffic waves.
    pub phase: f64,
    /// Signal the client settled on at its current AP; live readings wave
    /// around it instead of drifting off.
    pub signal_base: i32,
}

/// The complete synthetic network.
#[derive(Debug, Clone)]
pub(crate) struct Fleet {
    pub seed: u64,
    pub anchor: DateTime<Utc>,
    pub site: Site,
    pub networks: Vec<Network>,
    pub wifi: Vec<WifiBroadcast>,
    pub zones: Vec<FirewallZone>,
    pub policies: Vec<FirewallPolicy>,
    pub acls: Vec<AclRule>,
    pub nat: Vec<NatPolicy>,
    pub devices: Vec<Device>,
    pub clients: Vec<DemoClient>,
    /// Mobile clients that are out of range right now and may come back.
    pub away: Vec<DemoClient>,
    pub events: Vec<Event>,
    pub neighbors: Vec<RogueAp>,
}

impl Fleet {
    /// Generate the fleet for `seed`, with timestamps relative to `anchor`.
    pub(crate) fn generate(seed: u64, anchor: DateTime<Utc>) -> Self {
        let rng = DemoRng::new(seed);
        let site = Site {
            id: EntityId::Uuid(uuid_from(&mut rng.fork(1))),
            internal_name: "default".into(),
            name: SITE_NAME.into(),
            device_count: None,
            client_count: None,
            source: DataSource::IntegrationApi,
        };

        let mut net_rng = rng.fork(2);
        let networks: Vec<Network> = NETWORKS
            .iter()
            .map(|spec| build_network(spec, &mut net_rng))
            .collect();

        let mut fleet = Self {
            seed,
            anchor,
            site,
            networks,
            wifi: Vec::new(),
            zones: Vec::new(),
            policies: Vec::new(),
            acls: Vec::new(),
            nat: Vec::new(),
            devices: Vec::new(),
            clients: Vec::new(),
            away: Vec::new(),
            events: Vec::new(),
            neighbors: Vec::new(),
        };

        policy::build(&mut fleet, &mut rng.fork(3));
        devices::build(&mut fleet, &mut rng.fork(4));
        clients::build(&mut fleet, &mut rng.fork(5));
        devices::finish(&mut fleet);
        history::build(&mut fleet, &mut rng.fork(6));

        let device_count = u32::try_from(fleet.devices.len()).ok();
        let client_count = u32::try_from(fleet.clients.len()).ok();
        fleet.site.device_count = device_count;
        fleet.site.client_count = client_count;
        fleet
    }

    /// Network by catalog key (`"studio"`, `"iot"`, ...).
    pub(crate) fn network(&self, key: &str) -> &Network {
        &self.networks[network_index(key)]
    }

    /// Access point devices, in catalog order.
    pub(crate) fn aps(&self) -> &[Device] {
        &self.devices[AP_BASE..]
    }
}

/// Position of a network key in [`catalog::NETWORKS`]; unknown keys fall
/// back to the management network.
pub(crate) fn network_index(key: &str) -> usize {
    NETWORKS
        .iter()
        .position(|spec| spec.key == key)
        .unwrap_or_default()
}

/// `10.40.<octet>.<host>`
pub(crate) fn lan_ip(octet: u8, host: u8) -> Ipv4Addr {
    Ipv4Addr::new(10, 40, octet, host)
}

pub(crate) fn uuid_from(rng: &mut DemoRng) -> uuid::Uuid {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&rng.next_u64().to_be_bytes());
    bytes[8..].copy_from_slice(&rng.next_u64().to_be_bytes());
    uuid::Builder::from_random_bytes(bytes).into_uuid()
}

/// DHCP pool `(first, last)` host octets for a network.
pub(crate) fn dhcp_pool(spec: &NetworkSpec) -> (u8, u8) {
    match spec.key {
        "mgmt" | "render" | "cameras" => (100, 199),
        "guest" => (20, 250),
        "iot" => (60, 250),
        _ => (50, 250),
    }
}

fn build_network(spec: &NetworkSpec, rng: &mut DemoRng) -> Network {
    let gateway = lan_ip(spec.octet, 1);
    let (first, last) = dhcp_pool(spec);
    let ipv6_prefix = spec
        .ipv6
        .then(|| format!("2001:db8:4e48:{}::/64", spec.vlan));
    Network {
        id: EntityId::Uuid(uuid_from(rng)),
        name: spec.name.into(),
        enabled: true,
        management: Some(NetworkManagement::Gateway),
        purpose: Some(if spec.guest {
            NetworkPurpose::Guest
        } else {
            NetworkPurpose::Corporate
        }),
        is_default: spec.key == "mgmt",
        vlan_id: Some(spec.vlan),
        subnet: Some(format!("10.40.{}.0/24", spec.octet)),
        gateway_ip: Some(gateway),
        dhcp: Some(DhcpConfig {
            enabled: true,
            range_start: Some(lan_ip(spec.octet, first)),
            range_stop: Some(lan_ip(spec.octet, last)),
            lease_time_secs: Some(if spec.guest { 7_200 } else { 86_400 }),
            dns_servers: vec![IpAddr::V4(gateway)],
            gateway: Some(gateway),
        }),
        ipv6_enabled: spec.ipv6,
        ipv6_mode: spec.ipv6.then_some(Ipv6Mode::PrefixDelegation),
        ipv6_prefix,
        dhcpv6_enabled: false,
        slaac_enabled: spec.ipv6,
        ntp_server: None,
        pxe_enabled: spec.key == "render",
        tftp_server: (spec.key == "render").then(|| "10.40.20.41".to_owned()),
        firewall_zone_id: None,
        isolation_enabled: spec.isolated,
        internet_access_enabled: spec.internet,
        mdns_forwarding_enabled: matches!(spec.key, "studio" | "iot"),
        cellular_backup_enabled: false,
        origin: Some(if spec.key == "mgmt" {
            EntityOrigin::SystemDefined
        } else {
            EntityOrigin::UserDefined
        }),
        source: DataSource::IntegrationApi,
    }
}

/// Re-exported so the clients builder and simulation agree on SSIDs.
pub(crate) fn ssid_for_network(key: &str) -> &'static str {
    match key {
        "guest" => catalog::SSID_GUEST,
        "iot" => catalog::SSID_IOT,
        "mgmt" => catalog::SSID_CREW,
        _ => catalog::SSID_STUDIO,
    }
}
