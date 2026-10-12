use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr};

use chrono::{DateTime, Duration, TimeZone, Utc};

use super::fleet::{Fleet, GATEWAY};
use super::sim::Simulation;
use super::{DEFAULT_DEMO_SEED, DemoFleetOptions};
use crate::command::Command;
use crate::core_error::CoreError;
use crate::model::{ClientType, DeviceState, DeviceType, MacAddress, PortState};
use crate::{ConnectionState, Controller};

fn anchor() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 18, 15, 30, 0)
        .single()
        .expect("valid anchor")
}

fn fleet() -> Fleet {
    Fleet::generate(DEFAULT_DEMO_SEED, anchor())
}

fn fingerprint(fleet: &Fleet) -> String {
    serde_json::to_string(&(
        &fleet.devices,
        fleet
            .clients
            .iter()
            .map(|client| &client.client)
            .collect::<Vec<_>>(),
        &fleet.networks,
        &fleet.wifi,
        &fleet.policies,
        &fleet.events,
        &fleet.neighbors,
    ))
    .expect("fleet serializes")
}

/// `10.40.20.0/24` contains `10.40.20.7`.
fn subnet_contains(subnet: &str, ip: Ipv4Addr) -> bool {
    let Some((base, prefix)) = subnet.split_once('/') else {
        return false;
    };
    let (Ok(base), Ok(prefix)) = (base.parse::<Ipv4Addr>(), prefix.parse::<u32>()) else {
        return false;
    };
    let mask = u32::MAX.checked_shl(32 - prefix).unwrap_or(0);
    (u32::from(base) & mask) == (u32::from(ip) & mask)
}

#[test]
fn same_seed_same_fleet() {
    assert_eq!(fingerprint(&fleet()), fingerprint(&fleet()));
    let other = Fleet::generate(DEFAULT_DEMO_SEED + 1, anchor());
    assert_ne!(fingerprint(&fleet()), fingerprint(&other));
}

#[test]
fn fleet_has_the_promised_shape() {
    let fleet = fleet();
    let count = |kind: DeviceType| {
        fleet
            .devices
            .iter()
            .filter(|device| device.device_type == kind)
            .count()
    };
    assert_eq!(count(DeviceType::Gateway), 1);
    assert!((3..=4).contains(&count(DeviceType::Switch)));
    assert!((8..=12).contains(&count(DeviceType::AccessPoint)));
    assert!((120..=180).contains(&fleet.clients.len()));
    assert_eq!(fleet.networks.len(), 6);
    assert!((3..=4).contains(&fleet.wifi.len()));

    let states: HashSet<_> = fleet
        .devices
        .iter()
        .map(|device| format!("{:?}", device.state))
        .collect();
    assert!(states.contains("Offline"));
    assert!(states.contains("Updating"));
    assert!(fleet.devices.iter().any(|device| device.firmware_updatable));
    let (_, camera_port) = super::catalog::OFFLINE_CAMERA;
    let dark = fleet.devices[super::fleet::SWITCH_BASE]
        .ports
        .iter()
        .find(|port| port.index == camera_port)
        .expect("camera port");
    assert_eq!(
        dark.state,
        PortState::Down,
        "the camera port should be dark"
    );
    assert!(dark.poe.as_ref().is_some_and(|poe| poe.enabled));
}

#[test]
fn macs_are_unique_and_locally_administered() {
    let fleet = fleet();
    let macs: Vec<&MacAddress> = fleet
        .devices
        .iter()
        .map(|device| &device.mac)
        .chain(fleet.clients.iter().map(|client| &client.client.mac))
        .chain(fleet.away.iter().map(|client| &client.client.mac))
        .collect();
    let unique: HashSet<_> = macs.iter().collect();
    assert_eq!(unique.len(), macs.len(), "duplicate MAC in fleet");
    for mac in macs {
        let first = u8::from_str_radix(&mac.as_str()[..2], 16).expect("hex octet");
        assert_eq!(first & 0x03, 0x02, "{mac} must be local unicast");
    }
}

#[test]
fn clients_attach_to_real_devices_and_sit_in_their_subnets() {
    let fleet = fleet();
    let by_mac: HashMap<&MacAddress, _> = fleet
        .devices
        .iter()
        .map(|device| (&device.mac, device))
        .collect();
    let mut ips = HashSet::new();

    for demo in &fleet.clients {
        let client = &demo.client;
        let uplink = client
            .uplink_device_mac
            .as_ref()
            .and_then(|mac| by_mac.get(mac))
            .unwrap_or_else(|| panic!("{:?} has no uplink device", client.hostname));
        if client.client_type == ClientType::Wireless {
            assert_eq!(uplink.device_type, DeviceType::AccessPoint);
            assert!(uplink.state.is_online(), "client on a dark AP");
            let wireless = client.wireless.as_ref().expect("wireless info");
            assert_eq!(wireless.bssid.as_ref(), Some(&uplink.mac));
            let channel = wireless.channel.expect("channel");
            assert!(
                uplink
                    .radios
                    .iter()
                    .any(|radio| radio.channel == Some(channel)),
                "client channel {channel} is not on its AP"
            );
        } else {
            assert_eq!(uplink.device_type, DeviceType::Switch);
            let port = client.switch_port.expect("wired client port");
            let port = uplink
                .ports
                .iter()
                .find(|candidate| candidate.index == port)
                .expect("switch port exists");
            assert_eq!(port.state, PortState::Up);
        }

        let network = fleet
            .networks
            .iter()
            .find(|network| Some(&network.id) == client.network_id.as_ref())
            .expect("client network exists");
        let Some(IpAddr::V4(ip)) = client.ip else {
            panic!("client without an IPv4 address");
        };
        let subnet = network.subnet.as_deref().expect("subnet");
        assert!(subnet_contains(subnet, ip), "{ip} outside {subnet}");
        assert!(ips.insert(ip), "duplicate client IP {ip}");
        assert_eq!(client.vlan, network.vlan_id);
    }
}

#[test]
fn uplinks_form_a_tree_rooted_at_the_gateway() {
    let fleet = fleet();
    let gateway = &fleet.devices[GATEWAY];
    assert_eq!(gateway.device_type, DeviceType::Gateway);
    assert_eq!(gateway.uplink_device_mac, None);

    for device in &fleet.devices {
        let mut cursor = device;
        let mut hops = 0;
        while cursor.device_type != DeviceType::Gateway {
            let parent = cursor
                .uplink_device_mac
                .as_ref()
                .and_then(|mac| fleet.devices.iter().find(|candidate| &candidate.mac == mac))
                .unwrap_or_else(|| panic!("{:?} has a dangling uplink", cursor.name));
            if let Some(port) = cursor.uplink_port_idx {
                assert!(
                    parent.ports.iter().any(|candidate| candidate.index == port),
                    "{:?} uplinks through a missing port",
                    cursor.name
                );
            }
            cursor = parent;
            hops += 1;
            assert!(
                hops < fleet.devices.len(),
                "uplink cycle at {:?}",
                device.name
            );
        }
    }
}

#[test]
fn history_is_sorted_and_spans_the_week() {
    let fleet = fleet();
    assert!(fleet.events.len() > 200);
    assert!(
        fleet
            .events
            .windows(2)
            .all(|pair| pair[0].timestamp <= pair[1].timestamp)
    );
    let oldest = fleet.events.first().expect("events").timestamp;
    assert!(anchor() - oldest > Duration::days(5));
}

#[test]
fn ticks_animate_stats_and_emit_events() {
    let mut sim = Simulation::new(fleet());
    let now = anchor();
    let first = sim.step(now);
    let second = sim.step(now + Duration::seconds(1));
    let wan = |delta: &super::sim::Delta| {
        delta.devices[GATEWAY]
            .stats
            .uplink_bandwidth
            .expect("gateway bandwidth")
            .rx_bytes_per_sec
    };
    assert_ne!(wan(&first), wan(&second), "WAN should move between ticks");
    assert!(first.health.is_some());

    let rx_before: u64 = first
        .clients
        .iter()
        .filter_map(|client| client.rx_bytes)
        .sum();
    let mut events = 0;
    let mut last = second;
    for tick in 2..40 {
        last = sim.step(now + Duration::seconds(tick));
        events += last.events.len();
    }
    assert!(
        events >= 3,
        "expected roams, churn, and alerts; got {events}"
    );
    let rx_after: u64 = last
        .clients
        .iter()
        .filter_map(|client| client.rx_bytes)
        .sum();
    assert!(rx_after > rx_before);
}

#[test]
fn same_seed_same_animation() {
    let run = || {
        let mut sim = Simulation::new(fleet());
        (1..=90)
            .map(|tick| {
                let delta = sim.step(anchor() + Duration::seconds(tick));
                (
                    sim.wan,
                    delta
                        .events
                        .iter()
                        .map(|event| event.message.clone())
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}

#[test]
fn launch_upgrade_finishes() {
    let mut sim = Simulation::new(fleet());
    let updating = |sim: &Simulation| {
        sim.fleet
            .devices
            .iter()
            .filter(|device| device.state == DeviceState::Updating)
            .count()
    };
    assert_eq!(updating(&sim), 1);
    for tick in 1..=80 {
        sim.step(anchor() + Duration::seconds(tick));
    }
    assert_eq!(updating(&sim), 0);
}

#[test]
fn commands_simulate_or_refuse() {
    let mut sim = Simulation::new(fleet());
    let now = anchor();
    let ap = sim.fleet.aps()[0].clone();

    let (_, delta) = sim
        .apply_command(Command::RestartDevice { id: ap.id.clone() }, now)
        .expect("restart works in demo mode");
    assert_eq!(delta.events.len(), 1);

    let stale = sim
        .fleet
        .devices
        .iter()
        .find(|device| device.firmware_updatable)
        .expect("stale AP")
        .mac
        .clone();
    sim.apply_command(
        Command::UpgradeDevice {
            mac: stale,
            firmware_url: None,
        },
        now,
    )
    .expect("upgrade works in demo mode");

    let error = sim
        .apply_command(
            Command::DeleteNetwork {
                id: sim.fleet.networks[1].id.clone(),
                force: false,
            },
            now,
        )
        .expect_err("CRUD is refused");
    assert!(matches!(error, CoreError::Unsupported { .. }));
    assert!(error.to_string().contains("demo mode"), "{error}");
}

#[test]
fn power_cycling_the_dark_camera_port_brings_it_back() {
    let mut sim = Simulation::new(fleet());
    let core = sim.fleet.devices[super::fleet::SWITCH_BASE].id.clone();
    let (camera, port) = super::catalog::OFFLINE_CAMERA;
    sim.apply_command(
        Command::PowerCyclePort {
            device_id: core,
            port_idx: port,
        },
        anchor(),
    )
    .expect("power cycle works");
    for tick in 1..=10 {
        sim.step(anchor() + Duration::seconds(tick));
    }
    assert!(
        sim.fleet
            .clients
            .iter()
            .any(|client| client.client.hostname.as_deref() == Some(camera))
    );
}

#[test]
fn queries_match_the_fleet() {
    let sim = Simulation::new(fleet());
    let now = anchor();
    let hour_ago = (now - Duration::hours(1)).timestamp_millis();
    let rows = sim.gateway_stats(
        "5minutes",
        Some(hour_ago),
        Some(now.timestamp_millis()),
        now,
    );
    assert_eq!(rows.len(), 12);
    let first_time = rows[0]["time"].as_i64().expect("time");
    assert!(
        first_time > 1_000_000_000_000,
        "report times are milliseconds"
    );
    assert!(
        rows.iter()
            .all(|row| row["wan-rx_bytes"].as_u64() > Some(0))
    );

    let days = sim.site_stats("daily", None, Some(now.timestamp_millis()), now);
    assert!(days.len() >= 29);

    let wireless = sim
        .fleet
        .clients
        .iter()
        .find(|client| client.mobile && client.client.client_type == ClientType::Wireless)
        .expect("mobile client");
    let ip = wireless.client.ip.expect("ip").to_string();
    let experience = sim.client_wifi_experience(&ip).expect("experience");
    assert!(experience["signal"].as_i64().is_some());
    assert_ne!(
        experience["nearest_neighbors"]
            .as_array()
            .map_or(0, Vec::len),
        0
    );

    let roams = sim
        .client_roams(wireless.client.mac.as_str(), Some(50))
        .expect("roams");
    assert_ne!(roams.len(), 0);

    let observers: HashSet<String> = sim
        .fleet
        .aps()
        .iter()
        .map(|ap| ap.mac.to_string())
        .collect();
    let neighbors = sim.rogue_aps(Some(300));
    assert_ne!(neighbors.len(), 0);
    assert!(neighbors.iter().all(|neighbor| {
        neighbor
            .ap_mac
            .as_ref()
            .is_some_and(|mac| observers.contains(mac))
    }));
    assert_ne!(sim.dpi_applications().len(), 0);
}

#[tokio::test]
async fn demo_controller_connects_offline_and_animates() {
    let controller = Controller::demo(DemoFleetOptions::with_seed(7));
    assert!(controller.is_demo());
    let state = controller.connection_state();
    let refused = controller
        .execute(Command::SpeedtestDevice)
        .await
        .expect_err("commands need a connection");
    assert!(matches!(refused, CoreError::ControllerDisconnected));

    controller.connect().await.expect("demo connect");
    assert_eq!(*state.borrow(), ConnectionState::Connected);
    assert_ne!(controller.devices_snapshot().len(), 0);
    assert_ne!(controller.clients_snapshot().len(), 0);
    assert_ne!(controller.events_snapshot().len(), 0);
    assert_ne!(controller.site_health().borrow().len(), 0);
    assert_ne!(controller.list_channels().await.expect("channels").len(), 0);

    let mut events = controller.events();
    let device = controller.devices_snapshot()[0].clone();
    controller
        .execute(Command::LocateDevice {
            mac: device.mac.clone(),
            enable: true,
        })
        .await
        .expect("locate");
    let event = events.recv().await.expect("command event");
    assert!(event.message.contains("Locate"));

    controller.disconnect().await;
    assert_eq!(*state.borrow(), ConnectionState::Disconnected);
}

#[tokio::test(start_paused = true)]
async fn store_follows_the_simulation() {
    let controller = Controller::demo(DemoFleetOptions::default());
    controller.connect().await.expect("demo connect");
    let updating = |controller: &Controller| {
        controller
            .devices_snapshot()
            .iter()
            .filter(|device| device.state == DeviceState::Updating)
            .count()
    };
    assert_eq!(updating(&controller), 1);
    for _ in 0..90 {
        tokio::time::advance(std::time::Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
    }
    assert_eq!(
        updating(&controller),
        0,
        "launch upgrade should land in the store"
    );
    controller.disconnect().await;
}

#[test]
fn queries_survive_every_ap_going_dark() {
    let mut sim = Simulation::new(fleet());
    for device in &mut sim.fleet.devices {
        if device.device_type == DeviceType::AccessPoint {
            device.state = DeviceState::Offline;
        }
    }
    let mac = sim
        .fleet
        .clients
        .iter()
        .find(|client| client.client.client_type == ClientType::Wireless)
        .expect("wireless client")
        .client
        .mac
        .to_string();
    let roams = sim.client_roams(&mac, Some(20)).expect("roam history");
    assert_ne!(roams.len(), 0, "history comes from the AP inventory");
    for tick in 1..=30 {
        sim.step(anchor() + Duration::seconds(tick));
    }
}

#[test]
fn stats_ranges_are_clamped_and_bounded() {
    let sim = Simulation::new(fleet());
    let now = anchor();
    let started = std::time::Instant::now();
    for (start, end) in [
        (Some(i64::MIN), Some(i64::MAX)),
        (Some(i64::MAX), Some(i64::MIN)),
        (Some(i64::MIN), None),
        (None, Some(i64::MIN)),
        (Some(0), Some(0)),
    ] {
        for interval in ["5minutes", "hourly", "daily", "monthly"] {
            let rows = sim.gateway_stats(interval, start, end, now);
            assert!(rows.len() <= 2_000, "{interval}: {} rows", rows.len());
            sim.site_stats(interval, start, end, now);
        }
    }
    assert!(started.elapsed() < std::time::Duration::from_secs(5));

    let newest = sim.gateway_stats("5minutes", Some(i64::MIN), Some(i64::MAX), now);
    let last = newest
        .last()
        .and_then(|row| row["time"].as_i64())
        .expect("rows");
    assert!(
        now.timestamp_millis() - last <= 600_000,
        "keeps the newest buckets"
    );
}

#[test]
fn kicks_follow_unifi_wired_versus_wireless() {
    let mut sim = Simulation::new(fleet());
    let now = anchor();
    let wired = sim
        .fleet
        .clients
        .iter()
        .find(|client| client.client.client_type == ClientType::Wired)
        .expect("wired client")
        .client
        .mac
        .clone();
    let error = sim
        .apply_command(Command::KickClient { mac: wired.clone() }, now)
        .expect_err("wired clients cannot be kicked");
    assert!(matches!(error, CoreError::Rejected { .. }), "{error}");
    let still = sim.client_index_by_mac(&wired).expect("wired client stays");
    assert_eq!(
        sim.fleet.clients[still].client.client_type,
        ClientType::Wired
    );

    let wireless = sim
        .fleet
        .clients
        .iter()
        .find(|client| client.mobile && client.client.client_type == ClientType::Wireless)
        .expect("wireless client")
        .client
        .mac
        .clone();
    sim.apply_command(
        Command::KickClient {
            mac: wireless.clone(),
        },
        now,
    )
    .expect("wireless kick");
    assert!(sim.client_index_by_mac(&wireless).is_none());
    for tick in 1..=10 {
        sim.step(now + Duration::seconds(tick));
    }
    let back = sim
        .client_index_by_mac(&wireless)
        .expect("kicked client rejoins");
    let client = &sim.fleet.clients[back].client;
    assert!(client.wireless.is_some() && client.switch_port.is_none());
}

#[test]
fn history_series_do_not_depend_on_launch_time() {
    let early = anchor();
    let late = early + Duration::days(3) + Duration::minutes(17) + Duration::seconds(9);
    let series = |launch: DateTime<Utc>| {
        let sim = Simulation::new(Fleet::generate(DEFAULT_DEMO_SEED, launch));
        let now = launch + Duration::seconds(45);
        let mut rows = Vec::new();
        for interval in ["5minutes", "hourly", "daily"] {
            rows.extend(sim.gateway_stats(interval, None, None, now));
            rows.extend(sim.site_stats(interval, None, None, now));
        }
        let times: Vec<i64> = rows.iter().filter_map(|row| row["time"].as_i64()).collect();
        for row in &mut rows {
            if let Some(object) = row.as_object_mut() {
                object.remove("time");
            }
        }
        (rows, times)
    };
    let (early_rows, early_times) = series(early);
    let (late_rows, late_times) = series(late);
    assert_ne!(early_rows.len(), 0);
    assert_eq!(early_rows, late_rows, "same seed, same chart shapes");
    assert_ne!(early_times, late_times, "timestamps still track the clock");
}
