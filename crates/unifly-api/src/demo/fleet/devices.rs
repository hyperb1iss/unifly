// ── Demo infrastructure: gateway, switches, access points ──

use std::net::IpAddr;

use chrono::{DateTime, Duration, Utc};

use super::{AP_BASE, DemoClient, Fleet, GATEWAY, SWITCH_BASE, lan_ip, uuid_from};
use crate::demo::catalog::{
    self, AP_FIRMWARE, AP_FIRMWARE_OLD, APS, CHANNELS_5, CHANNELS_6, CHANNELS_24, GATEWAY_FIRMWARE,
    OFFLINE_AP, STALE_FIRMWARE_AP, SWITCH_FIRMWARE, UPGRADING_AP,
};
use crate::demo::rng::{DemoRng, to_u64};
use crate::model::common::{Bandwidth, DataSource};
use crate::model::{
    Device, DeviceState, DeviceStats, DeviceType, EntityId, EntityOrigin, MacAddress, PoeInfo,
    Port, PortConnector, PortState, Radio, UplinkMedium,
};

/// Where each AP plugs in: `Some((switch device index, switch port))`, or
/// `None` for a wireless mesh uplink to the AP named in [`MESH_PARENT`].
const AP_UPLINKS: [Option<(usize, u32)>; 9] = [
    Some((1, 17)),
    Some((1, 18)),
    Some((2, 13)),
    Some((1, 19)),
    Some((1, 20)),
    Some((1, 21)),
    None,
    Some((1, 23)),
    Some((1, 24)),
];
/// The wired AP the mesh AP hangs off (index into [`catalog::APS`]).
const MESH_PARENT: usize = 1;

/// Build all infrastructure devices and wire up their uplinks.
pub(super) fn build(fleet: &mut Fleet, rng: &mut DemoRng) {
    let anchor = fleet.anchor;
    let mut devices = vec![
        gateway(rng, anchor),
        switch(rng, anchor, "sw-core", "USW Pro Max 24 PoE", 2, (24, 2)),
        switch(rng, anchor, "sw-render", "USW Pro Max 16 PoE", 3, (16, 2)),
        switch(rng, anchor, "sw-lounge", "USW Flex", 4, (5, 0)),
    ];
    for (index, spec) in APS.iter().enumerate() {
        devices.push(access_point(rng, anchor, index, spec));
    }

    link(&mut devices, SWITCH_BASE, GATEWAY, 11, 25, 10_000);
    link(&mut devices, SWITCH_BASE + 1, SWITCH_BASE, 26, 18, 10_000);
    link(&mut devices, SWITCH_BASE + 2, SWITCH_BASE, 22, 1, 1_000);
    for (index, uplink) in AP_UPLINKS.iter().enumerate() {
        let ap = AP_BASE + index;
        if let Some((switch, port)) = *uplink {
            link(&mut devices, ap, switch, port, 1, 2_500);
        } else {
            let parent = &devices[AP_BASE + MESH_PARENT];
            let (parent_id, parent_mac) = (parent.id.clone(), parent.mac.clone());
            let child = &mut devices[ap];
            child.uplink_device_id = Some(parent_id);
            child.uplink_device_mac = Some(parent_mac);
            child.uplink_port_idx = None;
            child.uplink_medium = Some(UplinkMedium::Wireless);
            child.ports.clear();
        }
    }

    // An offline AP leaves its PoE port dark.
    if let Some((switch, port)) = AP_UPLINKS[OFFLINE_AP] {
        set_port_down(&mut devices[switch], port);
    }

    fleet.devices = devices;
}

/// Mark every port that carries a known client, then derive client counts
/// and a first round of uplink stats so the very first frame is populated.
pub(super) fn finish(fleet: &mut Fleet) {
    let (_, camera_port) = catalog::OFFLINE_CAMERA;
    if let Some(port) = fleet.devices[SWITCH_BASE]
        .ports
        .iter_mut()
        .find(|port| port.index == camera_port)
    {
        port.state = PortState::Down;
        port.speed_mbps = None;
        port.poe = Some(poe(true, PortState::Down));
    }

    let clients = fleet.clients.clone();
    for device in &mut fleet.devices {
        refresh_device_load(device, &clients);
    }
    // Where the simulation's WAN waves start, so the first chart sample
    // does not spike.
    if let Some(gateway) = fleet.devices.get_mut(GATEWAY) {
        gateway.client_count = u32::try_from(clients.len()).ok();
        gateway.stats.uplink_bandwidth = Some(Bandwidth {
            tx_bytes_per_sec: to_u64(catalog::WAN_TX_BASE * 0.92),
            rx_bytes_per_sec: to_u64(catalog::WAN_RX_BASE * 0.84),
        });
    }
}

/// Recompute a device's client count and uplink throughput from the
/// clients attached to it (directly, for switches and APs).
pub(crate) fn refresh_device_load(device: &mut Device, clients: &[DemoClient]) {
    if device.device_type == DeviceType::Gateway || !device.state.is_online() {
        if !device.state.is_online() {
            device.client_count = Some(0);
        }
        return;
    }
    let attached: Vec<&DemoClient> = clients
        .iter()
        .filter(|client| client.client.uplink_device_mac.as_ref() == Some(&device.mac))
        .collect();
    device.client_count = u32::try_from(attached.len()).ok();
    let (tx, rx) = attached.iter().fold((0u64, 0u64), |(tx, rx), client| {
        let bandwidth = client.client.bandwidth.unwrap_or_default();
        (
            tx.saturating_add(bandwidth.tx_bytes_per_sec),
            rx.saturating_add(bandwidth.rx_bytes_per_sec),
        )
    });
    device.stats.uplink_bandwidth = Some(Bandwidth {
        tx_bytes_per_sec: tx,
        rx_bytes_per_sec: rx,
    });
}

fn base_device(
    rng: &mut DemoRng,
    anchor: DateTime<Utc>,
    name: &str,
    model: &str,
    device_type: DeviceType,
) -> Device {
    let adopted_days = i64::try_from(rng.range(380, 720)).unwrap_or(400);
    let provisioned_days = i64::try_from(rng.range(1, 20)).unwrap_or(3);
    Device {
        id: EntityId::Uuid(uuid_from(rng)),
        mac: MacAddress::new(rng.mac()),
        ip: None,
        wan_ipv6: None,
        name: Some(name.into()),
        model: Some(model.into()),
        device_type,
        state: DeviceState::Online,
        firmware_version: None,
        firmware_updatable: false,
        adopted_at: Some(anchor - Duration::days(adopted_days)),
        provisioned_at: Some(anchor - Duration::days(provisioned_days)),
        last_seen: Some(anchor),
        serial: Some(format!("{:012X}", rng.next_u64() & 0xFFFF_FFFF_FFFF)),
        supported: true,
        ports: Vec::new(),
        radios: Vec::new(),
        uplink_device_id: None,
        uplink_device_mac: None,
        uplink_port_idx: None,
        uplink_medium: None,
        has_switching: false,
        has_access_point: false,
        stats: DeviceStats {
            last_heartbeat: Some(anchor),
            next_heartbeat: Some(anchor + Duration::seconds(10)),
            ..DeviceStats::default()
        },
        client_count: Some(0),
        origin: Some(EntityOrigin::UserDefined),
        source: DataSource::Merged,
        updated_at: anchor,
    }
}

fn gateway(rng: &mut DemoRng, anchor: DateTime<Utc>) -> Device {
    let mut device = base_device(rng, anchor, "gw-harbor", "UDM Pro Max", DeviceType::Gateway);
    device.ip = catalog::WAN_IP.parse().ok();
    device.wan_ipv6 = Some(catalog::WAN_IPV6.into());
    device.firmware_version = Some(GATEWAY_FIRMWARE.into());
    device.has_switching = true;
    device.stats.uptime_secs = Some(41 * 86_400 + rng.range(0, 7_200));
    device.stats.cpu_utilization_pct = Some(22.0);
    device.stats.memory_utilization_pct = Some(61.0);
    device.stats.load_average_1m = Some(0.9);
    device.stats.load_average_5m = Some(0.8);
    device.stats.load_average_15m = Some(0.7);
    device.ports = (1..=8)
        .map(|index| rj45(index, None, PortState::Down, None, 1_000, None))
        .collect();
    device.ports.push(rj45(
        9,
        Some("WAN"),
        PortState::Up,
        Some(2_500),
        2_500,
        None,
    ));
    device
        .ports
        .push(sfp(10, Some("WAN2"), PortState::Down, None));
    device.ports.push(sfp(11, None, PortState::Down, None));
    device
}

fn switch(
    rng: &mut DemoRng,
    anchor: DateTime<Utc>,
    name: &str,
    model: &str,
    host: u8,
    (copper, fiber): (u32, u32),
) -> Device {
    let mut device = base_device(rng, anchor, name, model, DeviceType::Switch);
    device.ip = Some(IpAddr::V4(lan_ip(0, host)));
    device.firmware_version = Some(SWITCH_FIRMWARE.into());
    device.has_switching = true;
    device.stats.uptime_secs = Some(41 * 86_400 + rng.range(0, 7_200));
    device.stats.cpu_utilization_pct = Some(rng.between(6.0, 14.0));
    device.stats.memory_utilization_pct = Some(rng.between(36.0, 46.0));
    device.stats.load_average_1m = Some(rng.between(0.1, 0.4));
    device.stats.load_average_5m = Some(rng.between(0.1, 0.3));
    device.stats.load_average_15m = Some(rng.between(0.1, 0.3));
    // A PoE-in Flex is powered on port 1; every other copper port sources
    // PoE on the Pro Max models.
    let sources_poe = fiber > 0;
    device.ports = (1..=copper)
        .map(|index| {
            let max = if index > copper.saturating_sub(8) && sources_poe {
                2_500
            } else {
                1_000
            };
            rj45(
                index,
                None,
                PortState::Down,
                None,
                max,
                sources_poe.then(|| poe(false, PortState::Down)),
            )
        })
        .collect();
    for index in 1..=fiber {
        device
            .ports
            .push(sfp(copper + index, None, PortState::Down, None));
    }
    device
}

fn access_point(
    rng: &mut DemoRng,
    anchor: DateTime<Utc>,
    index: usize,
    spec: &catalog::ApSpec,
) -> Device {
    let mut device = base_device(rng, anchor, spec.name, spec.model, DeviceType::AccessPoint);
    let host = 11 + u8::try_from(index).unwrap_or_default();
    device.ip = Some(IpAddr::V4(lan_ip(0, host)));
    device.has_access_point = true;
    device.firmware_version = Some(AP_FIRMWARE.into());
    device.stats.uptime_secs = Some(rng.range(9, 41) * 86_400 + rng.range(0, 86_399));
    device.stats.cpu_utilization_pct = Some(rng.between(6.0, 18.0));
    device.stats.memory_utilization_pct = Some(rng.between(52.0, 68.0));
    device.stats.load_average_1m = Some(rng.between(0.2, 0.7));
    device.stats.load_average_5m = Some(rng.between(0.2, 0.6));
    device.stats.load_average_15m = Some(rng.between(0.2, 0.5));
    device.ports = vec![rj45(1, None, PortState::Up, Some(2_500), 2_500, None)];
    device.radios = radios(rng, index, spec);

    match index {
        OFFLINE_AP => {
            device.state = DeviceState::Offline;
            device.last_seen = Some(anchor - Duration::minutes(192));
            device.stats = DeviceStats::default();
            for radio in &mut device.radios {
                radio.channel_utilization_pct = None;
                radio.tx_retries_pct = None;
            }
        }
        UPGRADING_AP => {
            device.state = DeviceState::Updating;
            device.firmware_version = Some(AP_FIRMWARE_OLD.into());
            device.stats.uptime_secs = None;
        }
        STALE_FIRMWARE_AP => {
            device.firmware_version = Some(AP_FIRMWARE_OLD.into());
            device.firmware_updatable = true;
        }
        _ => {}
    }
    device
}

fn radios(rng: &mut DemoRng, index: usize, spec: &catalog::ApSpec) -> Vec<Radio> {
    let standard = if spec.wifi7 { "802.11be" } else { "802.11ax" };
    let mut radios = vec![
        Radio {
            frequency_ghz: 2.4,
            channel: Some(CHANNELS_24[index % CHANNELS_24.len()]),
            channel_width_mhz: Some(20),
            wlan_standard: Some(standard.into()),
            tx_retries_pct: Some(rng.between(4.0, 11.0)),
            channel_utilization_pct: Some(rng.between(28.0, 55.0)),
        },
        Radio {
            frequency_ghz: 5.0,
            channel: Some(CHANNELS_5[index % CHANNELS_5.len()]),
            channel_width_mhz: Some(if spec.wifi7 { 80 } else { 40 }),
            wlan_standard: Some(standard.into()),
            tx_retries_pct: Some(rng.between(1.0, 6.0)),
            channel_utilization_pct: Some(rng.between(9.0, 34.0)),
        },
    ];
    if spec.six_ghz {
        radios.push(Radio {
            frequency_ghz: 6.0,
            channel: Some(CHANNELS_6[index % CHANNELS_6.len()]),
            channel_width_mhz: Some(if spec.model.ends_with("Max") {
                320
            } else {
                160
            }),
            wlan_standard: Some(standard.into()),
            tx_retries_pct: Some(rng.between(0.5, 3.0)),
            channel_utilization_pct: Some(rng.between(2.0, 12.0)),
        });
    }
    radios
}

/// Connect `child` to `parent_port` on `parent` through the child's
/// `child_port`, lighting both ends.
fn link(
    devices: &mut [Device],
    child: usize,
    parent: usize,
    parent_port: u32,
    child_port: u32,
    speed: u32,
) {
    let (parent_id, parent_mac) = (devices[parent].id.clone(), devices[parent].mac.clone());
    let child_is_ap = devices[child].device_type == DeviceType::AccessPoint;

    if let Some(port) = devices[parent]
        .ports
        .iter_mut()
        .find(|port| port.index == parent_port)
    {
        port.state = PortState::Up;
        port.speed_mbps = Some(speed);
        if child_is_ap {
            port.poe = Some(poe(true, PortState::Up));
        }
    }

    let device = &mut devices[child];
    device.uplink_device_id = Some(parent_id);
    device.uplink_device_mac = Some(parent_mac);
    device.uplink_port_idx = Some(parent_port);
    device.uplink_medium = Some(UplinkMedium::Wired);
    if let Some(port) = device
        .ports
        .iter_mut()
        .find(|port| port.index == child_port)
    {
        port.name = Some("Uplink".into());
        port.state = PortState::Up;
        port.speed_mbps = Some(speed);
    }
}

fn set_port_down(device: &mut Device, index: u32) {
    if let Some(port) = device.ports.iter_mut().find(|port| port.index == index) {
        port.state = PortState::Down;
        port.speed_mbps = None;
        if let Some(poe) = port.poe.as_mut() {
            poe.state = PortState::Down;
        }
    }
}

/// Light a switch port for a wired client.
pub(super) fn attach_wired(device: &mut Device, index: u32, speed: u32, powered: bool) {
    if let Some(port) = device.ports.iter_mut().find(|port| port.index == index) {
        port.state = PortState::Up;
        port.speed_mbps = Some(speed.min(port.max_speed_mbps.unwrap_or(speed)));
        if powered {
            port.poe = Some(poe(true, PortState::Up));
        }
    }
}

fn rj45(
    index: u32,
    name: Option<&str>,
    state: PortState,
    speed: Option<u32>,
    max: u32,
    poe: Option<PoeInfo>,
) -> Port {
    Port {
        index,
        name: name.map(Into::into),
        state,
        speed_mbps: speed,
        max_speed_mbps: Some(max),
        connector: Some(PortConnector::Rj45),
        poe,
    }
}

fn sfp(index: u32, name: Option<&str>, state: PortState, speed: Option<u32>) -> Port {
    Port {
        index,
        name: name.map(Into::into),
        state,
        speed_mbps: speed,
        max_speed_mbps: Some(10_000),
        connector: Some(PortConnector::SfpPlus),
        poe: None,
    }
}

fn poe(enabled: bool, state: PortState) -> PoeInfo {
    PoeInfo {
        standard: Some("802.3bt".into()),
        enabled,
        state,
    }
}

/// Weighted pick of an online access point for a new wireless client.
pub(crate) fn pick_ap(devices: &[Device], rng: &mut DemoRng) -> Option<usize> {
    let weights: Vec<(usize, u32)> = APS
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            devices
                .get(AP_BASE + index)
                .is_some_and(|device| device.state.is_online())
        })
        .map(|(index, spec)| (index, spec.weight))
        .filter(|(_, weight)| *weight > 0)
        .collect();
    let total: u32 = weights.iter().map(|(_, weight)| weight).sum();
    if total == 0 {
        return None;
    }
    let mut roll = u32::try_from(rng.range(0, u64::from(total) - 1)).unwrap_or(0);
    for (index, weight) in weights {
        if roll < weight {
            return Some(AP_BASE + index);
        }
        roll -= weight;
    }
    None
}
