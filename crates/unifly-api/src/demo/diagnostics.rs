// ── Demo diagnostics: alarms, WAN inventory, controller info ──
//
// The first reads of a diagnosis: what is alarming, how the site reaches
// the internet, and which controller answers. Every answer derives from
// the fleet's existing problems (the offline AP, the dark camera port, the
// AP with firmware waiting, the gateway's unplugged second WAN), so the
// alarms agree with `events list` and `devices get`. Alarms whose problem
// the simulation has since cleared (a revived port, a finished upgrade)
// come back archived.

use std::net::IpAddr;

use chrono::Duration;

use super::catalog::{
    NAMESERVERS, NETWORK_VERSION, OFFLINE_AP, OFFLINE_CAMERA, STALE_FIRMWARE_AP, WAN_GATEWAY,
    WAN_IP,
};
use super::fleet::{AP_BASE, GATEWAY, SWITCH_BASE, lan_ip, uuid_from};
use super::rng::DemoRng;
use super::sim::Simulation;
use crate::model::{
    Alarm, Device, EntityId, EventCategory, EventSeverity, PortState, SystemInfo, WanInterface,
};

/// Generator salts for alarm and WAN IDs, apart from every fleet stream.
const ALARM_SALT: u64 = 0xA1A2;
const WAN_SALT: u64 = 0x3A11;
/// How long ago the second WAN lost link.
const WAN2_DOWN_HOURS: i64 = 26;

fn port_is_up(device: &Device, index: u32) -> bool {
    device
        .ports
        .iter()
        .any(|port| port.index == index && port.state == PortState::Up)
}

/// The gateway's WAN ports, in port order.
fn wan_ports(gateway: &Device) -> impl Iterator<Item = &crate::model::Port> {
    gateway.ports.iter().filter(|port| {
        port.name
            .as_deref()
            .is_some_and(|name| name.starts_with("WAN"))
    })
}

impl Simulation {
    /// `stat/alarm`: the fleet's standing problems, newest first.
    pub(crate) fn alarms(&self) -> Vec<Alarm> {
        let fleet = &self.fleet;
        let mut rng = DemoRng::new(fleet.seed).fork(ALARM_SALT);
        // Draw every ID up front so the set of live problems never shifts
        // another alarm's ID.
        let ids: [String; 4] = std::array::from_fn(|_| rng.object_id());
        let [lost_id, dark_id, firmware_id, wan_id] = ids;

        let offline = &fleet.devices[AP_BASE + OFFLINE_AP];
        let stale = &fleet.devices[AP_BASE + STALE_FIRMWARE_AP];
        let core = &fleet.devices[SWITCH_BASE];
        let gateway = &fleet.devices[GATEWAY];
        let (_, camera_port) = OFFLINE_CAMERA;

        // The first three mirror the notable history events, so an alarm
        // and its event share a timestamp and wording.
        let mut alarms: Vec<Alarm> = [
            ("EVT_AP_Lost_Contact", lost_id, offline.state.is_online()),
            (
                "EVT_SW_PoeDisconnect",
                dark_id,
                port_is_up(core, camera_port),
            ),
            (
                "EVT_AP_FirmwareAvailable",
                firmware_id,
                !stale.firmware_updatable,
            ),
        ]
        .into_iter()
        .filter_map(|(key, id, resolved)| {
            let event = fleet
                .events
                .iter()
                .find(|event| event.raw_key.as_deref() == Some(key))?;
            Some(Alarm {
                id: EntityId::Legacy(id),
                timestamp: event.timestamp,
                category: event.category,
                severity: event.severity,
                message: event.message.clone(),
                archived: resolved,
                device_mac: event.device_mac.clone(),
                site_id: Some(fleet.site.id.clone()),
            })
        })
        .collect();

        let gateway_name = gateway.name.as_deref().unwrap_or("gateway");
        alarms.extend(
            wan_ports(gateway)
                .filter(|port| port.name.as_deref() != Some("WAN"))
                .map(|port| Alarm {
                    id: EntityId::Legacy(wan_id.clone()),
                    timestamp: fleet.anchor - Duration::hours(WAN2_DOWN_HOURS),
                    category: EventCategory::Network,
                    severity: EventSeverity::Warning,
                    message: format!(
                        "{gateway_name} {} is down: no link on port {}",
                        port.name.as_deref().unwrap_or("WAN"),
                        port.index
                    ),
                    archived: port.state == PortState::Up,
                    device_mac: Some(gateway.mac.clone()),
                    site_id: Some(fleet.site.id.clone()),
                }),
        );

        alarms.sort_by_key(|alarm| std::cmp::Reverse(alarm.timestamp));
        alarms
    }

    /// Integration `wans`: one interface per gateway WAN port. A port with
    /// no link has no address, gateway, or resolvers.
    pub(crate) fn wans(&self) -> Vec<WanInterface> {
        let mut rng = DemoRng::new(self.fleet.seed).fork(WAN_SALT);
        let parse = |raw: &str| raw.parse::<IpAddr>().ok();
        wan_ports(&self.fleet.devices[GATEWAY])
            .map(|port| {
                let id = EntityId::Uuid(uuid_from(&mut rng));
                let up = port.state == PortState::Up;
                WanInterface {
                    id,
                    name: port.name.clone(),
                    ip: up.then(|| parse(WAN_IP)).flatten(),
                    gateway: up.then(|| parse(WAN_GATEWAY)).flatten(),
                    dns: if up {
                        NAMESERVERS.iter().filter_map(|raw| parse(raw)).collect()
                    } else {
                        Vec::new()
                    },
                }
            })
            .collect()
    }

    /// Controller identity: the Network application on the gateway console.
    pub(crate) fn system_info(&self) -> SystemInfo {
        let gateway = &self.fleet.devices[GATEWAY];
        SystemInfo {
            controller_name: Some("UniFi Network".into()),
            version: NETWORK_VERSION.into(),
            build: None,
            hostname: gateway.name.clone(),
            ip: Some(IpAddr::V4(lan_ip(0, 1))),
            uptime_secs: gateway.stats.uptime_secs,
            update_available: Some(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::command::Command;
    use crate::demo::DEFAULT_DEMO_SEED;
    use crate::demo::fleet::Fleet;

    fn sim(seed: u64) -> Simulation {
        let anchor = Utc
            .with_ymd_and_hms(2026, 3, 18, 15, 30, 0)
            .single()
            .expect("valid anchor");
        Simulation::new(Fleet::generate(seed, anchor))
    }

    #[test]
    fn alarms_name_the_fleets_standing_problems() {
        let sim = sim(DEFAULT_DEMO_SEED);
        let alarms = sim.alarms();
        let messages: Vec<&str> = alarms.iter().map(|alarm| alarm.message.as_str()).collect();
        assert_eq!(alarms.len(), 4, "{messages:?}");
        for needle in ["ap-dock", "sw-core port 12", "ap-roof", "WAN2"] {
            assert!(
                messages.iter().any(|message| message.contains(needle)),
                "no alarm mentions {needle}: {messages:?}"
            );
        }
        assert!(alarms.iter().all(|alarm| !alarm.archived));
        assert!(
            alarms
                .windows(2)
                .all(|pair| pair[0].timestamp >= pair[1].timestamp),
            "alarms list newest first"
        );
        let lost = alarms
            .iter()
            .find(|alarm| alarm.message.contains("ap-dock"))
            .expect("offline AP alarm");
        assert_eq!(lost.severity, EventSeverity::Error);
        assert_eq!(
            lost.device_mac.as_ref(),
            Some(&sim.fleet.devices[AP_BASE + OFFLINE_AP].mac)
        );
    }

    #[test]
    fn alarms_and_wans_are_deterministic_per_seed() {
        let ids = |seed| {
            let sim = sim(seed);
            let alarms: Vec<String> = sim
                .alarms()
                .iter()
                .map(|alarm| alarm.id.to_string())
                .collect();
            let wans: Vec<String> = sim.wans().iter().map(|wan| wan.id.to_string()).collect();
            (alarms, wans)
        };
        assert_eq!(ids(DEFAULT_DEMO_SEED), ids(DEFAULT_DEMO_SEED));
        assert_ne!(ids(DEFAULT_DEMO_SEED), ids(DEFAULT_DEMO_SEED + 1));
    }

    #[test]
    fn a_cleared_problem_archives_its_alarm() {
        let mut sim = sim(DEFAULT_DEMO_SEED);
        let mac = sim.fleet.devices[AP_BASE + STALE_FIRMWARE_AP].mac.clone();
        sim.apply_command(
            Command::UpgradeDevice {
                mac,
                firmware_url: None,
            },
            sim.fleet.anchor,
        )
        .expect("upgrade starts");
        let firmware = sim
            .alarms()
            .into_iter()
            .find(|alarm| alarm.message.contains("ap-roof"))
            .expect("firmware alarm stays listed");
        assert!(firmware.archived);
    }

    #[test]
    fn wans_follow_the_gateway_ports() {
        let wans = sim(DEFAULT_DEMO_SEED).wans();
        let names: Vec<_> = wans.iter().map(|wan| wan.name.as_deref()).collect();
        assert_eq!(names, [Some("WAN"), Some("WAN2")]);
        assert_eq!(wans[0].ip, WAN_IP.parse().ok());
        assert_eq!(wans[0].gateway, WAN_GATEWAY.parse().ok());
        assert_eq!(wans[0].dns.len(), NAMESERVERS.len());
        assert!(wans[1].ip.is_none() && wans[1].gateway.is_none());
        assert_eq!(wans[1].dns, Vec::<IpAddr>::new());
    }

    #[test]
    fn system_info_describes_the_gateway_console() {
        let sim = sim(DEFAULT_DEMO_SEED);
        let info = sim.system_info();
        assert_eq!(info.version, NETWORK_VERSION);
        assert_eq!(info.hostname.as_deref(), Some("gw-harbor"));
        assert_eq!(
            info.uptime_secs,
            sim.fleet.devices[GATEWAY].stats.uptime_secs
        );
    }
}
