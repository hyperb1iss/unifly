use crate::integration_types;
use crate::model::common::DataSource;
use crate::model::entity_id::EntityId;
use crate::model::wifi::{WifiBroadcast, WifiBroadcastType, WifiSecurityMode};

use super::helpers::{extra_frequencies, origin_from_metadata};

impl From<integration_types::WifiBroadcastResponse> for WifiBroadcast {
    fn from(w: integration_types::WifiBroadcastResponse) -> Self {
        let broadcast_type = match w.broadcast_type.as_str() {
            "IOT_OPTIMIZED" => WifiBroadcastType::IotOptimized,
            _ => WifiBroadcastType::Standard,
        };

        let security = w
            .security_configuration
            .get("type")
            .or_else(|| w.security_configuration.get("mode"))
            .and_then(|v| v.as_str())
            .map_or(WifiSecurityMode::Open, |mode| match mode {
                "WPA2_PERSONAL" => WifiSecurityMode::Wpa2Personal,
                "WPA3_PERSONAL" => WifiSecurityMode::Wpa3Personal,
                "WPA2_WPA3_PERSONAL" => WifiSecurityMode::Wpa2Wpa3Personal,
                "WPA2_ENTERPRISE" => WifiSecurityMode::Wpa2Enterprise,
                "WPA3_ENTERPRISE" => WifiSecurityMode::Wpa3Enterprise,
                "WPA2_WPA3_ENTERPRISE" => WifiSecurityMode::Wpa2Wpa3Enterprise,
                _ => WifiSecurityMode::Open,
            });

        // The list names the networks that have a key of their own; the
        // detail carries the keys themselves.
        let preshared_key_count = ["presharedKeyNetworkIds", "presharedKeys"]
            .into_iter()
            .filter_map(|key| w.security_configuration.get(key)?.as_array())
            .map(Vec::len)
            .max()
            .unwrap_or(0);
        // A field the response leaves out is unknown, not off: the list
        // summary carries none of these.
        let flag = |key: &str| w.extra.get(key).and_then(serde_json::Value::as_bool);

        WifiBroadcast {
            id: EntityId::Uuid(w.id),
            name: w.name,
            enabled: w.enabled,
            broadcast_type,
            security,
            network_id: w
                .network
                .as_ref()
                .and_then(|v| v.get("networkId").or_else(|| v.get("id")))
                .and_then(|v| v.as_str())
                .and_then(|s| uuid::Uuid::parse_str(s).ok())
                .map(EntityId::Uuid),
            frequencies_ghz: extra_frequencies(&w.extra, "broadcastingFrequenciesGHz"),
            hidden: flag("hideName"),
            client_isolation: flag("clientIsolationEnabled"),
            band_steering: flag("bandSteeringEnabled"),
            mlo_enabled: flag("mloEnabled"),
            fast_roaming: flag("bssTransitionEnabled"),
            channel2g_locked_to6: flag("channel2gLockedTo6"),
            dtim_period2g_locked_to3: flag("dtimPeriod2gLockedTo3"),
            preshared_key_count,
            hotspot_enabled: w.extra.contains_key("hotspotConfiguration"),
            origin: origin_from_metadata(&w.metadata),
            source: DataSource::IntegrationApi,
        }
    }
}

impl From<integration_types::WifiBroadcastDetailsResponse> for WifiBroadcast {
    fn from(mut w: integration_types::WifiBroadcastDetailsResponse) -> Self {
        if let Some(value) = w.channel2g_locked_to6 {
            w.extra.insert("channel2gLockedTo6".into(), value.into());
        }
        if let Some(value) = w.dtim_period2g_locked_to3 {
            w.extra.insert("dtimPeriod2gLockedTo3".into(), value.into());
        }
        let overview = integration_types::WifiBroadcastResponse {
            id: w.id,
            name: w.name,
            broadcast_type: w.broadcast_type,
            enabled: w.enabled,
            security_configuration: w.security_configuration,
            metadata: w.metadata,
            network: w.network,
            broadcasting_device_filter: w.broadcasting_device_filter,
            extra: w.extra,
        };
        Self::from(overview)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use serde_json::json;

    #[test]
    fn details_preserve_unknown_and_explicit_radio_lock_values() {
        for channel in [None, Some(false), Some(true)] {
            for dtim in [None, Some(false), Some(true)] {
                let mut raw = json!({"id": uuid::Uuid::nil(), "name": "WiFi",
                    "type": "STANDARD", "enabled": true, "metadata": {},
                    "securityConfiguration": {"type": "OPEN"}});
                if let Some(value) = channel {
                    raw["channel2gLockedTo6"] = json!(value);
                }
                if let Some(value) = dtim {
                    raw["dtimPeriod2gLockedTo3"] = json!(value);
                }
                let details: integration_types::WifiBroadcastDetailsResponse =
                    serde_json::from_value(raw.clone()).expect("details");
                let overview: integration_types::WifiBroadcastResponse =
                    serde_json::from_value(raw).expect("overview");
                let details = WifiBroadcast::from(details);
                let overview = WifiBroadcast::from(overview);
                assert_eq!(details.channel2g_locked_to6, channel);
                assert_eq!(details.dtim_period2g_locked_to3, dtim);
                assert_eq!(details.channel2g_locked_to6, overview.channel2g_locked_to6);
                assert_eq!(
                    details.dtim_period2g_locked_to3,
                    overview.dtim_period2g_locked_to3
                );
            }
        }
    }

    #[test]
    fn integration_wifi_broadcast_preserves_standard_fields() {
        let response = integration_types::WifiBroadcastResponse {
            id: uuid::Uuid::nil(),
            name: "Main".into(),
            broadcast_type: "STANDARD".into(),
            enabled: true,
            security_configuration: json!({"mode": "WPA2_PERSONAL"}),
            metadata: json!({"origin": "USER"}),
            network: Some(json!({"id": uuid::Uuid::nil().to_string()})),
            broadcasting_device_filter: None,
            extra: HashMap::from([
                ("broadcastingFrequenciesGHz".into(), json!([2.4, 5.0])),
                ("hideName".into(), json!(true)),
                ("clientIsolationEnabled".into(), json!(true)),
                ("bandSteeringEnabled".into(), json!(true)),
                ("mloEnabled".into(), json!(false)),
                ("bssTransitionEnabled".into(), json!(true)),
                (
                    "hotspotConfiguration".into(),
                    json!({"type": "CAPTIVE_PORTAL"}),
                ),
            ]),
        };

        let wifi = WifiBroadcast::from(response);
        assert_eq!(wifi.frequencies_ghz.len(), 2);
        assert!((wifi.frequencies_ghz[0] - 2.4).abs() < f32::EPSILON);
        assert!((wifi.frequencies_ghz[1] - 5.0).abs() < f32::EPSILON);
        assert_eq!(wifi.hidden, Some(true));
        assert_eq!(wifi.client_isolation, Some(true));
        assert_eq!(wifi.band_steering, Some(true));
        assert_eq!(wifi.mlo_enabled, Some(false));
        assert_eq!(wifi.fast_roaming, Some(true));
        assert!(wifi.hotspot_enabled);
    }

    /// The list summary, with the fields the 11.0.97 spec gives it.
    fn overview(security: &serde_json::Value) -> integration_types::WifiBroadcastResponse {
        serde_json::from_value(json!({
            "id": uuid::Uuid::nil(), "name": "Main", "type": "STANDARD",
            "enabled": true, "metadata": {"origin": "USER_DEFINED"},
            "network": {"type": "NATIVE"}, "securityConfiguration": security,
            "broadcastingFrequenciesGHz": [2.4, 5]
        }))
        .expect("overview")
    }

    #[test]
    fn a_list_summary_leaves_the_detail_only_fields_unknown() {
        let wifi = WifiBroadcast::from(overview(&json!({"type": "WPA2_PERSONAL"})));
        assert_eq!(wifi.hidden, None);
        assert_eq!(wifi.client_isolation, None);
        assert_eq!(wifi.band_steering, None);
        assert_eq!(wifi.mlo_enabled, None);
        assert_eq!(wifi.fast_roaming, None);
        assert_eq!(wifi.preshared_key_count, 0);
        assert_eq!(
            wifi.frequencies_ghz.len(),
            2,
            "the summary does carry bands"
        );

        let listed = serde_json::to_value(&wifi).expect("serializes");
        assert!(
            listed.get("hidden").is_none(),
            "unknown is left out: {listed}"
        );
        assert!(listed.get("fast_roaming").is_none(), "{listed}");
    }

    #[test]
    fn per_network_keys_are_counted_from_the_summary_and_the_detail() {
        let listed = WifiBroadcast::from(overview(&json!({
            "type": "WPA2_PERSONAL",
            "presharedKeyNetworkIds": [uuid::Uuid::from_u128(1), uuid::Uuid::from_u128(2)]
        })));
        assert_eq!(listed.preshared_key_count, 2);

        let detail: integration_types::WifiBroadcastDetailsResponse =
            serde_json::from_value(json!({
                "id": uuid::Uuid::nil(), "name": "Main", "type": "STANDARD",
                "enabled": true, "metadata": {}, "hideName": false,
                "securityConfiguration": {"type": "WPA2_PERSONAL", "presharedKeys": [
                    {"network": {"type": "NATIVE"}, "passphrase": "first-key-1"},
                    {"network": {"type": "SPECIFIC", "networkId": uuid::Uuid::from_u128(2)},
                     "passphrase": "second-key-2"},
                    {"network": {"type": "SPECIFIC", "networkId": uuid::Uuid::from_u128(3)},
                     "passphrase": "third-key-3"}
                ]}
            }))
            .expect("detail");
        let detail = WifiBroadcast::from(detail);
        assert_eq!(detail.preshared_key_count, 3);
        assert_eq!(detail.hidden, Some(false));
        let shown = serde_json::to_string(&detail).expect("serializes");
        assert!(!shown.contains("first-key-1"), "no key leaves the model");
    }
}
