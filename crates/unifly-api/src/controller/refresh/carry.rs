use std::sync::Arc;

use crate::model::{Client, Device, MacAddress};

/// Carry last known session-merged client fields from the stored record onto
/// a fresh Integration client when the session fetch that supplies them
/// failed this cycle, so a transient error does not blank them.
///
/// The carried fields are exactly the ones `merge` fills from the session:
/// hostname, network name, traffic counters, wireless info, uplink MAC, and
/// switch port from the session clients fetch, plus DHCP reservation data from the
/// session users fetch. Integration's client conversion never sets them,
/// and a field the fresh record does set is never overwritten. Link fields
/// (wireless info, uplink MAC, switch port) are carried only while the
/// Integration connection type is unchanged, because a client that moved
/// between wired and wireless no longer has the old uplink.
pub(super) fn session_client_fields(
    clients: &mut [Client],
    session_clients_failed: bool,
    session_users_failed: bool,
    stored: impl Fn(&MacAddress) -> Option<Arc<Client>>,
) {
    if !session_clients_failed && !session_users_failed {
        return;
    }
    for client in clients {
        let Some(previous) = stored(&client.mac) else {
            continue;
        };
        if session_clients_failed {
            fill(&mut client.hostname, previous.hostname.as_ref());
            fill(&mut client.network_name, previous.network_name.as_ref());
            fill(&mut client.tx_bytes, previous.tx_bytes.as_ref());
            fill(&mut client.rx_bytes, previous.rx_bytes.as_ref());
            if previous.client_type == client.client_type {
                fill(&mut client.wireless, previous.wireless.as_ref());
                fill(
                    &mut client.uplink_device_mac,
                    previous.uplink_device_mac.as_ref(),
                );
                fill(&mut client.switch_port, previous.switch_port.as_ref());
            }
        }
        if session_users_failed && !client.use_fixedip && client.fixed_ip.is_none() {
            client.use_fixedip = previous.use_fixedip;
            client.fixed_ip = previous.fixed_ip;
        }
    }
}

/// Carry last known session-merged device fields from the stored record onto
/// a fresh Integration device when the session devices fetch failed.
///
/// Mirrors `merge::merge_session_devices`: client count, WAN IPv6, uplink
/// MAC, and uplink port fill only when the fresh record left them unset,
/// and ports and radios only when Integration reported none, so
/// Integration-sourced values always win.
pub(super) fn session_device_fields(
    devices: &mut [Device],
    stored: impl Fn(&MacAddress) -> Option<Arc<Device>>,
) {
    for device in devices {
        let Some(previous) = stored(&device.mac) else {
            continue;
        };
        fill(&mut device.client_count, previous.client_count.as_ref());
        fill(&mut device.wan_ipv6, previous.wan_ipv6.as_ref());
        fill(
            &mut device.uplink_device_mac,
            previous.uplink_device_mac.as_ref(),
        );
        fill(
            &mut device.uplink_port_idx,
            previous.uplink_port_idx.as_ref(),
        );
        fill(&mut device.uplink_medium, previous.uplink_medium.as_ref());
        if device.ports.is_empty() {
            device.ports.clone_from(&previous.ports);
        }
        if device.radios.is_empty() {
            device.radios.clone_from(&previous.radios);
        }
    }
}

/// Fill `slot` from `previous` only when the fresh record left it unset.
fn fill<T: Clone>(slot: &mut Option<T>, previous: Option<&T>) {
    if slot.is_none() {
        *slot = previous.cloned();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::{session_client_fields, session_device_fields};
    use crate::integration_types::{ClientResponse, DeviceResponse};
    use crate::model::{Client, ClientType, Device, MacAddress};
    use crate::session::models::SessionClientEntry;

    const CLIENT_MAC: &str = "aa:bb:cc:dd:ee:ff";

    fn integration_client(client_type: &str) -> Client {
        Client::from(
            serde_json::from_value::<ClientResponse>(json!({
                "id": "c56a4180-65aa-42ec-a945-5fd21dec0538",
                "name": "Laptop",
                "type": client_type,
                "ipAddress": "10.0.0.50",
                "connectedAt": null,
                "macAddress": CLIENT_MAC,
                "access": {},
            }))
            .unwrap(),
        )
    }

    /// A stored client as a successful session merge leaves it.
    fn stored_wireless_client() -> Arc<Client> {
        let session = Client::from(
            serde_json::from_value::<SessionClientEntry>(json!({
                "_id": "client-1",
                "mac": CLIENT_MAC,
                "ip": "10.0.0.50",
                "hostname": "laptop.lan",
                "network": "Studio",
                "is_wired": false,
                "essid": "home",
                "ap_mac": "11:22:33:44:55:66",
                "tx_bytes": 1234,
                "rx_bytes": 5678,
            }))
            .unwrap(),
        );
        let mut stored = integration_client("WIRELESS");
        stored.hostname = session.hostname;
        stored.network_name = session.network_name;
        stored.tx_bytes = session.tx_bytes;
        stored.rx_bytes = session.rx_bytes;
        stored.wireless = session.wireless;
        stored.uplink_device_mac = session.uplink_device_mac;
        stored.use_fixedip = true;
        stored.fixed_ip = Some("10.0.0.50".parse().unwrap());
        Arc::new(stored)
    }

    fn integration_device() -> Device {
        Device::from(
            serde_json::from_value::<DeviceResponse>(json!({
                "id": "9b2f6c1e-7a3d-4c5b-8e9f-0a1b2c3d4e5f",
                "macAddress": "aa:bb:cc:00:00:01",
                "ipAddress": "10.0.0.2",
                "name": "Core Switch",
                "model": "USW-24",
                "state": "ONLINE",
                "supported": true,
                "firmwareVersion": null,
                "firmwareUpdatable": false,
                "features": [],
                "interfaces": {},
            }))
            .unwrap(),
        )
    }

    #[test]
    fn failed_session_clients_fetch_carries_merged_fields() {
        let stored = stored_wireless_client();
        let mut clients = vec![integration_client("WIRELESS")];

        session_client_fields(&mut clients, true, false, |_| Some(Arc::clone(&stored)));

        let client = &clients[0];
        assert_eq!(client.hostname.as_deref(), Some("laptop.lan"));
        assert_eq!(client.network_name.as_deref(), Some("Studio"));
        assert_eq!(client.tx_bytes, Some(1234));
        assert_eq!(client.rx_bytes, Some(5678));
        assert_eq!(
            client.wireless.as_ref().and_then(|w| w.ssid.as_deref()),
            Some("home")
        );
        assert_eq!(
            client.uplink_device_mac,
            Some(MacAddress::new("11:22:33:44:55:66"))
        );
        assert!(
            !client.use_fixedip,
            "users fetch succeeded; nothing to carry"
        );
        assert_eq!(client.fixed_ip, None);
    }

    #[test]
    fn failed_session_users_fetch_carries_reservation() {
        let stored = stored_wireless_client();
        let mut clients = vec![integration_client("WIRELESS")];

        session_client_fields(&mut clients, false, true, |_| Some(Arc::clone(&stored)));

        let client = &clients[0];
        assert!(client.use_fixedip);
        assert_eq!(client.fixed_ip, Some("10.0.0.50".parse().unwrap()));
        assert_eq!(client.hostname, None, "clients fetch succeeded");
    }

    #[test]
    fn connection_type_change_drops_link_fields() {
        let stored = stored_wireless_client();
        let mut clients = vec![integration_client("WIRED")];

        session_client_fields(&mut clients, true, false, |_| Some(Arc::clone(&stored)));

        let client = &clients[0];
        assert_eq!(client.client_type, ClientType::Wired);
        assert_eq!(client.hostname.as_deref(), Some("laptop.lan"));
        assert!(client.wireless.is_none());
        assert_eq!(client.uplink_device_mac, None);
    }

    #[test]
    fn fresh_values_win_and_successful_fetches_carry_nothing() {
        let stored = stored_wireless_client();
        let mut clients = vec![integration_client("WIRELESS")];
        clients[0].hostname = Some("fresh.lan".into());

        session_client_fields(&mut clients, true, true, |_| Some(Arc::clone(&stored)));
        assert_eq!(clients[0].hostname.as_deref(), Some("fresh.lan"));

        let mut untouched = vec![integration_client("WIRELESS")];
        session_client_fields(&mut untouched, false, false, |_| Some(Arc::clone(&stored)));
        assert_eq!(untouched[0].hostname, None);
        assert_eq!(untouched[0].tx_bytes, None);
    }

    #[test]
    fn failed_session_devices_fetch_carries_merged_fields() {
        let mut stored = integration_device();
        stored.client_count = Some(7);
        stored.uplink_device_mac = Some(MacAddress::new("11:22:33:44:55:66"));
        stored.uplink_port_idx = Some(4);
        let stored = Arc::new(stored);

        let mut devices = vec![integration_device()];
        devices[0].client_count = Some(9);
        session_device_fields(&mut devices, |_| Some(Arc::clone(&stored)));

        let device = &devices[0];
        assert_eq!(device.client_count, Some(9), "fresh value wins");
        assert_eq!(
            device.uplink_device_mac,
            Some(MacAddress::new("11:22:33:44:55:66"))
        );
        assert_eq!(device.uplink_port_idx, Some(4));
    }
}
