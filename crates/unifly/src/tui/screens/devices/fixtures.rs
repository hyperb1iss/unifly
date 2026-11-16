//! Entities for the Devices and Clients screen tests.

use std::sync::Arc;

use serde_json::{Value, json};
use unifly_api::{Client, Device, Network};

/// Lay `patch`'s keys over `base`.
fn patched(mut base: Value, patch: &Value) -> Value {
    if let (Some(base), Some(patch)) = (base.as_object_mut(), patch.as_object()) {
        for (key, value) in patch {
            base.insert(key.clone(), value.clone());
        }
    }
    base
}

/// An online access point named `name`, with `patch` laid over it. Its MAC
/// ends in `index`.
pub(in crate::tui) fn device(index: u8, name: &str, patch: &Value) -> Arc<Device> {
    let base = json!({
        "id": format!("device-{index}"),
        "mac": format!("aa:bb:cc:dd:ee:{index:02x}"),
        "ip": format!("10.40.0.{index}"),
        "wan_ipv6": null,
        "name": name,
        "model": "U7 Pro",
        "device_type": "AccessPoint",
        "state": "Online",
        "firmware_version": "8.1.113",
        "firmware_updatable": false,
        "adopted_at": null,
        "provisioned_at": null,
        "last_seen": null,
        "serial": null,
        "supported": true,
        "ports": [],
        "radios": [],
        "uplink_device_id": null,
        "uplink_device_mac": null,
        "has_switching": false,
        "has_access_point": true,
        "stats": { "uptime_secs": 3_542_400 },
        "client_count": null,
        "origin": null
    });
    Arc::new(serde_json::from_value(patched(base, patch)).expect("device fixture"))
}

/// A switch with `ports` copper ports. Odd ports are up at 1 Gbps; the
/// first four supply PoE, and ports five to eight can but are switched
/// off.
pub(in crate::tui) fn switch(index: u8, name: &str, ports: u32) -> Arc<Device> {
    let ports: Vec<Value> = (1..=ports)
        .map(|port| {
            let up = port % 2 == 1;
            let poe = match port {
                1..=4 => json!({ "standard": "802.3at", "enabled": true, "state": "Up" }),
                5..=8 => json!({ "standard": "802.3at", "enabled": false, "state": "Down" }),
                _ => Value::Null,
            };
            json!({
                "index": port,
                "name": format!("Port {port}"),
                "state": if up { "Up" } else { "Down" },
                "speed_mbps": if up { json!(1000) } else { Value::Null },
                "max_speed_mbps": 1000,
                "connector": "Rj45",
                "poe": poe
            })
        })
        .collect();
    device(
        index,
        name,
        &json!({
            "model": "USW Pro Max 24 PoE",
            "device_type": "Switch",
            "has_switching": true,
            "has_access_point": false,
            "ports": ports
        }),
    )
}

/// A wired client named `name`, with `patch` laid over it. Its MAC ends
/// in `index`.
pub(in crate::tui) fn client(index: u8, name: &str, patch: &Value) -> Arc<Client> {
    let base = json!({
        "id": format!("client-{index}"),
        "mac": format!("82:8b:02:66:7c:{index:02x}"),
        "ip": format!("10.40.20.{index}"),
        "name": name,
        "hostname": null,
        "client_type": "Wired",
        "connected_at": null,
        "uplink_device_id": null,
        "uplink_device_mac": null,
        "switch_port": null,
        "network_id": null,
        "vlan": null,
        "wireless": null,
        "guest_auth": null,
        "is_guest": false,
        "tx_bytes": null,
        "rx_bytes": null,
        "bandwidth": null,
        "os_name": null,
        "device_class": null,
        "use_fixedip": false,
        "fixed_ip": null,
        "blocked": false
    });
    Arc::new(serde_json::from_value(patched(base, patch)).expect("client fixture"))
}

/// The wireless part of a client fixture, at `signal` dBm on `bssid`.
pub(in crate::tui) fn wireless(signal: i32, bssid: &str) -> Value {
    json!({
        "client_type": "Wireless",
        "wireless": {
            "ssid": "Harbor", "bssid": bssid, "channel": 44, "frequency_ghz": 5.0,
            "signal_dbm": signal, "noise_dbm": -95, "satisfaction": 97,
            "tx_rate_kbps": 866_000, "rx_rate_kbps": 720_000
        }
    })
}

/// A network named `name` on `vlan`.
pub(in crate::tui) fn network(id: &str, name: &str, vlan: u16) -> Arc<Network> {
    Arc::new(
        serde_json::from_value(json!({
            "id": id, "name": name, "vlan_id": vlan,
            "enabled": true, "management": "Gateway", "purpose": "Corporate",
            "is_default": false, "subnet": null, "gateway_ip": null,
            "isolation_enabled": false, "internet_access_enabled": true,
            "mdns_forwarding_enabled": false, "ipv6_enabled": false, "ipv6_mode": null,
            "ipv6_prefix": null, "dhcpv6_enabled": false, "slaac_enabled": false,
            "ntp_server": null, "pxe_enabled": false, "tftp_server": null,
            "firewall_zone_id": null, "cellular_backup_enabled": false, "origin": null,
            "dhcp": null
        }))
        .expect("network fixture"),
    )
}
