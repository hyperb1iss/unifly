//! Offline contract checks for the payloads shipped by `unifly skill`.
//!
//! These checks establish parser/model compatibility and field retention.
//! They do not validate placeholder values or controller acceptance. VPN
//! extension fields deliberately pass through untyped maps.
#![cfg(feature = "cli")]

use std::path::PathBuf;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use unifly::cli::commands::util::read_json_file;
use unifly::config::Config;
use unifly_api::command::{
    ApplyPortsRequest, CreateFirewallPolicyRequest, CreateNatPolicyRequest, CreateNetworkRequest,
    CreateRemoteAccessVpnServerRequest, CreateSiteToSiteVpnRequest, CreateVpnClientProfileRequest,
    CreateWifiBroadcastRequest, CreateWireGuardPeerRequest,
};

fn example_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("skill/examples")
        .join(name)
}

/// Serde commonly ignores unknown fields. Require every supplied field to
/// survive, including nested fields, while allowing defaults and aliases.
fn fields_preserved(input: &Value, output: &Value) -> bool {
    match input {
        Value::Object(fields) => fields.iter().all(|(key, value)| {
            let canonical = match key.as_str() {
                "native_vlan" => "native_network_id",
                "tagged_vlans" => "tagged_network_ids",
                _ => key,
            };
            output
                .get(canonical)
                .is_some_and(|retained| fields_preserved(value, retained))
        }),
        Value::Array(values) => output.as_array().is_some_and(|retained| {
            values.len() == retained.len()
                && values
                    .iter()
                    .zip(retained)
                    .all(|(a, b)| fields_preserved(a, b))
        }),
        _ => true,
    }
}

fn check_json<T: DeserializeOwned + Serialize>(name: &str) -> T {
    let input = read_json_file(&example_path(name)).expect("example must parse as CLI JSON/JSONC");
    let request: T =
        serde_json::from_value(input.clone()).expect("example must match request model");
    let output = serde_json::to_value(&request).expect("request must serialize");
    assert!(
        fields_preserved(&input, &output),
        "discarded fields in {name}"
    );
    request
}

#[test]
fn firewall_example_matches_request() {
    check_json::<CreateFirewallPolicyRequest>("firewall-block-iot.json");
}

#[test]
fn network_example_matches_request() {
    check_json::<CreateNetworkRequest>("network-iot-vlan.json");
}

#[test]
fn wifi_example_matches_request_and_requires_own_passphrase() {
    let request = check_json::<CreateWifiBroadcastRequest>("wifi-iot.json");
    assert_eq!(
        request.passphrase.as_deref(),
        Some("REPLACE_WITH_UNIQUE_WIFI_PASSPHRASE")
    );
}

#[test]
fn nat_examples_match_request() {
    check_json::<CreateNatPolicyRequest>("nat-masquerade.json");
    check_json::<CreateNatPolicyRequest>("nat-port-forward.json");
}

#[test]
fn vpn_examples_match_requests() {
    check_json::<CreateSiteToSiteVpnRequest>("vpn-site-to-site-ipsec.json");
    check_json::<CreateRemoteAccessVpnServerRequest>("vpn-remote-access-wireguard.json");
    check_json::<CreateVpnClientProfileRequest>("vpn-client-openvpn.json");
    check_json::<CreateWireGuardPeerRequest>("vpn-wireguard-peer.json");
}

#[test]
fn switch_example_accepts_comments_trailing_commas_and_network_aliases() {
    let request = check_json::<ApplyPortsRequest>("switch-ports.jsonc");
    assert_eq!(request.ports.len(), 4);
    let uplink = request.ports.first().expect("uplink port");
    assert_eq!(uplink.native_network_id.as_deref(), Some("infra"));
    assert_eq!(uplink.tagged_all, Some(true));
    assert!(request.ports.last().expect("reset port").reset);
}

#[test]
fn config_example_matches_config_model_without_discarding_fields() {
    let text = std::fs::read_to_string(example_path("config.toml")).expect("config example exists");
    let input: toml::Value = toml::from_str(&text).expect("valid TOML");
    let config: Config = toml::from_str(&text).expect("valid config model");
    assert!(fields_preserved(
        &serde_json::to_value(input).expect("TOML serializes"),
        &serde_json::to_value(&config).expect("config serializes"),
    ));
    assert_eq!(config.default_profile.as_deref(), Some("home"));
    assert_eq!(config.profiles.len(), 3);
}

#[test]
fn retention_check_detects_nested_discarded_fields() {
    let input = serde_json::json!({"ports": [{"index": 1, "misspelled": true}]});
    let output = serde_json::json!({"ports": [{"index": 1}]});
    assert!(!fields_preserved(&input, &output));
}
