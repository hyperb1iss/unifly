use std::collections::HashMap;
use std::net::Ipv4Addr;

use serde_json::{Map, Value};

use crate::command::UpdateNetworkRequest;
use crate::core_error::CoreError;
use crate::model::WifiSecurityMode;

/// DHCP lease time used when a request does not name one.
pub(in super::super) const DEFAULT_DHCP_LEASE_SECS: u32 = 86_400;

pub(in super::super) fn parse_ipv4_cidr(cidr: &str) -> Result<(Ipv4Addr, u8), CoreError> {
    let (host, prefix) = cidr
        .split_once('/')
        .ok_or_else(|| CoreError::ValidationFailed {
            message: format!("invalid ipv4 host/prefix value '{cidr}'"),
        })?;
    let host_ip = host
        .parse::<Ipv4Addr>()
        .map_err(|_| CoreError::ValidationFailed {
            message: format!("invalid IPv4 host address '{host}'"),
        })?;
    let prefix_len = prefix
        .parse::<u8>()
        .map_err(|_| CoreError::ValidationFailed {
            message: format!("invalid IPv4 prefix length '{prefix}'"),
        })?;
    if prefix_len > 32 {
        return Err(CoreError::ValidationFailed {
            message: format!("IPv4 prefix length must be <= 32, got {prefix_len}"),
        });
    }
    Ok((host_ip, prefix_len))
}

/// Apply the IPv4 parts of a network update (subnet, DHCP server state,
/// DHCP DNS servers) to the fields of the network as last read.
///
/// The Integration API replaces the whole network on update, so these are
/// written into `ipv4Configuration` alongside everything the controller
/// returned. A DHCP server is `dhcpConfiguration.mode = "SERVER"`; turning
/// it off omits `dhcpConfiguration`, which the API documents as DHCP off.
pub(in super::super) fn apply_ipv4_update(
    extra: &mut HashMap<String, Value>,
    update: &UpdateNetworkRequest,
) -> Result<(), CoreError> {
    if let Some(cidr) = update.subnet.as_deref() {
        let (host_ip, prefix_len) = parse_ipv4_cidr(cidr)?;
        let ipv4 = ipv4_configuration(extra, "subnet")?;
        ipv4.insert("hostIpAddress".into(), Value::String(host_ip.to_string()));
        ipv4.insert("prefixLength".into(), Value::from(prefix_len));
    }

    if let Some(enable) = update.dhcp_enabled {
        let serving = extra
            .get("ipv4Configuration")
            .and_then(|ipv4| ipv4.get("dhcpConfiguration"))
            .and_then(|dhcp| dhcp.get("mode"))
            .and_then(Value::as_str)
            == Some("SERVER");
        if enable && !serving {
            let ipv4 = ipv4_configuration(extra, "dhcp_enabled")?;
            let mut dhcp = match ipv4.remove("dhcpConfiguration") {
                Some(Value::Object(dhcp)) => dhcp,
                _ => Map::new(),
            };
            dhcp.insert("mode".into(), Value::String("SERVER".into()));
            // Relay targets do not belong to a server configuration.
            dhcp.remove("dhcpServerIpAddresses");
            dhcp.entry("leaseTimeSeconds")
                .or_insert_with(|| Value::from(DEFAULT_DHCP_LEASE_SECS));
            dhcp.entry("pingConflictDetectionEnabled")
                .or_insert(Value::Bool(false));
            ipv4.insert("dhcpConfiguration".into(), Value::Object(dhcp));
        } else if !enable
            && serving
            && let Some(ipv4) = extra
                .get_mut("ipv4Configuration")
                .and_then(Value::as_object_mut)
        {
            ipv4.remove("dhcpConfiguration");
        }
    }

    if let Some(servers) = update
        .dhcp
        .as_ref()
        .and_then(|dhcp| dhcp.dns_servers.as_ref())
        && let Some(dhcp) = extra
            .get_mut("ipv4Configuration")
            .and_then(|ipv4| ipv4.get_mut("dhcpConfiguration"))
            .and_then(Value::as_object_mut)
    {
        dhcp.insert(
            "dnsServerIpAddressesOverride".into(),
            serde_json::json!(servers),
        );
    }

    Ok(())
}

/// The network's `ipv4Configuration`, which only gateway- and
/// switch-managed networks carry.
fn ipv4_configuration<'a>(
    extra: &'a mut HashMap<String, Value>,
    field: &str,
) -> Result<&'a mut Map<String, Value>, CoreError> {
    extra
        .get_mut("ipv4Configuration")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| CoreError::ValidationFailed {
            message: format!(
                "{field} needs a network with an IPv4 configuration (gateway or switch managed)"
            ),
        })
}

fn wifi_security_mode_name(mode: WifiSecurityMode) -> &'static str {
    match mode {
        WifiSecurityMode::Open => "OPEN",
        WifiSecurityMode::Wpa2Personal => "WPA2_PERSONAL",
        WifiSecurityMode::Wpa3Personal => "WPA3_PERSONAL",
        WifiSecurityMode::Wpa2Wpa3Personal => "WPA2_WPA3_PERSONAL",
        WifiSecurityMode::Wpa2Enterprise => "WPA2_ENTERPRISE",
        WifiSecurityMode::Wpa3Enterprise => "WPA3_ENTERPRISE",
        WifiSecurityMode::Wpa2Wpa3Enterprise => "WPA2_WPA3_ENTERPRISE",
    }
}

fn wifi_payload_name(name: &str, ssid: &str) -> String {
    if name.is_empty() {
        ssid.to_owned()
    } else {
        name.to_owned()
    }
}

fn wifi_frequency_values(frequencies: &[f32]) -> Vec<serde_json::Value> {
    frequencies
        .iter()
        .map(|frequency| {
            // Parse through the string representation to avoid f32→f64
            // precision artifacts (e.g. 2.4f32 → 2.4000000953674316f64).
            let s = format!("{frequency}");
            serde_json::Number::from_f64(s.parse::<f64>().unwrap_or(f64::from(*frequency)))
                .map_or(serde_json::Value::Null, serde_json::Value::Number)
        })
        .collect()
}

fn ensure_wifi_payload_defaults(
    body: &mut serde_json::Map<String, serde_json::Value>,
    broadcast_type: &str,
) {
    body.entry("clientIsolationEnabled")
        .or_insert(serde_json::Value::Bool(false));
    body.entry("multicastToUnicastConversionEnabled")
        .or_insert(serde_json::Value::Bool(false));
    body.entry("hideName")
        .or_insert(serde_json::Value::Bool(false));
    body.entry("uapsdEnabled")
        .or_insert(serde_json::Value::Bool(true));

    if broadcast_type == "STANDARD" {
        body.entry("broadcastingFrequenciesGHz")
            .or_insert_with(|| serde_json::Value::Array(wifi_frequency_values(&[2.4, 5.0])));
        body.entry("mloEnabled")
            .or_insert(serde_json::Value::Bool(false));
        body.entry("bandSteeringEnabled")
            .or_insert(serde_json::Value::Bool(false));
        body.entry("arpProxyEnabled")
            .or_insert(serde_json::Value::Bool(false));
        body.entry("bssTransitionEnabled")
            .or_insert(serde_json::Value::Bool(false));
        body.entry("advertiseDeviceName")
            .or_insert(serde_json::Value::Bool(false));
    }
}

pub(in super::super) fn build_create_wifi_broadcast_payload(
    req: &crate::command::CreateWifiBroadcastRequest,
) -> crate::integration_types::WifiBroadcastCreateUpdate {
    let broadcast_type = req
        .broadcast_type
        .clone()
        .unwrap_or_else(|| "STANDARD".into());

    let mut body = serde_json::Map::new();
    let mode_name = wifi_security_mode_name(req.security_mode);
    let mut security_configuration = serde_json::Map::new();
    security_configuration.insert("type".into(), serde_json::Value::String(mode_name.into()));
    if let Some(passphrase) = req.passphrase.clone() {
        security_configuration.insert("passphrase".into(), serde_json::Value::String(passphrase));
    }
    // WPA3 modes require a saeConfiguration object.
    if matches!(
        req.security_mode,
        WifiSecurityMode::Wpa3Personal | WifiSecurityMode::Wpa2Wpa3Personal
    ) {
        security_configuration
            .entry("saeConfiguration")
            .or_insert_with(|| {
                serde_json::json!({
                    "anticloggingThresholdSeconds": 5,
                    "syncTimeSeconds": 5
                })
            });
    }
    // WPA modes accept fastRoamingEnabled inside securityConfiguration,
    // but IoT_OPTIMIZED forbids it — only include when explicitly set.
    if let Some(fast_roaming) = req.fast_roaming
        && !matches!(req.security_mode, WifiSecurityMode::Open)
    {
        security_configuration
            .entry("fastRoamingEnabled")
            .or_insert(serde_json::Value::Bool(fast_roaming));
    }
    body.insert(
        "securityConfiguration".into(),
        serde_json::Value::Object(security_configuration),
    );

    if let Some(network_id) = &req.network_id {
        body.insert(
            "network".into(),
            serde_json::json!({ "type": "SPECIFIC", "networkId": network_id.to_string() }),
        );
    }
    body.insert("hideName".into(), serde_json::Value::Bool(req.hide_ssid));
    if req.band_steering {
        body.insert("bandSteeringEnabled".into(), serde_json::Value::Bool(true));
    }
    if let Some(fast_roaming) = req.fast_roaming
        && broadcast_type == "STANDARD"
    {
        body.insert(
            "bssTransitionEnabled".into(),
            serde_json::Value::Bool(fast_roaming),
        );
    }
    if let Some(frequencies) = req.frequencies_ghz.as_ref() {
        body.insert(
            "broadcastingFrequenciesGHz".into(),
            serde_json::Value::Array(wifi_frequency_values(frequencies)),
        );
    }
    ensure_wifi_payload_defaults(&mut body, &broadcast_type);

    crate::integration_types::WifiBroadcastCreateUpdate {
        name: wifi_payload_name(&req.name, &req.ssid),
        broadcast_type,
        enabled: req.enabled,
        body,
    }
}

pub(in super::super) fn build_update_wifi_broadcast_payload(
    existing: &crate::integration_types::WifiBroadcastDetailsResponse,
    update: &crate::command::UpdateWifiBroadcastRequest,
) -> crate::integration_types::WifiBroadcastCreateUpdate {
    let mut body: serde_json::Map<String, serde_json::Value> =
        existing.extra.clone().into_iter().collect();

    body.remove("ssid");
    body.remove("hideSsid");
    body.remove("bandSteering");
    body.remove("fastRoaming");
    body.remove("frequencies");

    if let Some(network) = existing.network.clone() {
        body.insert("network".into(), network);
    }
    if let Some(filter) = existing.broadcasting_device_filter.clone() {
        body.insert("broadcastingDeviceFilter".into(), filter);
    }
    if let Some(hidden) = update.hide_ssid {
        body.insert("hideName".into(), serde_json::Value::Bool(hidden));
    }

    let mut security_cfg = existing
        .security_configuration
        .as_object()
        .cloned()
        .unwrap_or_default();
    if let Some(mode) = update.security_mode {
        security_cfg.insert(
            "type".into(),
            serde_json::Value::String(wifi_security_mode_name(mode).into()),
        );
    }
    if let Some(passphrase) = update.passphrase.clone() {
        security_cfg.insert("passphrase".into(), serde_json::Value::String(passphrase));
    }
    body.insert(
        "securityConfiguration".into(),
        serde_json::Value::Object(security_cfg),
    );
    ensure_wifi_payload_defaults(&mut body, &existing.broadcast_type);

    crate::integration_types::WifiBroadcastCreateUpdate {
        name: update
            .name
            .clone()
            .or_else(|| update.ssid.clone())
            .unwrap_or_else(|| existing.name.clone()),
        broadcast_type: existing.broadcast_type.clone(),
        enabled: update.enabled.unwrap_or(existing.enabled),
        body,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{apply_ipv4_update, build_create_wifi_broadcast_payload, parse_ipv4_cidr};
    use crate::command::{CreateWifiBroadcastRequest, UpdateNetworkRequest};
    use crate::model::WifiSecurityMode;
    use serde_json::json;

    fn relay_network() -> HashMap<String, serde_json::Value> {
        HashMap::from([(
            "ipv4Configuration".to_owned(),
            json!({
                "hostIpAddress": "10.0.50.1",
                "prefixLength": 24,
                "dhcpConfiguration": {
                    "mode": "RELAY",
                    "dhcpServerIpAddresses": ["10.0.0.5"],
                },
            }),
        )])
    }

    #[test]
    fn dhcp_off_leaves_a_relay_untouched() {
        let mut extra = relay_network();
        let update = UpdateNetworkRequest {
            dhcp_enabled: Some(false),
            ..Default::default()
        };

        apply_ipv4_update(&mut extra, &update).expect("update applies");

        assert_eq!(extra, relay_network());
    }

    #[test]
    fn dhcp_on_turns_a_relay_into_a_server_with_dns() {
        let mut extra = relay_network();
        let update: UpdateNetworkRequest = serde_json::from_value(json!({
            "dhcp_enabled": true,
            "dhcp": {"dns_servers": ["1.1.1.1"]},
        }))
        .expect("update deserializes");

        apply_ipv4_update(&mut extra, &update).expect("update applies");

        assert_eq!(
            extra["ipv4Configuration"]["dhcpConfiguration"],
            json!({
                "mode": "SERVER",
                "leaseTimeSeconds": 86400,
                "pingConflictDetectionEnabled": false,
                "dnsServerIpAddressesOverride": ["1.1.1.1"],
            })
        );
    }

    #[test]
    fn dhcp_on_needs_an_ipv4_configuration() {
        let mut extra = HashMap::new();
        let update = UpdateNetworkRequest {
            dhcp_enabled: Some(true),
            ..Default::default()
        };

        assert!(apply_ipv4_update(&mut extra, &update).is_err());
        assert!(extra.is_empty());
    }

    #[test]
    fn parse_ipv4_cidr_accepts_valid_input() {
        let (host, prefix) = parse_ipv4_cidr("192.168.10.1/24").expect("valid CIDR");
        assert_eq!(host.to_string(), "192.168.10.1");
        assert_eq!(prefix, 24);
    }

    #[test]
    fn parse_ipv4_cidr_rejects_invalid_prefix() {
        assert!(parse_ipv4_cidr("192.168.10.1/40").is_err());
    }

    #[test]
    fn parse_ipv4_cidr_rejects_missing_prefix() {
        assert!(parse_ipv4_cidr("192.168.10.1").is_err());
    }

    #[test]
    fn wifi_create_payload_uses_integration_field_names() {
        let payload = build_create_wifi_broadcast_payload(&CreateWifiBroadcastRequest {
            name: "Main".into(),
            ssid: "Main".into(),
            security_mode: WifiSecurityMode::Wpa2Personal,
            passphrase: Some("supersecret".into()),
            enabled: true,
            network_id: None,
            hide_ssid: true,
            broadcast_type: Some("STANDARD".into()),
            frequencies_ghz: Some(vec![2.4, 5.0]),
            band_steering: true,
            fast_roaming: Some(true),
        });

        assert_eq!(payload.name, "Main");
        assert!(payload.body.get("ssid").is_none());
        assert_eq!(payload.body.get("hideName"), Some(&json!(true)));
        let frequencies = payload
            .body
            .get("broadcastingFrequenciesGHz")
            .and_then(serde_json::Value::as_array)
            .expect("frequencies array");
        assert_eq!(frequencies.len(), 2);
        assert_eq!(frequencies[1], json!(5.0));
        assert_eq!(payload.body.get("bandSteeringEnabled"), Some(&json!(true)));
        assert_eq!(payload.body.get("bssTransitionEnabled"), Some(&json!(true)));
    }

    #[test]
    fn wifi_frequency_values_avoid_f32_precision_artifacts() {
        let payload = build_create_wifi_broadcast_payload(&CreateWifiBroadcastRequest {
            name: "Test".into(),
            ssid: "Test".into(),
            security_mode: WifiSecurityMode::Wpa3Personal,
            passphrase: Some("secret".into()),
            enabled: true,
            network_id: None,
            hide_ssid: false,
            broadcast_type: Some("STANDARD".into()),
            frequencies_ghz: Some(vec![2.4, 5.0, 6.0]),
            band_steering: false,
            fast_roaming: None,
        });

        let json = serde_json::to_string(&payload).expect("wifi payload should serialize");
        assert!(json.contains("2.4"), "expected 2.4 not f64 artifact");
        assert!(!json.contains("2.400000"), "f32→f64 precision artifact");
    }
}
