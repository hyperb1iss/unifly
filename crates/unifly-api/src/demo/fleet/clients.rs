// ── Demo clients: wired endpoints and the wireless crowd ──

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr};

use chrono::{DateTime, Duration, Utc};

use super::devices::{attach_wired, pick_ap};
use super::{
    DemoClient, Fleet, SWITCH_BASE, dhcp_pool, lan_ip, network_index, ssid_for_network, uuid_from,
};
use crate::demo::catalog::{NETWORKS, PLACES, WIRED, WIRELESS_KINDS};
use crate::demo::rng::{DemoRng, to_u64};
use crate::model::common::{Bandwidth, DataSource};
use crate::model::{
    Client, ClientType, Device, EntityId, GuestAuth, MacAddress, Network, WirelessInfo,
};

/// Phones and laptops that wander in and out during the simulation.
const AWAY_POOL: usize = 12;

/// Allocates unique MACs and per-network DHCP addresses.
struct Allocator {
    macs: HashSet<String>,
    ips: HashSet<Ipv4Addr>,
}

impl Allocator {
    fn mac(&mut self, rng: &mut DemoRng) -> MacAddress {
        loop {
            let mac = rng.mac();
            if self.macs.insert(mac.clone()) {
                return MacAddress::new(mac);
            }
        }
    }

    fn dhcp(&mut self, rng: &mut DemoRng, key: &str) -> Ipv4Addr {
        let spec = &NETWORKS[network_index(key)];
        let (first, last) = dhcp_pool(spec);
        loop {
            let host = u8::try_from(rng.range(u64::from(first), u64::from(last))).unwrap_or(first);
            let ip = lan_ip(spec.octet, host);
            if self.ips.insert(ip) {
                return ip;
            }
        }
    }
}

pub(super) fn build(fleet: &mut Fleet, rng: &mut DemoRng) {
    let mut alloc = Allocator {
        macs: fleet
            .devices
            .iter()
            .map(|device| device.mac.as_str().to_owned())
            .collect(),
        ips: HashSet::new(),
    };

    let mut clients = Vec::new();
    for spec in &WIRED {
        let switch_index = SWITCH_BASE + spec.switch;
        let network = fleet.network(spec.network).clone();
        let octet = NETWORKS[network_index(spec.network)].octet;
        let ip = lan_ip(octet, spec.host);
        alloc.ips.insert(ip);
        let switch = &mut fleet.devices[switch_index];
        attach_wired(switch, spec.port, spec.speed, spec.poe);
        let mut client = base_client(rng, &mut alloc, fleet.anchor, spec.name, &network);
        client.client_type = ClientType::Wired;
        client.ip = Some(IpAddr::V4(ip));
        client.uplink_device_id = Some(switch.id.clone());
        client.uplink_device_mac = Some(switch.mac.clone());
        client.switch_port = Some(spec.port);
        client.os_name = Some(spec.os.into());
        client.device_class = Some(spec.class.into());
        client.use_fixedip = spec.class != "Desktop";
        client.fixed_ip = client.use_fixedip.then_some(ip);
        client.name = Some(spec.name.into());
        let uptime_hours = if spec.class == "Desktop" {
            rng.range(2, 11)
        } else {
            rng.range(26, 330)
        };
        client.connected_at =
            Some(fleet.anchor - Duration::hours(i64::try_from(uptime_hours).unwrap_or(4)));
        clients.push(with_traffic(rng, client, spec.rate, false, fleet.anchor));
    }

    let mut counters = [0usize; WIRELESS_KINDS.len()];
    for (kind_index, kind) in WIRELESS_KINDS.iter().enumerate() {
        for _ in 0..kind.count {
            counters[kind_index] += 1;
            let hostname = hostname(rng, kind.prefix, counters[kind_index]);
            let Some(ap) = pick_ap(&fleet.devices, rng) else {
                continue;
            };
            let client = wireless_client(rng, &mut alloc, fleet, kind_index, &hostname, ap);
            clients.push(client);
        }
    }

    // A pool of devices that are out of range at launch.
    let mut away = Vec::new();
    for index in 0..AWAY_POOL {
        let kind_index = if index % 3 == 0 { 0 } else { 2 };
        counters[kind_index] += 1;
        let hostname = hostname(rng, WIRELESS_KINDS[kind_index].prefix, counters[kind_index]);
        let Some(ap) = pick_ap(&fleet.devices, rng) else {
            continue;
        };
        let mut client = wireless_client(rng, &mut alloc, fleet, kind_index, &hostname, ap);
        client.client.tx_bytes = Some(0);
        client.client.rx_bytes = Some(0);
        away.push(client);
    }

    fleet.clients = clients;
    fleet.away = away;
}

fn hostname(rng: &mut DemoRng, prefix: &str, count: usize) -> String {
    match prefix {
        "phone" | "pixel" | "guest" => format!("{prefix}-{:04x}", rng.next_u64() & 0xFFFF),
        "speaker" | "sensor" | "display" => {
            let place = PLACES[(count - 1) % PLACES.len()];
            if count > PLACES.len() {
                format!("{prefix}-{place}-{}", count / PLACES.len() + 1)
            } else {
                format!("{prefix}-{place}")
            }
        }
        _ => format!("{prefix}-{count:02}"),
    }
}

fn wireless_client(
    rng: &mut DemoRng,
    alloc: &mut Allocator,
    fleet: &Fleet,
    kind_index: usize,
    hostname: &str,
    ap_index: usize,
) -> DemoClient {
    let kind = &WIRELESS_KINDS[kind_index];
    let network = fleet.network(kind.network).clone();
    let ap = &fleet.devices[ap_index];
    let mut client = base_client(rng, alloc, fleet.anchor, hostname, &network);
    client.client_type = ClientType::Wireless;
    client.ip = Some(IpAddr::V4(alloc.dhcp(rng, kind.network)));
    client.os_name = Some(kind.os.into());
    client.device_class = Some(kind.class.into());
    let named = !matches!(kind.prefix, "phone" | "pixel" | "guest");
    client.name = named.then(|| hostname.to_owned());
    let minutes = if kind.mobile {
        rng.range(4, 540)
    } else {
        rng.range(600, 20_000)
    };
    client.connected_at =
        Some(fleet.anchor - Duration::minutes(i64::try_from(minutes).unwrap_or(30)));
    attach_to_ap(rng, &mut client, ap, kind.band, kind.mobile);

    if kind.network == "guest" {
        client.is_guest = true;
        client.guest_auth = Some(GuestAuth {
            authorized: true,
            method: Some("voucher".into()),
            expires_at: Some(fleet.anchor + Duration::hours(6)),
            tx_bytes: None,
            rx_bytes: None,
            elapsed_minutes: Some(minutes),
        });
    }

    with_traffic(rng, client, kind.rate, kind.mobile, fleet.anchor)
}

/// Point a wireless client at `ap` on the best band it supports, with a
/// believable signal and link rate, and return the signal it settled on.
/// Also used by the simulation for roams and arrivals.
pub(crate) fn attach_to_ap(
    rng: &mut DemoRng,
    client: &mut Client,
    ap: &Device,
    preferred_band: f32,
    mobile: bool,
) -> i32 {
    let radio = ap
        .radios
        .iter()
        .find(|radio| (radio.frequency_ghz - preferred_band).abs() < 0.1)
        .or_else(|| {
            ap.radios
                .iter()
                .find(|radio| (radio.frequency_ghz - 5.0).abs() < 0.1)
        })
        .or_else(|| ap.radios.first());
    let frequency = radio.map_or(preferred_band, |radio| radio.frequency_ghz);
    let channel = radio.and_then(|radio| radio.channel);
    // Outdoor APs reach clients through walls and glass, so their links
    // run weaker; that is the AP the WiFi screen flags for attention.
    let outdoor = ap
        .model
        .as_deref()
        .is_some_and(|model| model.contains("Outdoor"));
    let signal = match (outdoor, mobile) {
        (true, _) => rng.range(63, 79),
        (false, true) => rng.range(42, 68),
        (false, false) => rng.range(48, 72),
    };
    let signal = -i32::try_from(signal).unwrap_or(60);
    let (low, high) = if frequency > 5.5 {
        (1_200_000, 2_882_000)
    } else if frequency > 4.5 {
        (400_000, 1_201_000)
    } else {
        (72_000, 172_000)
    };
    let tx_rate = rng.range(low, high);
    let ssid = current_ssid(client);

    client.uplink_device_id = Some(ap.id.clone());
    client.uplink_device_mac = Some(ap.mac.clone());
    client.switch_port = None;
    client.wireless = Some(WirelessInfo {
        ssid: Some(ssid.into()),
        bssid: Some(ap.mac.clone()),
        channel,
        frequency_ghz: Some(frequency),
        signal_dbm: Some(signal),
        noise_dbm: Some(-i32::try_from(rng.range(91, 96)).unwrap_or(94)),
        satisfaction: Some(satisfaction(signal)),
        tx_rate_kbps: Some(tx_rate),
        rx_rate_kbps: Some(tx_rate * 9 / 10),
    });
    signal
}

/// The SSID a client joins follows from its VLAN.
fn current_ssid(client: &Client) -> &'static str {
    let key = match client.vlan {
        Some(30) => "guest",
        Some(40) => "iot",
        Some(1) => "mgmt",
        _ => "studio",
    };
    ssid_for_network(key)
}

/// Experience score from signal strength: links at -50 dBm or better score
/// 99, -60 lands near 87, -70 near 75, and it keeps sliding from there.
pub(crate) fn satisfaction(signal_dbm: i32) -> u8 {
    let deficit = (-signal_dbm - 50).max(0);
    let score = 99 - deficit * 6 / 5;
    u8::try_from(score.clamp(40, 99)).unwrap_or(90)
}

fn base_client(
    rng: &mut DemoRng,
    alloc: &mut Allocator,
    anchor: DateTime<Utc>,
    hostname: &str,
    network: &Network,
) -> Client {
    Client {
        id: EntityId::Uuid(uuid_from(rng)),
        mac: alloc.mac(rng),
        ip: None,
        name: None,
        hostname: Some(hostname.into()),
        client_type: ClientType::Wired,
        connected_at: Some(anchor),
        uplink_device_id: None,
        uplink_device_mac: None,
        switch_port: None,
        network_id: Some(network.id.clone()),
        vlan: network.vlan_id,
        wireless: None,
        guest_auth: None,
        is_guest: false,
        tx_bytes: None,
        rx_bytes: None,
        bandwidth: None,
        os_name: None,
        device_class: None,
        use_fixedip: false,
        fixed_ip: None,
        blocked: false,
        source: DataSource::Merged,
        updated_at: anchor,
    }
}

/// Seed session byte counters and a current rate from a client's typical
/// throughput and how long it has been connected.
fn with_traffic(
    rng: &mut DemoRng,
    mut client: Client,
    rate: f64,
    mobile: bool,
    anchor: DateTime<Utc>,
) -> DemoClient {
    let connected_secs = client
        .connected_at
        .map_or(0, |at| (anchor - at).num_seconds().max(0));
    let connected_secs = u32::try_from(connected_secs).unwrap_or(u32::MAX);
    let activity = rng.between(0.12, 0.45);
    // Session totals assume quieter stretches than the live rate suggests.
    let total = f64::from(connected_secs) * rate * activity * 0.5;
    client.rx_bytes = Some(to_u64(total * 0.7));
    client.tx_bytes = Some(to_u64(total * 0.3));
    client.bandwidth = Some(Bandwidth {
        tx_bytes_per_sec: to_u64(rate * activity * 0.3),
        rx_bytes_per_sec: to_u64(rate * activity * 0.7),
    });
    let signal_base = client
        .wireless
        .as_ref()
        .and_then(|wireless| wireless.signal_dbm)
        .unwrap_or(-60);
    DemoClient {
        client,
        rate,
        mobile,
        phase: rng.between(0.0, std::f64::consts::TAU),
        signal_base,
    }
}
