use serde::{Deserialize, Serialize};

use crate::model::{EntityId, NetworkManagement, NetworkPurpose, WifiSecurityMode};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct CreateNetworkRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vlan_id: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub management: Option<NetworkManagement>,
    /// Integration UUID of the L3 switch that manages a switch network.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<EntityId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<NetworkPurpose>,
    pub dhcp_enabled: bool,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dhcp_range_start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dhcp_range_stop: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dhcp_lease_time: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns_servers: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firewall_zone_id: Option<String>,
    pub isolation_enabled: bool,
    pub internet_access_enabled: bool,
}

/// Optional DHCP overrides for network updates (from JSON files).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DhcpUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_stop: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns_servers: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct UpdateNetworkRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vlan_id: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dhcp_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolation_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internet_access_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mdns_forwarding_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv6_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dhcp: Option<DhcpUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct CreateWifiBroadcastRequest {
    pub name: String,
    #[serde(default)]
    pub ssid: String,
    pub security_mode: WifiSecurityMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passphrase: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none", alias = "network")]
    pub network_id: Option<EntityId>,
    #[serde(default, alias = "hideName", alias = "hidden")]
    pub hide_ssid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broadcast_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "broadcastingFrequenciesGHz", alias = "frequencies")]
    pub frequencies_ghz: Option<Vec<f32>>,
    #[serde(default)]
    #[serde(alias = "bandSteeringEnabled")]
    pub band_steering: bool,
    #[serde(
        default,
        alias = "bssTransitionEnabled",
        skip_serializing_if = "Option::is_none"
    )]
    pub fast_roaming: Option<bool>,
    /// Lock the 2.4 GHz channel to 6 on broadcasting devices.
    #[serde(skip_serializing_if = "Option::is_none", alias = "channel2gLockedTo6")]
    pub channel2g_locked_to6: Option<bool>,
    /// Lock the 2.4 GHz DTIM period to 3.
    #[serde(
        skip_serializing_if = "Option::is_none",
        alias = "dtimPeriod2gLockedTo3"
    )]
    pub dtim_period2g_locked_to3: Option<bool>,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateWifiBroadcastRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_mode: Option<WifiSecurityMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passphrase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "hideName")]
    pub hide_ssid: Option<bool>,
    /// Lock the 2.4 GHz channel to 6 on broadcasting devices.
    #[serde(skip_serializing_if = "Option::is_none", alias = "channel2gLockedTo6")]
    pub channel2g_locked_to6: Option<bool>,
    /// Lock the 2.4 GHz DTIM period to 3.
    #[serde(
        skip_serializing_if = "Option::is_none",
        alias = "dtimPeriod2gLockedTo3"
    )]
    pub dtim_period2g_locked_to3: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::CreateWifiBroadcastRequest;

    #[test]
    fn create_wifi_broadcast_request_honors_alias_fields() {
        let request: CreateWifiBroadcastRequest = serde_json::from_value(serde_json::json!({
            "name": "Guest",
            "ssid": "Guest",
            "security_mode": "Open",
            "enabled": true,
            "hideName": true,
            "bandSteeringEnabled": true,
            "bssTransitionEnabled": true
        }))
        .expect("wifi broadcast request should deserialize");

        assert!(request.hide_ssid);
        assert!(request.band_steering);
        assert_eq!(request.fast_roaming, Some(true));
    }

    #[test]
    fn create_wifi_broadcast_request_honors_cli_style_aliases() {
        let request: CreateWifiBroadcastRequest = serde_json::from_value(serde_json::json!({
            "name": "Test",
            "security_mode": "Wpa3Personal",
            "passphrase": "test1234",
            "hidden": true,
            "fast_roaming": true,
            "frequencies": [2.4]
        }))
        .expect("wifi broadcast request should deserialize with CLI-style aliases");

        assert!(request.hide_ssid);
        assert_eq!(request.fast_roaming, Some(true));
        assert_eq!(request.frequencies_ghz, Some(vec![2.4]));
    }
}
