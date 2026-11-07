use crate::controller::Controller;
use crate::controller::support::require_session;
use crate::core_error::CoreError;
use crate::model::{
    EntityId, HealthSummary, IpsecSa, MagicSiteToSiteVpnConfig, RemoteAccessVpnServer,
    SiteToSiteVpn, VpnClientConnection, VpnClientProfile, WireGuardPeer,
};

use super::common::redact_sensitive_value;

impl Controller {
    pub async fn list_site_to_site_vpns(&self) -> Result<Vec<SiteToSiteVpn>, CoreError> {
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        let raw = session.list_network_conf().await?;
        Ok(raw
            .iter()
            .filter_map(site_to_site_vpn_from_raw)
            .collect::<Vec<_>>())
    }

    pub async fn get_site_to_site_vpn(&self, id: &str) -> Result<SiteToSiteVpn, CoreError> {
        self.list_site_to_site_vpns()
            .await?
            .into_iter()
            .find(|vpn| matches!(&vpn.id, EntityId::Legacy(value) if value == id))
            .ok_or_else(|| CoreError::NotFound {
                entity_type: "site-to-site VPN".into(),
                identifier: id.into(),
            })
    }

    pub async fn list_remote_access_vpn_servers(
        &self,
    ) -> Result<Vec<RemoteAccessVpnServer>, CoreError> {
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        let raw = session.list_network_conf().await?;
        Ok(raw
            .iter()
            .filter_map(remote_access_vpn_server_from_raw)
            .collect::<Vec<_>>())
    }

    pub async fn get_remote_access_vpn_server(
        &self,
        id: &str,
    ) -> Result<RemoteAccessVpnServer, CoreError> {
        self.list_remote_access_vpn_servers()
            .await?
            .into_iter()
            .find(|server| matches!(&server.id, EntityId::Legacy(value) if value == id))
            .ok_or_else(|| CoreError::NotFound {
                entity_type: "remote-access VPN server".into(),
                identifier: id.into(),
            })
    }

    pub async fn list_wireguard_peers(
        &self,
        server_id: Option<&str>,
    ) -> Result<Vec<WireGuardPeer>, CoreError> {
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        let raw = match server_id {
            Some(server_id) => session.list_wireguard_peers(server_id).await?,
            None => session.list_all_wireguard_peers().await?,
        };
        let mut peers = raw
            .iter()
            .filter_map(|value| wireguard_peer_from_raw(value, server_id))
            .collect::<Vec<_>>();
        peers.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
        });
        Ok(peers)
    }

    pub async fn get_wireguard_peer(
        &self,
        server_id: &str,
        id: &str,
    ) -> Result<WireGuardPeer, CoreError> {
        self.list_wireguard_peers(Some(server_id))
            .await?
            .into_iter()
            .find(|peer| matches!(&peer.id, EntityId::Legacy(value) if value == id))
            .ok_or_else(|| CoreError::NotFound {
                entity_type: "WireGuard peer".into(),
                identifier: id.into(),
            })
    }

    pub async fn list_wireguard_peer_existing_subnets(&self) -> Result<Vec<String>, CoreError> {
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        let raw = session.get_wireguard_peer_existing_subnets().await?;
        Ok(raw
            .get("subnets")
            .and_then(serde_json::Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default())
    }

    pub async fn list_openvpn_port_suggestions(&self) -> Result<Vec<u16>, CoreError> {
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        let raw = session.get_openvpn_port_suggestions().await?;
        Ok(raw
            .get("available_ports")
            .and_then(serde_json::Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(serde_json::Value::as_u64)
                    .filter_map(|value| u16::try_from(value).ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default())
    }

    pub async fn download_openvpn_configuration(&self, id: &str) -> Result<Vec<u8>, CoreError> {
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        Ok(session.download_openvpn_configuration(id).await?)
    }

    pub async fn list_vpn_client_profiles(&self) -> Result<Vec<VpnClientProfile>, CoreError> {
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        let mut clients = session
            .list_network_conf()
            .await?
            .iter()
            .filter_map(vpn_client_profile_from_raw)
            .collect::<Vec<_>>();
        clients.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
        });
        Ok(clients)
    }

    pub async fn get_vpn_client_profile(&self, id: &str) -> Result<VpnClientProfile, CoreError> {
        self.list_vpn_client_profiles()
            .await?
            .into_iter()
            .find(|client| matches!(&client.id, EntityId::Legacy(value) if value == id))
            .ok_or_else(|| CoreError::NotFound {
                entity_type: "VPN client profile".into(),
                identifier: id.into(),
            })
    }

    pub async fn list_vpn_client_connections(&self) -> Result<Vec<VpnClientConnection>, CoreError> {
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        let mut connections = session
            .list_vpn_client_connections()
            .await?
            .iter()
            .filter_map(vpn_client_connection_from_raw)
            .collect::<Vec<_>>();
        connections.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
        });
        Ok(connections)
    }

    pub async fn get_vpn_client_connection(
        &self,
        id: &str,
    ) -> Result<VpnClientConnection, CoreError> {
        self.list_vpn_client_connections()
            .await?
            .into_iter()
            .find(|connection| matches!(&connection.id, EntityId::Legacy(value) if value == id))
            .ok_or_else(|| CoreError::NotFound {
                entity_type: "VPN client connection".into(),
                identifier: id.into(),
            })
    }

    pub async fn list_magic_site_to_site_vpn_configs(
        &self,
    ) -> Result<Vec<MagicSiteToSiteVpnConfig>, CoreError> {
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        let mut configs = session
            .list_magic_site_to_site_vpn_configs()
            .await?
            .iter()
            .filter_map(magic_site_to_site_vpn_config_from_raw)
            .collect::<Vec<_>>();
        configs.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
        });
        Ok(configs)
    }

    pub async fn get_magic_site_to_site_vpn_config(
        &self,
        id: &str,
    ) -> Result<MagicSiteToSiteVpnConfig, CoreError> {
        self.list_magic_site_to_site_vpn_configs()
            .await?
            .into_iter()
            .find(|config| matches!(&config.id, EntityId::Legacy(value) if value == id))
            .ok_or_else(|| CoreError::NotFound {
                entity_type: "magic site-to-site VPN config".into(),
                identifier: id.into(),
            })
    }

    pub async fn list_ipsec_sa(&self) -> Result<Vec<IpsecSa>, CoreError> {
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        Ok(session.list_ipsec_sa().await?)
    }

    pub fn get_vpn_health(&self) -> Option<HealthSummary> {
        self.inner
            .store
            .site_health_snapshot()
            .iter()
            .find(|summary| summary.subsystem.eq_ignore_ascii_case("vpn"))
            .cloned()
    }
}

fn site_to_site_vpn_from_raw(raw: &serde_json::Value) -> Option<SiteToSiteVpn> {
    let fields = raw.as_object()?;
    if fields.get("purpose")?.as_str()? != "site-vpn" {
        return None;
    }

    let id = fields.get("_id")?.as_str()?.to_owned();
    let name = fields
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();

    let vpn_type = fields
        .get("vpn_type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown")
        .to_owned();

    let remote_host = fields
        .get("ipsec_peer_ip")
        .or_else(|| fields.get("openvpn_remote_host"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);

    let local_ip = fields
        .get("ipsec_local_ip")
        .or_else(|| fields.get("openvpn_local_address"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);

    let interface = fields
        .get("ipsec_interface")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);

    let remote_site_id = fields
        .get("remote_site_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);

    let remote_vpn_subnets = fields
        .get("remote_vpn_subnets")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Some(SiteToSiteVpn {
        id: EntityId::Legacy(id),
        name,
        enabled: fields
            .get("enabled")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
        vpn_type,
        remote_site_id,
        local_ip,
        interface,
        remote_host,
        remote_vpn_subnets,
        fields: redact_sensitive_value(&serde_json::Value::Object(fields.clone()))
            .as_object()
            .cloned()
            .unwrap_or_default(),
    })
}

fn remote_access_vpn_server_from_raw(raw: &serde_json::Value) -> Option<RemoteAccessVpnServer> {
    let fields = raw.as_object()?;
    if fields.get("purpose")?.as_str()? != "remote-user-vpn" {
        return None;
    }

    let id = fields.get("_id")?.as_str()?.to_owned();
    let vpn_type = fields
        .get("vpn_type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown")
        .to_owned();

    Some(RemoteAccessVpnServer {
        id: EntityId::Legacy(id),
        name: fields
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        enabled: fields
            .get("enabled")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
        vpn_type,
        local_port: fields
            .get("local_port")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u16::try_from(value).ok()),
        local_wan_ip: first_non_empty_string(
            fields,
            &[
                "wireguard_local_wan_ip",
                "openvpn_local_wan_ip",
                "l2tp_local_wan_ip",
            ],
        ),
        interface: first_non_empty_string(
            fields,
            &["wireguard_interface", "openvpn_interface", "l2tp_interface"],
        ),
        gateway_subnet: first_non_empty_string(fields, &["ip_subnet", "ipv6_subnet"]),
        radius_profile_id: first_non_empty_string(fields, &["radiusprofile_id"]),
        exposed_to_site_vpn: fields
            .get("exposed_to_site_vpn")
            .and_then(serde_json::Value::as_bool),
        fields: redact_sensitive_value(&serde_json::Value::Object(fields.clone()))
            .as_object()
            .cloned()
            .unwrap_or_default(),
    })
}

fn wireguard_peer_from_raw(
    raw: &serde_json::Value,
    default_server_id: Option<&str>,
) -> Option<WireGuardPeer> {
    let fields = raw.as_object()?;
    let id = fields.get("_id")?.as_str()?.to_owned();
    let server_id = first_non_empty_string(fields, &["networkId", "network_id"])
        .or_else(|| default_server_id.map(str::to_owned))
        .map(EntityId::Legacy);
    let allowed_ips = fields
        .get("allowed_ips")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Some(WireGuardPeer {
        id: EntityId::Legacy(id),
        server_id,
        name: fields
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        interface_ip: first_non_empty_string(fields, &["interface_ip"]),
        interface_ipv6: first_non_empty_string(fields, &["interface_ipv6"]),
        public_key: first_non_empty_string(fields, &["public_key"]),
        allowed_ips,
        has_private_key: fields
            .get("private_key")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| !value.is_empty()),
        has_preshared_key: fields
            .get("preshared_key")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| !value.is_empty()),
        fields: redact_sensitive_value(&serde_json::Value::Object(fields.clone()))
            .as_object()
            .cloned()
            .unwrap_or_default(),
    })
}

fn vpn_client_connection_from_raw(raw: &serde_json::Value) -> Option<VpnClientConnection> {
    let fields = raw.as_object()?;
    let id = first_non_empty_string(fields, &["network_id", "networkId", "id", "_id"])?;

    Some(VpnClientConnection {
        id: EntityId::Legacy(id),
        name: first_non_empty_string(
            fields,
            &[
                "name",
                "display_name",
                "network_name",
                "server_name",
                "remote_name",
            ],
        ),
        connection_type: first_non_empty_string(fields, &["type", "vpn_type", "connection_type"]),
        status: first_non_empty_string(fields, &["status", "state"]),
        local_address: first_non_empty_string(
            fields,
            &["local_ip", "local_address", "localAddress"],
        ),
        remote_address: first_non_empty_string(
            fields,
            &[
                "remote_ip",
                "remote_address",
                "remoteAddress",
                "server_ip",
                "serverAddress",
                "remote_host",
            ],
        ),
        username: first_non_empty_string(fields, &["username", "user"]),
        fields: redact_sensitive_value(&serde_json::Value::Object(fields.clone()))
            .as_object()
            .cloned()
            .unwrap_or_default(),
    })
}

fn vpn_client_profile_from_raw(raw: &serde_json::Value) -> Option<VpnClientProfile> {
    let fields = raw.as_object()?;
    let vpn_type = first_non_empty_string(fields, &["vpn_type", "type"]).unwrap_or_default();

    match fields.get("purpose").and_then(serde_json::Value::as_str) {
        Some("vpn-client") => {}
        Some(_) => return None,
        None if !vpn_type.ends_with("-client") => return None,
        None => {}
    }

    let id = first_non_empty_string(fields, &["_id", "id"])?;

    Some(VpnClientProfile {
        id: EntityId::Legacy(id),
        name: fields
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        enabled: fields
            .get("enabled")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
        vpn_type,
        server_address: first_non_empty_string(
            fields,
            &[
                "remote_host",
                "remote_address",
                "server_address",
                "server_ip",
                "server",
                "host",
                "peer_ip",
            ],
        ),
        server_port: ["remote_port", "server_port", "port"]
            .iter()
            .find_map(|key| {
                fields
                    .get(*key)
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| u16::try_from(value).ok())
            }),
        local_address: first_non_empty_string(
            fields,
            &[
                "local_address",
                "local_ip",
                "interface_ip",
                "vpn_ip",
                "openvpn_local_address",
                "wireguard_local_address",
            ],
        ),
        username: first_non_empty_string(fields, &["username", "user"]),
        interface: first_non_empty_string(
            fields,
            &[
                "interface",
                "wan_interface",
                "wan_network",
                "bind_interface",
                "openvpn_interface",
                "wireguard_interface",
            ],
        ),
        route_distance: fields
            .get("route_distance")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        fields: redact_sensitive_value(&serde_json::Value::Object(fields.clone()))
            .as_object()
            .cloned()
            .unwrap_or_default(),
    })
}

fn magic_site_to_site_vpn_config_from_raw(
    raw: &serde_json::Value,
) -> Option<MagicSiteToSiteVpnConfig> {
    let fields = raw.as_object()?;
    let id = first_non_empty_string(fields, &["id", "_id"])?;

    Some(MagicSiteToSiteVpnConfig {
        id: EntityId::Legacy(id),
        name: first_non_empty_string(fields, &["name", "display_name"]),
        status: first_non_empty_string(fields, &["status", "state"]),
        enabled: fields.get("enabled").and_then(serde_json::Value::as_bool),
        local_site_name: first_non_empty_string(
            fields,
            &["localSiteName", "local_site_name", "local_site"],
        ),
        remote_site_name: first_non_empty_string(
            fields,
            &["remoteSiteName", "remote_site_name", "remote_site"],
        ),
        fields: redact_sensitive_value(&serde_json::Value::Object(fields.clone()))
            .as_object()
            .cloned()
            .unwrap_or_default(),
    })
}

fn first_non_empty_string(
    fields: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<String> {
    keys.iter().find_map(|key| {
        fields
            .get(*key)
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        magic_site_to_site_vpn_config_from_raw, remote_access_vpn_server_from_raw,
        site_to_site_vpn_from_raw, vpn_client_connection_from_raw, vpn_client_profile_from_raw,
        wireguard_peer_from_raw,
    };
    use crate::config::ControllerConfig;
    use crate::controller::Controller;
    use crate::model::{EntityId, HealthSummary};

    #[test]
    fn get_vpn_health_reads_cached_store_snapshot() {
        let controller = Controller::new(ControllerConfig::default());
        let summaries = Arc::new(vec![
            HealthSummary {
                subsystem: "wan".into(),
                status: "ok".into(),
                num_adopted: Some(1),
                num_sta: Some(5),
                tx_bytes_r: Some(100),
                rx_bytes_r: Some(200),
                latency: Some(1.0),
                wan_ip: Some("198.51.100.1".into()),
                gateways: Some(vec!["gw".into()]),
                extra: serde_json::Value::Null,
            },
            HealthSummary {
                subsystem: "vpn".into(),
                status: "warn".into(),
                num_adopted: None,
                num_sta: Some(2),
                tx_bytes_r: Some(300),
                rx_bytes_r: Some(400),
                latency: Some(2.5),
                wan_ip: None,
                gateways: None,
                extra: serde_json::json!({ "source": "cache" }),
            },
        ]);
        let _previous = controller.inner.store.site_health.send_replace(summaries);

        let vpn_health = controller.get_vpn_health().expect("vpn summary");

        assert_eq!(vpn_health.subsystem, "vpn");
        assert_eq!(vpn_health.status, "warn");
        assert_eq!(vpn_health.num_sta, Some(2));
        assert_eq!(vpn_health.extra["source"], "cache");
    }

    #[test]
    fn site_to_site_vpn_from_raw_maps_legacy_networkconf_record() {
        let raw = serde_json::json!({
            "_id": "vpn123",
            "name": "Branch Tunnel",
            "purpose": "site-vpn",
            "enabled": true,
            "vpn_type": "ipsec-vpn",
            "ipsec_local_ip": "10.0.0.1",
            "ipsec_interface": "WAN1",
            "ipsec_peer_ip": "198.51.100.10",
            "remote_vpn_subnets": ["10.10.0.0/24"]
        });

        let vpn = site_to_site_vpn_from_raw(&raw).expect("site-vpn should map");

        assert_eq!(vpn.id, EntityId::Legacy("vpn123".into()));
        assert_eq!(vpn.name, "Branch Tunnel");
        assert_eq!(vpn.vpn_type, "ipsec-vpn");
        assert_eq!(vpn.local_ip.as_deref(), Some("10.0.0.1"));
        assert_eq!(vpn.interface.as_deref(), Some("WAN1"));
        assert_eq!(vpn.remote_host.as_deref(), Some("198.51.100.10"));
        assert_eq!(vpn.remote_vpn_subnets, vec!["10.10.0.0/24"]);
    }

    #[test]
    fn remote_access_vpn_server_from_raw_maps_legacy_networkconf_record() {
        let raw = serde_json::json!({
            "_id": "vpn456",
            "name": "WireGuard Remote Access",
            "purpose": "remote-user-vpn",
            "enabled": true,
            "vpn_type": "wireguard",
            "local_port": 51820,
            "wireguard_local_wan_ip": "203.0.113.10",
            "wireguard_interface": "WAN1",
            "ip_subnet": "192.168.42.1/24",
            "radiusprofile_id": "radius123",
            "exposed_to_site_vpn": true,
            "x_wireguard_private_key": "secret"
        });

        let server = remote_access_vpn_server_from_raw(&raw).expect("remote-user-vpn should map");

        assert_eq!(server.id, EntityId::Legacy("vpn456".into()));
        assert_eq!(server.name, "WireGuard Remote Access");
        assert_eq!(server.vpn_type, "wireguard");
        assert_eq!(server.local_port, Some(51820));
        assert_eq!(server.local_wan_ip.as_deref(), Some("203.0.113.10"));
        assert_eq!(server.interface.as_deref(), Some("WAN1"));
        assert_eq!(server.gateway_subnet.as_deref(), Some("192.168.42.1/24"));
        assert_eq!(server.radius_profile_id.as_deref(), Some("radius123"));
        assert_eq!(server.exposed_to_site_vpn, Some(true));
        assert_eq!(
            server.fields["x_wireguard_private_key"].as_str(),
            Some("***REDACTED***")
        );
    }

    #[test]
    fn wireguard_peer_from_raw_maps_legacy_v2_record() {
        let raw = serde_json::json!({
            "_id": "peer789",
            "networkId": "server123",
            "name": "Laptop",
            "interface_ip": "192.168.42.2",
            "interface_ipv6": "fd00::2",
            "public_key": "pubkey",
            "private_key": "secret",
            "preshared_key": "psk",
            "allowed_ips": ["10.0.0.0/24"]
        });

        let peer = wireguard_peer_from_raw(&raw, None).expect("WireGuard peer should map");

        assert_eq!(peer.id, EntityId::Legacy("peer789".into()));
        assert_eq!(peer.server_id, Some(EntityId::Legacy("server123".into())));
        assert_eq!(peer.name, "Laptop");
        assert_eq!(peer.interface_ip.as_deref(), Some("192.168.42.2"));
        assert_eq!(peer.interface_ipv6.as_deref(), Some("fd00::2"));
        assert_eq!(peer.public_key.as_deref(), Some("pubkey"));
        assert_eq!(peer.allowed_ips, vec!["10.0.0.0/24"]);
        assert!(peer.has_private_key);
        assert!(peer.has_preshared_key);
        assert_eq!(peer.fields["private_key"].as_str(), Some("***REDACTED***"));
        assert_eq!(
            peer.fields["preshared_key"].as_str(),
            Some("***REDACTED***")
        );
    }

    #[test]
    fn vpn_client_connection_from_raw_maps_v2_record() {
        let raw = serde_json::json!({
            "network_id": "vpn-client-1",
            "name": "Branch Client",
            "type": "openvpn-client",
            "status": "CONNECTED",
            "local_ip": "10.0.0.2",
            "remote_ip": "198.51.100.10",
            "username": "branch-user",
            "password": "secret"
        });

        let connection = vpn_client_connection_from_raw(&raw).expect("VPN connection should map");

        assert_eq!(connection.id, EntityId::Legacy("vpn-client-1".into()));
        assert_eq!(connection.name.as_deref(), Some("Branch Client"));
        assert_eq!(
            connection.connection_type.as_deref(),
            Some("openvpn-client")
        );
        assert_eq!(connection.status.as_deref(), Some("CONNECTED"));
        assert_eq!(connection.local_address.as_deref(), Some("10.0.0.2"));
        assert_eq!(connection.remote_address.as_deref(), Some("198.51.100.10"));
        assert_eq!(connection.username.as_deref(), Some("branch-user"));
        assert_eq!(
            connection.fields["password"].as_str(),
            Some("***REDACTED***")
        );
    }

    #[test]
    fn vpn_client_profile_from_raw_maps_legacy_networkconf_record() {
        let raw = serde_json::json!({
            "_id": "vpn-client-1",
            "name": "Branch Client",
            "purpose": "vpn-client",
            "enabled": false,
            "vpn_type": "openvpn-client",
            "remote_host": "198.51.100.10",
            "remote_port": 1194,
            "local_address": "10.0.0.2",
            "username": "branch-user",
            "interface": "WAN1",
            "route_distance": 15,
            "password": "secret"
        });

        let client = vpn_client_profile_from_raw(&raw).expect("vpn-client should map");

        assert_eq!(client.id, EntityId::Legacy("vpn-client-1".into()));
        assert_eq!(client.name, "Branch Client");
        assert!(!client.enabled);
        assert_eq!(client.vpn_type, "openvpn-client");
        assert_eq!(client.server_address.as_deref(), Some("198.51.100.10"));
        assert_eq!(client.server_port, Some(1194));
        assert_eq!(client.local_address.as_deref(), Some("10.0.0.2"));
        assert_eq!(client.username.as_deref(), Some("branch-user"));
        assert_eq!(client.interface.as_deref(), Some("WAN1"));
        assert_eq!(client.route_distance, Some(15));
        assert_eq!(client.fields["password"].as_str(), Some("***REDACTED***"));
    }

    #[test]
    fn magic_site_to_site_vpn_config_from_raw_maps_v2_record() {
        let raw = serde_json::json!({
            "id": "magic-1",
            "name": "HQ <-> Branch",
            "status": "CONNECTED",
            "enabled": true,
            "localSiteName": "HQ",
            "remoteSiteName": "Branch",
            "token": "secret"
        });

        let config =
            magic_site_to_site_vpn_config_from_raw(&raw).expect("magic site-to-site should map");

        assert_eq!(config.id, EntityId::Legacy("magic-1".into()));
        assert_eq!(config.name.as_deref(), Some("HQ <-> Branch"));
        assert_eq!(config.status.as_deref(), Some("CONNECTED"));
        assert_eq!(config.enabled, Some(true));
        assert_eq!(config.local_site_name.as_deref(), Some("HQ"));
        assert_eq!(config.remote_site_name.as_deref(), Some("Branch"));
        assert_eq!(config.fields["token"].as_str(), Some("***REDACTED***"));
    }
}
