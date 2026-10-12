use chrono::{TimeZone, Utc};

use super::*;
use crate::Controller;
use crate::command::Command;
use crate::controller::ports::{export_ports, port_profiles};
use crate::core_error::CoreError;
use crate::demo::{DEFAULT_DEMO_SEED, DemoFleetOptions};
use crate::model::{DeviceState, PoeMode, PortMode, PortProfile, PortSpeedSetting, StpState};

fn sim(seed: u64) -> Simulation {
    let anchor = Utc
        .with_ymd_and_hms(2026, 3, 18, 15, 30, 0)
        .single()
        .expect("valid anchor");
    Simulation::new(Fleet::generate(seed, anchor))
}

fn device_index(sim: &Simulation, name: &str) -> usize {
    sim.fleet
        .devices
        .iter()
        .position(|device| device.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no device named {name}"))
}

fn profiles_at(sim: &Simulation, index: usize) -> Vec<PortProfile> {
    let mac = &sim.fleet.devices[index].mac;
    let device = sim.session_device(mac).expect("device in fleet");
    port_profiles(&device, &sim.network_conf())
}

fn profiles(sim: &Simulation, name: &str) -> Vec<PortProfile> {
    profiles_at(sim, device_index(sim, name))
}

fn port(profiles: &[PortProfile], index: u32) -> &PortProfile {
    profiles
        .iter()
        .find(|profile| profile.index == index)
        .unwrap_or_else(|| panic!("no port {index}"))
}

#[test]
fn every_device_port_has_a_profile() {
    let sim = sim(DEFAULT_DEMO_SEED);
    for (index, device) in sim.fleet.devices.iter().enumerate() {
        let indices: Vec<u32> = profiles_at(&sim, index)
            .iter()
            .map(|profile| profile.index)
            .collect();
        let expected: Vec<u32> = device.ports.iter().map(|port| port.index).collect();
        assert_eq!(indices, expected, "{:?}", device.name);
    }
}

#[test]
fn the_dark_camera_port_is_configured_but_down() {
    let sim = sim(DEFAULT_DEMO_SEED);
    let (camera, camera_port) = OFFLINE_CAMERA;
    let core = profiles(&sim, "sw-core");
    let dark = port(&core, camera_port);
    assert_eq!(dark.name.as_deref(), Some(camera));
    assert_eq!(dark.link_state, PortState::Down);
    assert_eq!(dark.mode, PortMode::Access);
    assert_eq!(dark.native_network_name.as_deref(), Some("Cameras"));
    assert_eq!(dark.native_vlan_id, Some(50));
    assert_eq!(dark.poe_mode, Some(PoeMode::Auto));
    assert_eq!(dark.link_speed_mbps, None);
    assert_eq!(dark.stp_state, StpState::Disabled);
    assert!(
        sim.fleet
            .clients
            .iter()
            .all(|client| client.client.switch_port != Some(camera_port)
                || client.client.uplink_device_mac.as_ref()
                    != Some(&sim.fleet.devices[SWITCH_BASE].mac)),
        "nothing is connected to the dark port"
    );
}

#[test]
fn uplink_ports_match_each_devices_uplink() {
    let sim = sim(DEFAULT_DEMO_SEED);
    let mut checked = 0;
    for (index, device) in sim.fleet.devices.iter().enumerate() {
        let (Some(parent_mac), Some(parent_port)) =
            (device.uplink_device_mac.as_ref(), device.uplink_port_idx)
        else {
            continue;
        };
        let parent = sim
            .device_index_by_mac(parent_mac)
            .expect("parent in fleet");
        let parent_profiles = profiles_at(&sim, parent);
        let link = port(&parent_profiles, parent_port);
        assert_eq!(link.name, device.name, "port {parent_port}");
        assert_eq!(link.mode, PortMode::Trunk, "{:?}", device.name);
        assert_eq!(link.native_vlan_id, Some(1));
        let lit = device.state != DeviceState::Offline;
        assert_eq!(link.link_state == PortState::Up, lit, "{:?}", device.name);
        if device.device_type == DeviceType::AccessPoint {
            assert!(link.tagged_all, "{:?} carries every SSID", device.name);
            assert_eq!(link.poe_mode, Some(PoeMode::Auto), "{:?}", device.name);
        }

        let own = profiles_at(&sim, index);
        let uplink = own
            .iter()
            .find(|profile| profile.name.as_deref() == Some(UPLINK))
            .unwrap_or_else(|| panic!("{:?} has no uplink port", device.name));
        assert_eq!(uplink.mode, PortMode::Trunk);
        assert!(uplink.tagged_all);
        checked += 1;
    }
    // Gateway to core, two switch links, eight wired APs.
    assert_eq!(checked, 11);
}

#[test]
fn trunks_to_downstream_switches_tag_only_what_sits_behind_them() {
    let sim = sim(DEFAULT_DEMO_SEED);
    let core = profiles(&sim, "sw-core");
    let tagged = |name: &str| {
        core.iter()
            .find(|profile| profile.name.as_deref() == Some(name))
            .map(|profile| (profile.tagged_all, profile.tagged_vlan_ids.clone()))
    };
    assert_eq!(tagged("sw-render"), Some((false, vec![10, 20, 30, 40])));
    assert_eq!(tagged("sw-lounge"), Some((false, vec![10, 40])));
    assert_eq!(tagged(UPLINK), Some((true, Vec::new())));

    let gateway = profiles(&sim, "gw-harbor");
    let to_core = gateway
        .iter()
        .find(|profile| profile.name.as_deref() == Some("sw-core"))
        .expect("gateway port to the core");
    assert!(to_core.tagged_all);
    assert_eq!(to_core.link_speed_mbps, Some(10_000));
}

#[test]
fn wired_clients_sit_on_access_ports_for_their_network() {
    let sim = sim(DEFAULT_DEMO_SEED);
    let mut checked = 0;
    for client in sim.fleet.clients.iter().map(|client| &client.client) {
        let (Some(switch_mac), Some(switch_port)) =
            (client.uplink_device_mac.as_ref(), client.switch_port)
        else {
            continue;
        };
        let switch = sim.device_index_by_mac(switch_mac).expect("switch");
        let switch_profiles = profiles_at(&sim, switch);
        let profile = port(&switch_profiles, switch_port);
        assert_eq!(profile.name, client.name, "port {switch_port}");
        assert_eq!(profile.mode, PortMode::Access, "{:?}", client.name);
        assert_eq!(profile.link_state, PortState::Up, "{:?}", client.name);
        assert_eq!(profile.native_vlan_id, client.vlan, "{:?}", client.name);
        assert_eq!(
            profile.native_network_id,
            client.network_id.as_ref().map(ToString::to_string),
            "{:?}",
            client.name
        );
        checked += 1;
    }
    assert_eq!(checked, WIRED.len());
}

#[test]
fn the_core_has_two_ports_at_100_mbps() {
    let sim = sim(DEFAULT_DEMO_SEED);
    let core = profiles(&sim, "sw-core");
    let slow: Vec<(Option<&str>, Option<PortSpeedSetting>)> = core
        .iter()
        .filter(|profile| profile.link_speed_mbps == Some(100))
        .map(|profile| (profile.name.as_deref(), profile.speed_setting))
        .collect();
    assert_eq!(
        slow,
        [
            (Some("printer-ops"), Some(PortSpeedSetting::Mbps100)),
            (Some("badge-front"), Some(PortSpeedSetting::Auto)),
        ]
    );
}

#[test]
fn render_switch_holds_the_render_floor() {
    let sim = sim(DEFAULT_DEMO_SEED);
    let render = profiles(&sim, "sw-render");
    for index in 1..=10 {
        let node = port(&render, index);
        assert_eq!(node.name, Some(format!("render-{index:02}")));
        assert_eq!(node.native_network_name.as_deref(), Some("Render"));
    }
    assert_eq!(port(&render, 11).name.as_deref(), Some("dailies-review"));
    let nas = port(&render, 17);
    assert_eq!(nas.name.as_deref(), Some("nas-vault"));
    assert_eq!(nas.link_speed_mbps, Some(10_000));
    assert_eq!(nas.poe_mode, None, "SFP+ ports have no PoE");
    assert_eq!(port(&render, 13).name.as_deref(), Some("ap-render"));
    assert_eq!(port(&render, 18).name.as_deref(), Some(UPLINK));
}

#[test]
fn gateway_wan_ports_carry_no_switch_profile() {
    let sim = sim(DEFAULT_DEMO_SEED);
    let gateway = profiles(&sim, "gw-harbor");
    let wans: Vec<(Option<&str>, PortState, PortMode, Option<u32>)> = gateway
        .iter()
        .filter(|profile| {
            profile
                .name
                .as_deref()
                .is_some_and(|n| n.starts_with("WAN"))
        })
        .map(|profile| {
            (
                profile.name.as_deref(),
                profile.link_state,
                profile.mode,
                profile.link_speed_mbps,
            )
        })
        .collect();
    assert_eq!(
        wans,
        [
            (Some("WAN"), PortState::Up, PortMode::Unknown, Some(2_500)),
            (Some("WAN2"), PortState::Down, PortMode::Unknown, None),
        ]
    );
}

#[test]
fn export_lists_configured_ports_and_all_adds_the_rest() {
    let sim = sim(DEFAULT_DEMO_SEED);
    let index = device_index(&sim, "sw-core");
    let device = sim
        .session_device(&sim.fleet.devices[index].mac)
        .expect("device");
    let core = port_profiles(&device, &sim.network_conf());

    let sparse = export_ports(&device, false);
    let configured: Vec<u32> = sparse.ports.iter().map(|entry| entry.index).collect();
    let unconfigured: Vec<u32> = core
        .iter()
        .map(|profile| profile.index)
        .filter(|index| !configured.contains(index))
        .collect();
    assert_eq!(unconfigured, [15, 16]);

    for entry in &sparse.ports {
        let profile = port(&core, entry.index);
        assert_eq!(entry.name, profile.name, "port {}", entry.index);
        let mode = match profile.mode {
            PortMode::Access => "access",
            PortMode::Trunk => "trunk",
            other => panic!("port {} is {other:?}", entry.index),
        };
        assert_eq!(entry.mode.as_deref(), Some(mode));
        assert_eq!(entry.native_network_id, profile.native_network_id);
        assert_eq!(
            entry.tagged_all.unwrap_or_default(),
            profile.tagged_all,
            "port {}",
            entry.index
        );
        if !profile.tagged_all && profile.mode == PortMode::Trunk {
            assert_eq!(
                entry.tagged_network_ids.as_ref(),
                Some(&profile.tagged_network_ids)
            );
        }
        let pinned = profile.name.as_deref() == Some("printer-ops");
        assert_eq!(entry.speed.as_deref(), pinned.then_some("100"));
    }

    let full = export_ports(&device, true);
    let every: Vec<u32> = full.ports.iter().map(|entry| entry.index).collect();
    let expected: Vec<u32> = core.iter().map(|profile| profile.index).collect();
    assert_eq!(every, expected);
}

#[test]
fn ports_follow_the_simulation() {
    let mut sim = sim(DEFAULT_DEMO_SEED);
    let (_, camera_port) = OFFLINE_CAMERA;
    let core = sim.fleet.devices[SWITCH_BASE].id.clone();
    sim.apply_command(
        Command::PowerCyclePort {
            device_id: core,
            port_idx: camera_port,
        },
        sim.fleet.anchor,
    )
    .expect("power cycle");
    let mut now = sim.fleet.anchor;
    for _ in 0..12 {
        now += chrono::Duration::seconds(1);
        sim.step(now);
    }
    let revived = profiles(&sim, "sw-core");
    let camera = port(&revived, camera_port);
    assert_eq!(camera.link_state, PortState::Up);
    assert_eq!(camera.link_speed_mbps, Some(1_000));
}

#[test]
fn ports_are_deterministic_per_seed() {
    let raw = |seed| {
        let sim = sim(seed);
        let index = device_index(&sim, "sw-core");
        (
            sim.session_device(&sim.fleet.devices[index].mac),
            sim.network_conf(),
        )
    };
    assert_eq!(raw(DEFAULT_DEMO_SEED), raw(DEFAULT_DEMO_SEED));

    // Another seed rolls new identities onto the same catalog layout.
    let layout = |seed| {
        profiles(&sim(seed), "sw-core")
            .into_iter()
            .map(|profile| {
                (
                    profile.index,
                    profile.name,
                    profile.mode,
                    profile.native_vlan_id,
                    profile.tagged_vlan_ids,
                    profile.link_state,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(layout(DEFAULT_DEMO_SEED), layout(7));
    assert_ne!(
        raw(DEFAULT_DEMO_SEED).1,
        raw(7).1,
        "network IDs follow the seed"
    );
}

#[tokio::test]
async fn demo_controller_answers_port_queries() {
    let controller = Controller::demo(DemoFleetOptions::default());
    controller.connect().await.expect("connect");
    let core = controller
        .devices_snapshot()
        .iter()
        .find(|device| device.name.as_deref() == Some("sw-core"))
        .map(|device| device.mac.clone())
        .expect("sw-core");
    let ports = controller.list_device_ports(&core).await.expect("ports");
    assert_eq!(ports.len(), 26);
    let export = controller
        .export_device_ports(&core, false)
        .await
        .expect("export");
    assert_eq!(export.ports.len(), 24);

    let stranger = MacAddress::new("02:00:00:00:00:99");
    assert!(matches!(
        controller.list_device_ports(&stranger).await,
        Err(CoreError::DeviceNotFound { .. })
    ));
    controller.disconnect().await;
}
