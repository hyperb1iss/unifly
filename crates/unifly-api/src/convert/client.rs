use chrono::Utc;

use crate::integration_types;
use crate::model::client::{Client, ClientType, GuestAuth, WirelessInfo};
use crate::model::common::DataSource;
use crate::model::entity_id::{EntityId, MacAddress};
use crate::session::models::SessionClientEntry;

use super::helpers::{parse_ip, parse_iso};

// ── Session API ──────────────────────────────────────────────────

fn channel_to_frequency(channel: Option<i32>) -> Option<f32> {
    channel.map(|ch| match ch {
        1..=14 => 2.4,
        32..=68 | 96..=177 => 5.0,
        _ => 6.0,
    })
}

impl From<SessionClientEntry> for Client {
    fn from(c: SessionClientEntry) -> Self {
        let is_wired = c.is_wired.unwrap_or(false);
        let client_type = if is_wired {
            ClientType::Wired
        } else {
            ClientType::Wireless
        };

        let wireless = if is_wired {
            None
        } else {
            Some(WirelessInfo {
                ssid: c.essid.clone(),
                bssid: c.bssid.as_deref().map(MacAddress::new),
                channel: c.channel.and_then(|ch| ch.try_into().ok()),
                frequency_ghz: channel_to_frequency(c.channel),
                signal_dbm: c.signal.or(c.rssi),
                noise_dbm: c.noise,
                satisfaction: c.satisfaction.and_then(|s| s.try_into().ok()),
                tx_rate_kbps: c.tx_rate.and_then(|r| r.try_into().ok()),
                rx_rate_kbps: c.rx_rate.and_then(|r| r.try_into().ok()),
            })
        };

        let is_guest = c.is_guest.unwrap_or(false);
        let guest_auth = if is_guest {
            Some(GuestAuth {
                authorized: c.authorized.unwrap_or(false),
                method: None,
                expires_at: None,
                tx_bytes: c.tx_bytes.and_then(|b| b.try_into().ok()),
                rx_bytes: c.rx_bytes.and_then(|b| b.try_into().ok()),
                elapsed_minutes: None,
            })
        } else {
            None
        };

        let uplink_device_mac = if is_wired {
            c.sw_mac.as_deref().map(MacAddress::new)
        } else {
            c.ap_mac.as_deref().map(MacAddress::new)
        };

        let switch_port = if is_wired {
            c.sw_port.and_then(|port| u32::try_from(port).ok())
        } else {
            None
        };

        let connected_at = c.uptime.and_then(|secs| {
            let duration = chrono::Duration::seconds(secs);
            Utc::now().checked_sub_signed(duration)
        });

        Client {
            id: EntityId::from(c.id),
            mac: MacAddress::new(&c.mac),
            ip: parse_ip(c.ip.as_ref()),
            name: c.name,
            hostname: c.hostname,
            client_type,
            connected_at,
            uplink_device_id: None,
            uplink_device_mac,
            switch_port,
            network_id: c.network_id.map(EntityId::from),
            vlan: None,
            wireless,
            guest_auth,
            is_guest,
            tx_bytes: c.tx_bytes.and_then(|b| b.try_into().ok()),
            rx_bytes: c.rx_bytes.and_then(|b| b.try_into().ok()),
            bandwidth: None,
            os_name: None,
            device_class: None,
            use_fixedip: false,
            fixed_ip: None,
            blocked: c.blocked.unwrap_or(false),
            source: DataSource::SessionApi,
            updated_at: Utc::now(),
        }
    }
}

// ── Integration API ──────────────────────────────────────────────

impl From<integration_types::ClientResponse> for Client {
    fn from(c: integration_types::ClientResponse) -> Self {
        let client_type = match c.client_type.as_str() {
            "WIRED" => ClientType::Wired,
            "WIRELESS" => ClientType::Wireless,
            "VPN" => ClientType::Vpn,
            "TELEPORT" => ClientType::Teleport,
            _ => ClientType::Unknown,
        };

        let uuid_fallback = c.id.to_string();
        let mac_str = c
            .mac_address
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&uuid_fallback);

        Client {
            id: EntityId::Uuid(c.id),
            mac: MacAddress::new(mac_str),
            ip: c.ip_address.as_deref().and_then(|s| s.parse().ok()),
            name: Some(c.name),
            hostname: None,
            client_type,
            connected_at: c.connected_at.as_deref().and_then(parse_iso),
            uplink_device_id: None,
            uplink_device_mac: None,
            switch_port: None,
            network_id: None,
            vlan: None,
            wireless: None,
            guest_auth: None,
            is_guest: false,
            tx_bytes: None,
            rx_bytes: None,
            bandwidth: None,
            os_name: None,
            device_class: None,
            use_fixedip: false,
            fixed_ip: None,
            blocked: false,
            source: DataSource::IntegrationApi,
            updated_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_frequency_bands() {
        assert_eq!(channel_to_frequency(Some(6)), Some(2.4));
        assert_eq!(channel_to_frequency(Some(36)), Some(5.0));
        assert_eq!(channel_to_frequency(Some(149)), Some(5.0));
        assert_eq!(channel_to_frequency(None), None);
    }

    #[test]
    fn wired_session_client_carries_switch_port() {
        let entry: SessionClientEntry = serde_json::from_value(serde_json::json!({
            "_id": "abc",
            "mac": "aa:bb:cc:dd:ee:ff",
            "is_wired": true,
            "sw_mac": "11:22:33:44:55:66",
            "sw_port": 9
        }))
        .expect("deserialize");
        let client: Client = entry.into();
        assert_eq!(client.switch_port, Some(9));
        assert_eq!(client.client_type, ClientType::Wired);
    }

    #[test]
    fn wireless_session_client_drops_switch_port() {
        let entry: SessionClientEntry = serde_json::from_value(serde_json::json!({
            "_id": "abc",
            "mac": "aa:bb:cc:dd:ee:ff",
            "is_wired": false,
            "sw_port": 9
        }))
        .expect("deserialize");
        let client: Client = entry.into();
        assert!(client.switch_port.is_none());
    }
}
