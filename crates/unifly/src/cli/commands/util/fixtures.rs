//! Snapshot records for resolver tests, built from their JSON shape.

use std::sync::Arc;

use serde_json::json;
use unifly_api::{Client, Device};

pub fn from_json<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Arc<T> {
    Arc::new(serde_json::from_value(value).expect("fixture should deserialize"))
}

pub fn device(id: &str, name: Option<&str>, mac: &str, ip: Option<&str>) -> Arc<Device> {
    from_json(json!({
        "id": id,
        "mac": mac,
        "ip": ip,
        "wan_ipv6": null,
        "name": name,
        "model": null,
        "device_type": "AccessPoint",
        "state": "Online",
        "firmware_version": null,
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
        "stats": {},
        "client_count": null,
        "origin": null
    }))
}

pub fn client(name: &str, hostname: &str, mac: &str, ip: Option<&str>) -> Arc<Client> {
    from_json(json!({
        "id": "9d5cf91f-0513-4c50-b79c-b3350a023b95",
        "mac": mac,
        "ip": ip,
        "name": name,
        "hostname": hostname,
        "client_type": "Wireless",
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
    }))
}
