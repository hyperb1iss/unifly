// ── Demo history: a week of past events and the neighboring RF scene ──

use chrono::{DateTime, Duration, Utc};

use super::{AP_BASE, Fleet, SWITCH_BASE, ssid_for_network};
use crate::demo::catalog::{
    self, AP_FIRMWARE, APS, HOSTILE_IPS, IPS_SIGNATURES, NEIGHBOR_SSIDS, OFFLINE_AP,
    STALE_FIRMWARE_AP, UPGRADING_AP,
};
use crate::demo::rng::{DemoRng, from_i64, to_i64};
use crate::model::common::DataSource;
use crate::model::{Client, ClientType, EntityId, Event, EventCategory, EventSeverity, MacAddress};
use crate::session::models::RogueAp;

/// Past events seeded into the store at connect.
const HISTORY_EVENTS: usize = 320;
const WEEK_SECS: u64 = 7 * 86_400;

pub(super) fn build(fleet: &mut Fleet, rng: &mut DemoRng) {
    let mut events = Vec::with_capacity(HISTORY_EVENTS + 4);
    while events.len() < HISTORY_EVENTS {
        let ago = rng.range(300, WEEK_SECS);
        let at = fleet.anchor - Duration::seconds(i64::try_from(ago).unwrap_or(600));
        if !rng.chance(activity(at, fleet.anchor)) {
            continue;
        }
        events.push(random_event(fleet, rng, at));
    }
    events.extend(notable_events(fleet, rng));
    events.sort_by_key(|event| event.timestamp);
    fleet.events = events;
    fleet.neighbors = neighbors(fleet, rng);
}

/// Studio clock at launch: mid-afternoon on a Thursday, whatever the wall
/// clock says, so the history reads the same whenever a demo is recorded.
const LAUNCH_HOUR: f64 = 14.5;
/// Day of week at launch, Monday = 0.
const LAUNCH_WEEKDAY: i64 = 3;

/// How busy the studio is at `at`: a working-hours hump that sags on
/// weekends, with a floor for overnight renders and automation. Hours are
/// measured on the studio clock relative to `anchor` (the launch time).
pub(crate) fn activity(at: DateTime<Utc>, anchor: DateTime<Utc>) -> f64 {
    let hours = from_i64((at - anchor).num_seconds()) / 3_600.0 + LAUNCH_HOUR;
    let hour = hours.rem_euclid(24.0);
    let day = to_i64(hours.div_euclid(24.0));
    let weekend = (LAUNCH_WEEKDAY + day).rem_euclid(7) >= 5;
    let workday = (-((hour - 14.0) / 4.2).powi(2)).exp();
    let scale = if weekend { 0.35 } else { 1.0 };
    (0.08 + 0.92 * workday * scale).clamp(0.0, 1.0)
}

pub(crate) fn event(
    rng: &mut DemoRng,
    fleet: &Fleet,
    at: DateTime<Utc>,
    (category, severity): (EventCategory, EventSeverity),
    key: &str,
    message: String,
) -> Event {
    Event {
        id: Some(EntityId::Legacy(rng.object_id())),
        timestamp: at,
        category,
        severity,
        event_type: key.into(),
        message,
        device_mac: None,
        client_mac: None,
        site_id: Some(fleet.site.id.clone()),
        raw_key: Some(key.into()),
        source: DataSource::SessionApi,
    }
}

fn device_name(fleet: &Fleet, index: usize) -> String {
    fleet.devices[index].name.clone().unwrap_or_default()
}

fn random_event(fleet: &Fleet, rng: &mut DemoRng, at: DateTime<Utc>) -> Event {
    let info = (EventCategory::Client, EventSeverity::Info);
    let roll = rng.unit();
    // Wi-Fi comings and goings dominate; wired links only flap now and then.
    let wireless = roll < 0.70;
    let pool: Vec<&Client> = fleet
        .clients
        .iter()
        .filter(|demo| {
            if wireless {
                demo.mobile && demo.client.client_type == ClientType::Wireless
            } else {
                demo.client.client_type == ClientType::Wired
            }
        })
        .map(|demo| &demo.client)
        .collect();
    let client = *rng.pick(&pool);
    let label = client
        .name
        .clone()
        .or_else(|| client.hostname.clone())
        .unwrap_or_else(|| client.mac.to_string());
    let online_aps: Vec<usize> = (AP_BASE..fleet.devices.len())
        .filter(|index| *index - AP_BASE != OFFLINE_AP)
        .collect();
    let ap = *rng.pick(&online_aps);
    let other_ap = *rng.pick(&online_aps);

    let mut event = if roll < 0.28 {
        let ssid = client
            .wireless
            .as_ref()
            .and_then(|wireless| wireless.ssid.clone())
            .unwrap_or_else(|| ssid_for_network("studio").into());
        let message = format!(
            "{label} connected to \"{ssid}\" on {}",
            device_name(fleet, ap)
        );
        event(rng, fleet, at, info, "EVT_WU_Connected", message)
    } else if roll < 0.50 || (roll < 0.70 && ap == other_ap) {
        let message = format!("{label} disconnected from {}", device_name(fleet, ap));
        event(rng, fleet, at, info, "EVT_WU_Disconnected", message)
    } else if roll < 0.70 {
        let message = format!(
            "{label} roamed from {} to {}",
            device_name(fleet, ap),
            device_name(fleet, other_ap)
        );
        event(rng, fleet, at, info, "EVT_WU_Roam", message)
    } else if roll < 0.76 {
        let switch = client
            .uplink_device_mac
            .as_ref()
            .and_then(|mac| fleet.devices.iter().find(|device| &device.mac == mac))
            .and_then(|device| device.name.clone())
            .unwrap_or_default();
        let port = client.switch_port.unwrap_or(1);
        let message = format!("{label} connected to {switch} port {port}");
        let mut event = event(rng, fleet, at, info, "EVT_LU_Connected", message);
        event.client_mac = Some(client.mac.clone());
        event.device_mac.clone_from(&client.uplink_device_mac);
        return event;
    } else if roll < 0.84 {
        return security_event(fleet, rng, at);
    } else if roll < 0.88 {
        let channel = fleet.devices[ap]
            .radios
            .get(1)
            .and_then(|radio| radio.channel)
            .unwrap_or(36);
        let message = format!(
            "{} moved its 5 GHz radio to channel {channel}",
            device_name(fleet, ap)
        );
        let kind = (EventCategory::Device, EventSeverity::Info);
        let mut event = event(rng, fleet, at, kind, "EVT_AP_ChannelChanged", message);
        event.device_mac = Some(fleet.devices[ap].mac.clone());
        return event;
    } else if roll < 0.91 {
        let host = rng.range(50, 250);
        let message = format!("Admin crew-lead@neonharbor.example logged in from 10.40.10.{host}");
        let kind = (EventCategory::Admin, EventSeverity::Info);
        return event(rng, fleet, at, kind, "EVT_AD_Login", message);
    } else if roll < 0.95 {
        let peer = rng.range(1, 4);
        let message = format!("WireGuard peer crew-laptop-0{peer} connected to Crew VPN");
        let kind = (EventCategory::Vpn, EventSeverity::Info);
        return event(rng, fleet, at, kind, "EVT_VPN_PeerConnected", message);
    } else {
        let latency = rng.range(38, 95);
        let message = format!("WAN latency rose to {latency} ms on {}", catalog::ISP_NAME);
        let kind = (EventCategory::Network, EventSeverity::Warning);
        return event(rng, fleet, at, kind, "EVT_GW_WANLatency", message);
    };
    event.client_mac = Some(client.mac.clone());
    event.device_mac = Some(fleet.devices[ap].mac.clone());
    event
}

/// An IPS alert or firewall drop from a documentation-range address.
pub(crate) fn security_event(fleet: &Fleet, rng: &mut DemoRng, at: DateTime<Utc>) -> Event {
    let source = *rng.pick(&HOSTILE_IPS);
    if rng.chance(0.55) {
        let signature = *rng.pick(&IPS_SIGNATURES);
        let message = format!("IPS alert: {signature} from {source}");
        let kind = (EventCategory::Firewall, EventSeverity::Warning);
        event(rng, fleet, at, kind, "EVT_IPS_IpsAlert", message)
    } else {
        let port = *rng.pick(&[22u16, 22, 23, 445, 3389, 5900, 8291]);
        let rule = if port == 22 {
            "Drop inbound SSH"
        } else {
            "Block Invalid Traffic"
        };
        let message = format!(
            "Firewall dropped {source} -> {}:{port}/tcp ({rule})",
            catalog::WAN_IP
        );
        let kind = (EventCategory::Firewall, EventSeverity::Info);
        event(rng, fleet, at, kind, "EVT_GW_FirewallBlock", message)
    }
}

/// The handful of events that explain the fleet's interesting states.
fn notable_events(fleet: &Fleet, rng: &mut DemoRng) -> Vec<Event> {
    let anchor = fleet.anchor;
    let offline = AP_BASE + OFFLINE_AP;
    let upgrading = AP_BASE + UPGRADING_AP;
    let stale = AP_BASE + STALE_FIRMWARE_AP;
    let (camera, port) = catalog::OFFLINE_CAMERA;

    let mut lost = event(
        rng,
        fleet,
        anchor - Duration::minutes(192),
        (EventCategory::Device, EventSeverity::Error),
        "EVT_AP_Lost_Contact",
        format!(
            "{} lost contact with the gateway",
            device_name(fleet, offline)
        ),
    );
    lost.device_mac = Some(fleet.devices[offline].mac.clone());

    let mut port_down = event(
        rng,
        fleet,
        anchor - Duration::minutes(87),
        (EventCategory::Device, EventSeverity::Warning),
        "EVT_SW_PoeDisconnect",
        format!(
            "{camera} went dark: {} port {port} link down",
            device_name(fleet, SWITCH_BASE)
        ),
    );
    port_down.device_mac = Some(fleet.devices[SWITCH_BASE].mac.clone());

    let mut firmware = event(
        rng,
        fleet,
        anchor - Duration::minutes(41),
        (EventCategory::Device, EventSeverity::Info),
        "EVT_AP_FirmwareAvailable",
        format!(
            "Firmware {AP_FIRMWARE} is available for {}",
            device_name(fleet, stale)
        ),
    );
    firmware.device_mac = Some(fleet.devices[stale].mac.clone());

    let mut upgrade = event(
        rng,
        fleet,
        anchor - Duration::seconds(50),
        (EventCategory::Device, EventSeverity::Info),
        "EVT_AP_Upgrade_Scheduled",
        format!(
            "{} is upgrading to {AP_FIRMWARE}",
            device_name(fleet, upgrading)
        ),
    );
    upgrade.device_mac = Some(fleet.devices[upgrading].mac.clone());

    vec![lost, port_down, firmware, upgrade]
}

/// Neighboring networks heard by our APs, plus one rogue lookalike.
fn neighbors(fleet: &Fleet, rng: &mut DemoRng) -> Vec<RogueAp> {
    let observers: Vec<MacAddress> = APS
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != OFFLINE_AP && *index != UPGRADING_AP)
        .map(|(index, _)| fleet.devices[AP_BASE + index].mac.clone())
        .collect();

    let mut neighbors = Vec::new();
    for ssid in NEIGHBOR_SSIDS {
        let radios = rng.range(1, 3);
        for radio_index in 0..radios {
            let radio = match radio_index {
                0 => "ng",
                1 => "na",
                _ => "6e",
            };
            let observer = rng.pick(&observers).clone();
            let signal = -i32::try_from(rng.range(58, 90)).unwrap_or(80);
            neighbors.push(neighbor(rng, ssid, radio, signal, &observer, false));
        }
    }

    let lounge = fleet.devices[AP_BASE + 5].mac.clone();
    let mut rogue = neighbor(rng, "NeonHarbor-Free", "ng", -49, &lounge, true);
    rogue.security = Some("OPEN".into());
    neighbors.push(rogue);
    neighbors
}

fn neighbor(
    rng: &mut DemoRng,
    ssid: &str,
    radio: &str,
    signal: i32,
    observer: &MacAddress,
    rogue: bool,
) -> RogueAp {
    let (channel, freq) = match radio {
        "na" => {
            let channel = *rng.pick(&[36, 40, 44, 48, 52, 100, 149, 153, 157, 161]);
            (channel, 5_000 + 5 * channel)
        }
        "6e" => {
            let channel = *rng.pick(&[5, 21, 37, 53, 85, 117]);
            (channel, 5_950 + 5 * channel)
        }
        _ => {
            let channel = *rng.pick(&[1, 1, 3, 6, 6, 11, 11]);
            (channel, 2_407 + 5 * channel)
        }
    };
    let security = *rng.pick(&["WPA2", "WPA2", "WPA3", "WPA2/WPA3", "OPEN"]);
    let age = i64::try_from(rng.range(8, 280)).unwrap_or(60);
    RogueAp {
        bssid: rng.mac(),
        essid: Some(ssid.into()),
        channel: Some(channel),
        freq: Some(freq),
        signal: Some(signal),
        rssi: Some(signal + 96),
        noise: Some(-95),
        security: Some(security.into()),
        radio: Some(radio.into()),
        age: Some(age),
        is_rogue: rogue,
        ap_mac: Some(observer.to_string()),
        extra: serde_json::Map::new(),
    }
}
