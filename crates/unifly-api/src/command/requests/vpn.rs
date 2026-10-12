use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

fn default_enabled() -> bool {
    true
}

fn default_purpose() -> String {
    "site-vpn".into()
}

fn default_remote_access_purpose() -> String {
    "remote-user-vpn".into()
}

fn default_vpn_client_purpose() -> String {
    "vpn-client".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSiteToSiteVpnRequest {
    pub name: String,
    pub vpn_type: String,
    #[serde(default = "default_purpose")]
    pub purpose: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_site_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_ipsec_pre_shared_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_peer_ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_dynamic_routing: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_separate_ikev2_networks: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_tunnel_ip_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_tunnel_ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_key_exchange: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_remote_identifier_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_remote_identifier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_local_identifier_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_local_identifier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_pfs: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_ike_encryption: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_ike_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_ike_dh_group: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_dh_group: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_ike_lifetime: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_esp_encryption: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_esp_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_esp_dh_group: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_esp_lifetime: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_interface: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipsec_local_ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_vpn_dynamic_subnets_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_openvpn_shared_secret_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openvpn_local_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openvpn_local_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openvpn_encryption_cipher: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openvpn_remote_host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openvpn_remote_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openvpn_remote_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openvpn_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interface_mtu_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interface_mtu: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mss_clamp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mss_clamp_mss: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_distance: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remote_vpn_subnets: Vec<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

pub type UpdateSiteToSiteVpnRequest = CreateSiteToSiteVpnRequest;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRemoteAccessVpnServerRequest {
    pub name: String,
    pub vpn_type: String,
    #[serde(default = "default_remote_access_purpose")]
    pub purpose: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setting_preference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l2tp_allow_weak_ciphers: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require_mschapv2: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposed_to_site_vpn: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dhcpd_wins_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dhcpd_wins_1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dhcpd_wins_2: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dhcpd_dns_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dhcpd_dns_1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dhcpd_dns_2: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_wireguard_private_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vpn_client_configuration_remote_ip_override_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vpn_client_configuration_remote_ip_override: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_ipsec_pre_shared_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radiusprofile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_subnet: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipv6_subnet: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dhcpd_start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dhcpd_stop: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wireguard_interface: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wireguard_interface_binding_mode_ip_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l2tp_interface: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openvpn_interface: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wireguard_local_wan_ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l2tp_local_wan_ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openvpn_local_wan_ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vpn_binding_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interface_mtu_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interface_mtu: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mss_clamp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mss_clamp_mss: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mss_clamp_ipv6: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mss_clamp_mss_ipv6: Option<u16>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

pub type UpdateRemoteAccessVpnServerRequest = CreateRemoteAccessVpnServerRequest;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateVpnClientProfileRequest {
    pub name: String,
    pub vpn_type: String,
    #[serde(default = "default_vpn_client_purpose")]
    pub purpose: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

pub type UpdateVpnClientProfileRequest = CreateVpnClientProfileRequest;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWireGuardPeerRequest {
    pub name: String,
    pub interface_ip: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interface_ipv6: Option<String>,
    pub public_key: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_ips: Vec<String>,
    #[serde(default)]
    pub preshared_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

pub type UpdateWireGuardPeerRequest = CreateWireGuardPeerRequest;

#[cfg(test)]
mod tests {
    use super::{
        CreateRemoteAccessVpnServerRequest, CreateSiteToSiteVpnRequest,
        CreateVpnClientProfileRequest, CreateWireGuardPeerRequest,
    };

    #[test]
    fn site_to_site_request_defaults_purpose_and_enabled() {
        let request: CreateSiteToSiteVpnRequest = serde_json::from_value(serde_json::json!({
            "name": "Branch Tunnel",
            "vpn_type": "ipsec-vpn"
        }))
        .expect("request should deserialize");

        assert_eq!(request.purpose, "site-vpn");
        assert!(request.enabled);
    }

    #[test]
    fn remote_access_request_defaults_purpose_and_enabled() {
        let request: CreateRemoteAccessVpnServerRequest =
            serde_json::from_value(serde_json::json!({
                "name": "WireGuard Remote Access",
                "vpn_type": "wireguard"
            }))
            .expect("request should deserialize");

        assert_eq!(request.purpose, "remote-user-vpn");
        assert!(request.enabled);
    }

    #[test]
    fn vpn_client_profile_request_defaults_purpose_and_enabled() {
        let request: CreateVpnClientProfileRequest = serde_json::from_value(serde_json::json!({
            "name": "Branch Client",
            "vpn_type": "openvpn-client"
        }))
        .expect("request should deserialize");

        assert_eq!(request.purpose, "vpn-client");
        assert!(request.enabled);
    }

    #[test]
    fn wireguard_peer_request_preserves_empty_preshared_key() {
        let request: CreateWireGuardPeerRequest = serde_json::from_value(serde_json::json!({
            "name": "Laptop",
            "interface_ip": "192.168.42.2",
            "public_key": "pubkey",
            "allowed_ips": []
        }))
        .expect("request should deserialize");

        assert_eq!(request.preshared_key, "");
        assert_eq!(request.allowed_ips, Vec::<String>::new());
        assert!(request.private_key.is_none());
    }
}
