use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Source endpoint of a firewall policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallPolicySource {
    pub zone_id: Option<Uuid>,
    #[serde(default)]
    pub traffic_filter: Option<SourceTrafficFilter>,
}

/// Destination endpoint of a firewall policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallPolicyDestination {
    pub zone_id: Option<Uuid>,
    #[serde(default)]
    pub traffic_filter: Option<DestTrafficFilter>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SourceTrafficFilter {
    #[serde(rename = "NETWORK")]
    Network {
        #[serde(rename = "networkFilter")]
        network_filter: NetworkFilter,
        #[serde(
            rename = "macAddressFilter",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        mac_address_filter: Option<MacAddressFilter>,
        #[serde(
            rename = "portFilter",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        port_filter: Option<PortFilter>,
    },
    #[serde(rename = "IP_ADDRESS")]
    IpAddress {
        #[serde(rename = "ipAddressFilter")]
        ip_address_filter: IpAddressFilter,
        #[serde(
            rename = "macAddressFilter",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        mac_address_filter: Option<MacAddressFilter>,
        #[serde(
            rename = "portFilter",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        port_filter: Option<PortFilter>,
    },
    #[serde(rename = "MAC_ADDRESS")]
    MacAddress {
        #[serde(rename = "macAddressFilter")]
        mac_address_filter: MacAddressFilter,
        #[serde(
            rename = "portFilter",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        port_filter: Option<PortFilter>,
    },
    #[serde(rename = "PORT")]
    Port {
        #[serde(rename = "portFilter")]
        port_filter: PortFilter,
    },
    #[serde(rename = "REGION")]
    Region {
        #[serde(rename = "regionFilter")]
        region_filter: RegionFilter,
        #[serde(
            rename = "portFilter",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        port_filter: Option<PortFilter>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DestTrafficFilter {
    #[serde(rename = "NETWORK")]
    Network {
        #[serde(rename = "networkFilter")]
        network_filter: NetworkFilter,
        #[serde(
            rename = "portFilter",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        port_filter: Option<PortFilter>,
    },
    #[serde(rename = "IP_ADDRESS")]
    IpAddress {
        #[serde(rename = "ipAddressFilter")]
        ip_address_filter: IpAddressFilter,
        #[serde(
            rename = "portFilter",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        port_filter: Option<PortFilter>,
    },
    #[serde(rename = "PORT")]
    Port {
        #[serde(rename = "portFilter")]
        port_filter: PortFilter,
    },
    #[serde(rename = "REGION")]
    Region {
        #[serde(rename = "regionFilter")]
        region_filter: RegionFilter,
        #[serde(
            rename = "portFilter",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        port_filter: Option<PortFilter>,
    },
    #[serde(rename = "APPLICATION")]
    Application {
        #[serde(rename = "applicationFilter")]
        application_filter: ApplicationFilter,
        #[serde(
            rename = "portFilter",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        port_filter: Option<PortFilter>,
    },
    #[serde(rename = "APPLICATION_CATEGORY")]
    ApplicationCategory {
        #[serde(rename = "applicationCategoryFilter")]
        application_category_filter: ApplicationCategoryFilter,
        #[serde(
            rename = "portFilter",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        port_filter: Option<PortFilter>,
    },
    #[serde(rename = "DOMAIN")]
    Domain {
        #[serde(rename = "domainFilter")]
        domain_filter: DomainFilter,
        #[serde(
            rename = "portFilter",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        port_filter: Option<PortFilter>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkFilter {
    pub network_ids: Vec<Uuid>,
    #[serde(default)]
    pub match_opposite: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum IpAddressFilter {
    #[serde(rename = "IP_ADDRESSES", alias = "SPECIFIC")]
    Specific {
        #[serde(default)]
        items: Vec<IpAddressItem>,
        #[serde(default, rename = "matchOpposite")]
        match_opposite: bool,
    },
    #[serde(rename = "TRAFFIC_MATCHING_LIST")]
    TrafficMatchingList {
        #[serde(rename = "trafficMatchingListId")]
        traffic_matching_list_id: Uuid,
        #[serde(default, rename = "matchOpposite")]
        match_opposite: bool,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum IpAddressItem {
    #[serde(rename = "IP_ADDRESS")]
    Address { value: String },
    #[serde(rename = "RANGE")]
    Range { start: String, stop: String },
    #[serde(rename = "SUBNET")]
    Subnet { value: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PortFilter {
    #[serde(rename = "PORTS", alias = "VALUE")]
    Ports {
        #[serde(default)]
        items: Vec<PortItem>,
        #[serde(default, rename = "matchOpposite")]
        match_opposite: bool,
    },
    #[serde(rename = "TRAFFIC_MATCHING_LIST")]
    TrafficMatchingList {
        #[serde(rename = "trafficMatchingListId")]
        traffic_matching_list_id: Uuid,
        #[serde(default, rename = "matchOpposite")]
        match_opposite: bool,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PortItem {
    #[serde(rename = "PORT_NUMBER")]
    Number {
        #[serde(deserialize_with = "deserialize_port_value")]
        value: String,
    },
    #[serde(rename = "PORT_NUMBER_RANGE", alias = "PORT_RANGE")]
    Range {
        #[serde(rename = "startPort", deserialize_with = "deserialize_port_value")]
        start_port: String,
        #[serde(rename = "endPort", deserialize_with = "deserialize_port_value")]
        end_port: String,
    },
    #[serde(other)]
    Unknown,
}

fn deserialize_port_value<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct PortValueVisitor;

    impl serde::de::Visitor<'_> for PortValueVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a port number as string or integer")
        }

        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<String, E> {
            Ok(value.to_string())
        }

        fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<String, E> {
            Ok(value.to_string())
        }

        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<String, E> {
            Ok(value.to_string())
        }
    }

    deserializer.deserialize_any(PortValueVisitor)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacAddressFilter {
    pub mac_addresses: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationFilter {
    pub application_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationCategoryFilter {
    pub application_category_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegionFilter {
    pub regions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DomainFilter {
    #[serde(rename = "SPECIFIC")]
    Specific { domains: Vec<String> },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallPolicyResponse {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub enabled: bool,
    pub action: Value,
    pub ip_protocol_scope: Option<Value>,
    #[serde(default)]
    pub logging_enabled: bool,
    pub metadata: Option<Value>,
    #[serde(default)]
    pub source: Option<FirewallPolicySource>,
    #[serde(default)]
    pub destination: Option<FirewallPolicyDestination>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallPolicyCreateUpdate {
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub action: Value,
    pub source: Value,
    pub destination: Value,
    pub ip_protocol_scope: Value,
    pub logging_enabled: bool,
    pub ipsec_filter: Option<String>,
    pub schedule: Option<Value>,
    pub connection_state_filter: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallPolicyPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging_enabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallPolicyOrderingEnvelope {
    pub ordered_firewall_policy_ids: FirewallPolicyOrdering,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallPolicyOrdering {
    pub before_system_defined: Vec<Uuid>,
    pub after_system_defined: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallZoneResponse {
    pub id: Uuid,
    pub name: String,
    pub network_ids: Vec<Uuid>,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallZoneCreateUpdate {
    pub name: String,
    pub network_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AclRuleResponse {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub rule_type: String,
    pub action: String,
    pub enabled: bool,
    pub index: i32,
    pub description: Option<String>,
    pub source_filter: Option<Value>,
    pub destination_filter: Option<Value>,
    pub enforcing_device_filter: Option<Value>,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AclRuleCreateUpdate {
    pub name: String,
    #[serde(rename = "type")]
    pub rule_type: String,
    pub action: String,
    pub enabled: bool,
    pub description: Option<String>,
    pub source_filter: Option<Value>,
    pub destination_filter: Option<Value>,
    pub enforcing_device_filter: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AclRuleOrdering {
    pub ordered_acl_rule_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsPolicyResponse {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub policy_type: String,
    pub enabled: bool,
    pub domain: Option<String>,
    pub metadata: Value,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsPolicyCreateUpdate {
    #[serde(rename = "type")]
    pub policy_type: String,
    pub enabled: bool,
    #[serde(flatten)]
    pub fields: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficMatchingListResponse {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub list_type: String,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficMatchingListCreateUpdate {
    pub name: String,
    #[serde(rename = "type")]
    pub list_type: String,
    #[serde(flatten)]
    pub fields: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherResponse {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub created_at: String,
    pub activated_at: Option<String>,
    pub expires_at: Option<String>,
    pub expired: bool,
    pub time_limit_minutes: i64,
    pub authorized_guest_count: i64,
    pub authorized_guest_limit: Option<i64>,
    pub data_usage_limit_m_bytes: Option<i64>,
    pub rx_rate_limit_kbps: Option<i64>,
    pub tx_rate_limit_kbps: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherCreateRequest {
    pub name: String,
    pub count: Option<i32>,
    pub time_limit_minutes: i64,
    pub authorized_guest_limit: Option<i64>,
    pub data_usage_limit_m_bytes: Option<i64>,
    pub rx_rate_limit_kbps: Option<i64>,
    pub tx_rate_limit_kbps: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherDeletionResults {
    #[serde(flatten)]
    pub fields: HashMap<String, Value>,
}

// ── NAT Policies ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NatPolicyResponse {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub enabled: bool,
    #[serde(rename = "type")]
    pub nat_type: String,
    #[serde(default)]
    pub interface_id: Option<Uuid>,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub source: Option<Value>,
    #[serde(default)]
    pub destination: Option<Value>,
    #[serde(default)]
    pub translated_address: Option<String>,
    #[serde(default)]
    pub translated_port: Option<String>,
    pub metadata: Option<Value>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NatPolicyCreateUpdate {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub enabled: bool,
    #[serde(rename = "type")]
    pub nat_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translated_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translated_port: Option<String>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{IpAddressFilter, PortFilter, PortItem};

    #[test]
    fn port_filter_accepts_value_alias_and_numeric_ports() {
        let filter: PortFilter = serde_json::from_value(serde_json::json!({
            "type": "VALUE",
            "items": [
                { "type": "PORT_NUMBER", "value": 443 },
                { "type": "PORT_RANGE", "startPort": 8000, "endPort": "9000" }
            ],
            "matchOpposite": true
        }))
        .unwrap();

        match filter {
            PortFilter::Ports {
                items,
                match_opposite,
            } => {
                assert!(match_opposite);
                assert_eq!(
                    items,
                    vec![
                        PortItem::Number {
                            value: "443".into()
                        },
                        PortItem::Range {
                            start_port: "8000".into(),
                            end_port: "9000".into()
                        },
                    ]
                );
            }
            other => panic!("unexpected filter: {other:?}"),
        }
    }

    #[test]
    fn ip_address_filter_accepts_specific_alias() {
        let filter: IpAddressFilter = serde_json::from_value(serde_json::json!({
            "type": "SPECIFIC",
            "items": [{ "type": "IP_ADDRESS", "value": "192.168.1.10" }],
            "matchOpposite": false
        }))
        .unwrap();

        assert!(matches!(filter, IpAddressFilter::Specific { .. }));
    }

    #[test]
    fn port_item_range_serializes_as_port_number_range() {
        let item = PortItem::Range {
            start_port: "49152".into(),
            end_port: "65535".into(),
        };
        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(
            json.get("type").and_then(serde_json::Value::as_str),
            Some("PORT_NUMBER_RANGE"),
            "Range must serialize as PORT_NUMBER_RANGE, not PORT_RANGE"
        );
        assert_eq!(
            json.get("startPort").and_then(serde_json::Value::as_str),
            Some("49152")
        );
        assert_eq!(
            json.get("endPort").and_then(serde_json::Value::as_str),
            Some("65535")
        );
    }

    #[test]
    fn port_item_range_deserializes_from_port_range_alias() {
        let item: PortItem = serde_json::from_value(serde_json::json!({
            "type": "PORT_RANGE",
            "startPort": "8000",
            "endPort": "9000"
        }))
        .unwrap();
        assert!(
            matches!(item, PortItem::Range { .. }),
            "PORT_RANGE alias must still deserialize"
        );
    }
}
