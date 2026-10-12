//! Labels the Clients table resolves against the other entity lists: the
//! network a client sits on and the device it connects through.

use std::sync::Arc;

use unifly_api::{Client, ClientType, Device, Network};

/// The client's network by name: its `network_id` first, then a network
/// with the client's VLAN, then the bare VLAN number. `None` when the
/// client carries neither.
pub(super) fn network_label(client: &Client, networks: &[Arc<Network>]) -> Option<String> {
    let by_id = client
        .network_id
        .as_ref()
        .and_then(|id| networks.iter().find(|network| &network.id == id));
    let by_vlan = || {
        client.vlan.and_then(|vlan| {
            networks
                .iter()
                .find(|network| network.vlan_id == Some(vlan))
        })
    };
    by_id
        .or_else(by_vlan)
        .map(|network| network.name.clone())
        .or_else(|| client.vlan.map(|vlan| format!("VLAN {vlan}")))
}

/// Where the client connects: the access point's name for a wireless
/// client, the switch's name and port for a wired one. Falls back to the
/// uplink MAC when that device is not in `devices`, and to `None` when the
/// client reports no uplink at all.
pub(super) fn uplink_label(client: &Client, devices: &[Arc<Device>]) -> Option<String> {
    let bssid = client
        .wireless
        .as_ref()
        .and_then(|wireless| wireless.bssid.as_ref());
    let device = devices.iter().find(|device| {
        client.uplink_device_mac.as_ref() == Some(&device.mac)
            || client.uplink_device_id.as_ref() == Some(&device.id)
            || bssid == Some(&device.mac)
    });

    let name = match device {
        Some(device) => device
            .name
            .clone()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| device.mac.to_string()),
        None => client.uplink_device_mac.as_ref()?.to_string(),
    };

    match (client.client_type, client.switch_port) {
        (ClientType::Wired, Some(port)) => Some(format!("{name} port {port}")),
        _ => Some(name),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};
    use unifly_api::{Client, Device, Network};

    use super::{network_label, uplink_label};

    const SWITCH_MAC: &str = "9e:b8:1f:d8:fc:f2";
    const AP_MAC: &str = "16:b0:16:bd:a0:84";

    fn device(name: Option<&str>, mac: &str, device_type: &str) -> Arc<Device> {
        Arc::new(
            serde_json::from_value(json!({
                "id": format!("dev-{mac}"),
                "mac": mac,
                "ip": null,
                "wan_ipv6": null,
                "name": name,
                "model": null,
                "device_type": device_type,
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
                "has_switching": device_type == "Switch",
                "has_access_point": device_type == "AccessPoint",
                "stats": {},
                "client_count": null,
                "origin": null
            }))
            .expect("device fixture should deserialize"),
        )
    }

    fn fleet() -> Vec<Arc<Device>> {
        vec![
            device(Some("sw-core"), SWITCH_MAC, "Switch"),
            device(Some("ap-lounge"), AP_MAC, "AccessPoint"),
        ]
    }

    fn client(fields: &Value) -> Client {
        let mut base = json!({
            "id": "client-1",
            "mac": "aa:00:00:00:00:01",
            "ip": null,
            "name": "laptop",
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
        if let (Some(base), Some(fields)) = (base.as_object_mut(), fields.as_object()) {
            base.extend(fields.clone());
        }
        serde_json::from_value(base).expect("client fixture should deserialize")
    }

    fn network(id: &str, name: &str, vlan: u16) -> Arc<Network> {
        Arc::new(
            serde_json::from_value(json!({
                "id": id,
                "name": name,
                "vlan_id": vlan,
                "enabled": true,
                "management": "Gateway",
                "purpose": "Corporate",
                "is_default": false,
                "subnet": null,
                "gateway_ip": null,
                "isolation_enabled": false,
                "internet_access_enabled": true,
                "mdns_forwarding_enabled": false,
                "ipv6_enabled": false,
                "ipv6_mode": null,
                "ipv6_prefix": null,
                "dhcpv6_enabled": false,
                "slaac_enabled": false,
                "ntp_server": null,
                "pxe_enabled": false,
                "tftp_server": null,
                "firewall_zone_id": null,
                "cellular_backup_enabled": false,
                "origin": null,
                "dhcp": null
            }))
            .expect("network fixture should deserialize"),
        )
    }

    #[test]
    fn wired_clients_show_switch_and_port() {
        let wired = client(&json!({ "uplink_device_mac": SWITCH_MAC, "switch_port": 14 }));
        assert_eq!(
            uplink_label(&wired, &fleet()).as_deref(),
            Some("sw-core port 14")
        );
    }

    #[test]
    fn wireless_clients_show_the_access_point() {
        let by_uplink = client(&json!({
            "client_type": "Wireless",
            "uplink_device_mac": AP_MAC,
        }));
        assert_eq!(
            uplink_label(&by_uplink, &fleet()).as_deref(),
            Some("ap-lounge")
        );

        let by_bssid = client(&json!({
            "client_type": "Wireless",
            "wireless": {
                "ssid": "Studio", "bssid": AP_MAC, "channel": 36, "frequency_ghz": 5.0,
                "signal_dbm": -55, "noise_dbm": null, "satisfaction": 98,
                "tx_rate_kbps": null, "rx_rate_kbps": null
            },
        }));
        assert_eq!(
            uplink_label(&by_bssid, &fleet()).as_deref(),
            Some("ap-lounge")
        );
    }

    #[test]
    fn unknown_uplinks_fall_back_to_the_mac_then_to_none() {
        let foreign =
            client(&json!({ "uplink_device_mac": "02:00:00:00:00:99", "switch_port": 3 }));
        assert_eq!(
            uplink_label(&foreign, &fleet()).as_deref(),
            Some("02:00:00:00:00:99 port 3")
        );
        assert_eq!(uplink_label(&client(&json!({})), &fleet()), None);

        let unnamed = vec![device(None, SWITCH_MAC, "Switch")];
        let wired = client(&json!({ "uplink_device_mac": SWITCH_MAC, "switch_port": 2 }));
        assert_eq!(
            uplink_label(&wired, &unnamed).as_deref(),
            Some("9e:b8:1f:d8:fc:f2 port 2")
        );
    }

    #[test]
    fn network_resolves_by_id_then_vlan_then_bare_vlan() {
        let networks = vec![
            network("11111111-1111-4111-8111-111111111111", "Studio", 10),
            network("22222222-2222-4222-8222-222222222222", "IoT", 40),
        ];
        let by_id = client(&json!({
            "network_id": "22222222-2222-4222-8222-222222222222",
            "vlan": 10,
        }));
        assert_eq!(network_label(&by_id, &networks).as_deref(), Some("IoT"));

        let by_vlan = client(&json!({ "vlan": 10 }));
        assert_eq!(
            network_label(&by_vlan, &networks).as_deref(),
            Some("Studio")
        );

        let unknown_vlan = client(&json!({ "vlan": 99 }));
        assert_eq!(
            network_label(&unknown_vlan, &networks).as_deref(),
            Some("VLAN 99")
        );
        assert_eq!(network_label(&client(&json!({})), &networks), None);
    }
}
