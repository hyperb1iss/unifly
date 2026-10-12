use std::collections::HashMap;

use crate::model::{Client, Device, MacAddress};
use crate::session::models::{SessionClientEntry, SessionDevice, SessionUserEntry};

use super::super::support::parse_session_device_wan_ipv6;

pub(super) fn merge_session_clients(
    clients: &mut [Client],
    session_clients: &[SessionClientEntry],
    session_users: &[SessionUserEntry],
) {
    merge_client_traffic(clients, session_clients);
    merge_user_reservations(clients, session_clients, session_users);
}

pub(super) fn merge_session_devices(devices: &mut [Device], session_devices: &[SessionDevice]) {
    if session_devices.is_empty() {
        return;
    }

    let session_by_mac: HashMap<&str, &SessionDevice> = session_devices
        .iter()
        .map(|device| (device.mac.as_str(), device))
        .collect();
    for device in devices {
        if let Some(legacy_device) = session_by_mac.get(device.mac.as_str()) {
            if device.client_count.is_none() {
                device.client_count = legacy_device
                    .num_sta
                    .and_then(|count| count.try_into().ok());
            }
            if device.wan_ipv6.is_none() {
                device.wan_ipv6 = parse_session_device_wan_ipv6(
                    legacy_device
                        .wan1
                        .as_ref()
                        .and_then(|wan| wan.ipv6.as_ref()),
                    legacy_device.ipv6.as_ref(),
                );
            }
            if device.ports.is_empty()
                || device.radios.is_empty()
                || device.uplink_device_mac.is_none()
                || device.uplink_port_idx.is_none()
            {
                let session_dev: Device = Device::from((*legacy_device).clone());
                if device.ports.is_empty() && !session_dev.ports.is_empty() {
                    device.ports = session_dev.ports;
                }
                if device.radios.is_empty() && !session_dev.radios.is_empty() {
                    device.radios = session_dev.radios;
                }
                if device.uplink_device_mac.is_none() {
                    device.uplink_device_mac = session_dev.uplink_device_mac;
                }
                if device.uplink_port_idx.is_none() {
                    device.uplink_port_idx = session_dev.uplink_port_idx;
                }
            }
        }
    }
}

fn merge_client_traffic(clients: &mut [Client], session_clients: &[SessionClientEntry]) {
    if session_clients.is_empty() {
        return;
    }

    let session_by_ip: HashMap<&str, &SessionClientEntry> = session_clients
        .iter()
        .filter_map(|client| client.ip.as_deref().map(|ip| (ip, client)))
        .collect();
    let mut merged = 0u32;
    for client in clients.iter_mut() {
        let ip_key = client.ip.map(|ip| ip.to_string());
        if let Some(session_client) = ip_key.as_deref().and_then(|ip| session_by_ip.get(ip)) {
            if client.tx_bytes.is_none() {
                client.tx_bytes = session_client
                    .tx_bytes
                    .and_then(|bytes| u64::try_from(bytes).ok());
            }
            if client.rx_bytes.is_none() {
                client.rx_bytes = session_client
                    .rx_bytes
                    .and_then(|bytes| u64::try_from(bytes).ok());
            }
            if client.hostname.is_none() {
                client.hostname.clone_from(&session_client.hostname);
            }
            if client.wireless.is_none() {
                let session_client: Client = Client::from((*session_client).clone());
                client.wireless = session_client.wireless;
            }
            let session_is_wired = session_client.is_wired.unwrap_or(false);
            if client.uplink_device_mac.is_none() {
                let uplink = if session_is_wired {
                    session_client.sw_mac.as_deref()
                } else {
                    session_client.ap_mac.as_deref()
                };
                client.uplink_device_mac = uplink.map(MacAddress::new);
            }
            if client.switch_port.is_none() && session_is_wired {
                client.switch_port = session_client.sw_port.and_then(|p| u32::try_from(p).ok());
            }
            merged += 1;
        }
    }
    tracing::debug!(
        total_clients = clients.len(),
        legacy_available = session_by_ip.len(),
        merged,
        "client traffic merge (by IP)"
    );
}

fn merge_user_reservations(
    clients: &mut [Client],
    session_clients: &[SessionClientEntry],
    session_users: &[SessionUserEntry],
) {
    if session_users.is_empty() {
        return;
    }

    let users_by_mac: HashMap<String, &SessionUserEntry> = session_users
        .iter()
        .map(|user| (user.mac.to_lowercase(), user))
        .collect();
    let mut merged_users = 0u32;
    for client in clients {
        let user = users_by_mac
            .get(&client.mac.as_str().to_lowercase())
            .or_else(|| {
                let ip_str = client.ip.map(|ip| ip.to_string())?;
                let session_client = session_clients
                    .iter()
                    .find(|lc| lc.ip.as_deref() == Some(ip_str.as_str()))?;
                users_by_mac.get(&session_client.mac.to_lowercase())
            });
        if let Some(user) = user {
            client.use_fixedip = user.use_fixedip.unwrap_or(false);
            client.fixed_ip = user.fixed_ip.as_deref().and_then(|ip| ip.parse().ok());
            if client.use_fixedip {
                merged_users += 1;
            }
        }
    }
    tracing::debug!(
        users_available = users_by_mac.len(),
        merged_users,
        "user DHCP reservation merge"
    );
}
