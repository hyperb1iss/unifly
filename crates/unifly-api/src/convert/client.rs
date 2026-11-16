use chrono::Utc;

use crate::integration_types;
use crate::model::client::{Client, ClientType, GuestAuth, WirelessInfo};
use crate::model::common::DataSource;
use crate::model::entity_id::{EntityId, MacAddress};
use crate::session::models::{SessionClientEntry, SessionUserEntry};

use super::helpers::{parse_ip, parse_ipv6_list, parse_iso};

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
            // The current addresses lead; ones seen recently follow.
            ipv6_addresses: parse_ipv6_list(c.ipv6_addresses.iter().chain(c.last_ipv6.iter())),
            name: c.name,
            hostname: c.hostname,
            client_type,
            connected_at,
            uplink_device_id: None,
            uplink_device_mac,
            switch_port,
            network_id: c.network_id.map(EntityId::from),
            network_name: c.network.filter(|name| !name.is_empty()),
            vlan: None,
            wireless,
            guest_auth,
            is_guest,
            tx_bytes: c.tx_bytes.and_then(|b| b.try_into().ok()),
            rx_bytes: c.rx_bytes.and_then(|b| b.try_into().ok()),
            bandwidth: None,
            os_name: None,
            device_class: None,
            use_fixedip: c.use_fixedip.unwrap_or(false),
            fixed_ip: c.fixed_ip.as_deref().and_then(|ip| ip.parse().ok()),
            blocked: c.blocked.unwrap_or(false),
            source: DataSource::SessionApi,
            updated_at: Utc::now(),
        }
    }
}

/// A client the controller remembers but that is not connected, such as
/// one it blocks. It has no address, uplink, or traffic to report.
impl From<SessionUserEntry> for Client {
    fn from(user: SessionUserEntry) -> Self {
        let client_type = match user.is_wired {
            Some(true) => ClientType::Wired,
            Some(false) => ClientType::Wireless,
            None => ClientType::Unknown,
        };
        Client {
            id: EntityId::from(user.id),
            mac: MacAddress::new(&user.mac),
            ip: None,
            ipv6_addresses: Vec::new(),
            name: user.name.filter(|name| !name.is_empty()),
            hostname: user.hostname.filter(|name| !name.is_empty()),
            client_type,
            connected_at: None,
            uplink_device_id: None,
            uplink_device_mac: None,
            switch_port: None,
            network_id: None,
            network_name: None,
            vlan: None,
            wireless: None,
            guest_auth: None,
            is_guest: user.is_guest.unwrap_or(false),
            tx_bytes: None,
            rx_bytes: None,
            bandwidth: None,
            os_name: None,
            device_class: None,
            use_fixedip: user.use_fixedip.unwrap_or(false),
            fixed_ip: user.fixed_ip.as_deref().and_then(|ip| ip.parse().ok()),
            blocked: user.blocked.unwrap_or(false),
            source: DataSource::SessionApi,
            updated_at: Utc::now(),
        }
    }
}

// ── Integration API ──────────────────────────────────────────────

/// Guest state from an Integration client's `access` object: its `type`
/// is `GUEST` for a client behind the portal, with `authorized` beside it.
fn integration_guest_auth(access: &serde_json::Value) -> Option<GuestAuth> {
    let text = |key: &str| access.get(key).and_then(serde_json::Value::as_str);
    (text("type") == Some("GUEST")).then(|| GuestAuth {
        authorized: access
            .get("authorized")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        method: None,
        expires_at: None,
        tx_bytes: None,
        rx_bytes: None,
        elapsed_minutes: None,
    })
}

impl From<integration_types::ClientResponse> for Client {
    fn from(c: integration_types::ClientResponse) -> Self {
        let client_type = match c.client_type.as_str() {
            "WIRED" => ClientType::Wired,
            "WIRELESS" => ClientType::Wireless,
            "VPN" => ClientType::Vpn,
            "TELEPORT" => ClientType::Teleport,
            _ => ClientType::Unknown,
        };

        let guest_auth = integration_guest_auth(&c.access);
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
            ipv6_addresses: Vec::new(),
            name: Some(c.name),
            hostname: None,
            client_type,
            connected_at: c.connected_at.as_deref().and_then(parse_iso),
            uplink_device_id: None,
            uplink_device_mac: None,
            switch_port: None,
            network_id: None,
            network_name: None,
            vlan: None,
            wireless: None,
            is_guest: guest_auth.is_some(),
            guest_auth,
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
    use std::net::Ipv6Addr;

    use super::*;

    #[test]
    fn channel_frequency_bands() {
        assert_eq!(channel_to_frequency(Some(6)), Some(2.4));
        assert_eq!(channel_to_frequency(Some(36)), Some(5.0));
        assert_eq!(channel_to_frequency(Some(149)), Some(5.0));
        assert_eq!(channel_to_frequency(None), None);
    }

    #[test]
    fn an_integration_guest_carries_its_portal_state() {
        let client = |access: serde_json::Value| -> Client {
            serde_json::from_value::<integration_types::ClientResponse>(serde_json::json!({
                "id": "c56a4180-65aa-42ec-a945-5fd21dec0538",
                "name": "Visitor phone",
                "type": "WIRELESS",
                "macAddress": "aa:bb:cc:dd:ee:ff",
                "access": access,
            }))
            .expect("deserialize")
            .into()
        };

        let waiting = client(serde_json::json!({ "type": "GUEST", "authorized": false }));
        assert!(waiting.is_guest);
        assert!(waiting.guest_auth.is_some_and(|auth| !auth.authorized));

        let through = client(serde_json::json!({ "type": "GUEST", "authorized": true }));
        assert!(through.guest_auth.is_some_and(|auth| auth.authorized));

        let regular = client(serde_json::json!({ "type": "DEFAULT" }));
        assert!(!regular.is_guest);
        assert!(regular.guest_auth.is_none());
    }

    #[test]
    fn a_remembered_user_becomes_an_offline_client() {
        let user: SessionUserEntry = serde_json::from_value(serde_json::json!({
            "_id": "user-9",
            "mac": "AA:BB:CC:DD:EE:09",
            "hostname": "old-tablet",
            "name": "",
            "blocked": true,
            "is_wired": false,
            "use_fixedip": true,
            "fixed_ip": "10.0.0.9"
        }))
        .expect("deserialize");
        let client: Client = user.into();
        assert_eq!(client.id, EntityId::from("user-9"));
        assert_eq!(client.mac.as_str(), "aa:bb:cc:dd:ee:09");
        assert_eq!(client.name, None, "an empty alias is no name");
        assert_eq!(client.hostname.as_deref(), Some("old-tablet"));
        assert_eq!(client.client_type, ClientType::Wireless);
        assert!(client.blocked);
        assert!(client.ip.is_none() && client.connected_at.is_none());
        assert_eq!(client.fixed_ip, "10.0.0.9".parse().ok());
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
    fn session_client_carries_its_network_name() {
        let entry: SessionClientEntry = serde_json::from_value(serde_json::json!({
            "_id": "abc",
            "mac": "aa:bb:cc:dd:ee:ff",
            "network": "Render",
            "network_id": "5f1a2b3c4d5e6f7a8b9c0d1e"
        }))
        .expect("deserialize");
        let client: Client = entry.into();
        assert_eq!(client.network_name.as_deref(), Some("Render"));

        let unnamed: SessionClientEntry = serde_json::from_value(serde_json::json!({
            "_id": "abc",
            "mac": "aa:bb:cc:dd:ee:ff",
            "network": ""
        }))
        .expect("deserialize");
        assert_eq!(Client::from(unnamed).network_name, None);
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

    #[test]
    fn a_station_holds_its_current_ipv6_addresses_then_the_recent_ones() {
        let entry: SessionClientEntry = serde_json::from_value(serde_json::json!({
            "_id": "abc",
            "mac": "aa:bb:cc:dd:ee:ff",
            "ip": "10.4.20.198",
            "ipv6_addresses": ["fe80::9", "2001:db8:3fa::9"],
            "last_ipv6": ["2001:db8:3fa::9", "fd00:c0de::9", "2001:db8:3fa::a"],
        }))
        .expect("deserialize");
        let client = Client::from(entry);
        let addresses: Vec<String> = client
            .ipv6_addresses
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            addresses,
            [
                "2001:db8:3fa::9",
                "2001:db8:3fa::a",
                "fd00:c0de::9",
                "fe80::9"
            ]
        );
        assert!(client.has_address("fd00:c0de::9".parse().expect("address")));
        assert!(client.has_address("10.4.20.198".parse().expect("address")));
        assert!(!client.has_address("2001:db8:3fa::b".parse().expect("address")));
        assert_eq!(client.primary_address(), "10.4.20.198".parse().ok());

        // A link-local address is never the one to reach a client at.
        let mut only_link_local = client.clone();
        only_link_local.ip = "fe80::9".parse().ok();
        only_link_local
            .ipv6_addresses
            .retain(Ipv6Addr::is_unicast_link_local);
        assert_eq!(only_link_local.primary_address(), None);
    }
}
