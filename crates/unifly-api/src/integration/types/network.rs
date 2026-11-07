use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Network overview — from `GET /v1/sites/{siteId}/networks`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkResponse {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub management: String,
    pub vlan_id: i32,
    #[serde(default)]
    pub default: bool,
    pub metadata: Value,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Network details — extends overview with additional fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkDetailsResponse {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub management: String,
    pub vlan_id: i32,
    #[serde(default)]
    pub default: bool,
    pub metadata: Value,
    pub dhcp_guarding: Option<Value>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Create or update a network.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkCreateUpdate {
    pub name: String,
    pub enabled: bool,
    pub management: String,
    pub vlan_id: i32,
    pub dhcp_guarding: Option<Value>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// References to resources using a network.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkReferencesResponse {
    #[serde(flatten)]
    pub fields: HashMap<String, Value>,
}

/// WiFi broadcast overview — from `GET /v1/sites/{siteId}/wifi`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiBroadcastResponse {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub broadcast_type: String,
    pub enabled: bool,
    pub security_configuration: Value,
    pub metadata: Value,
    pub network: Option<Value>,
    pub broadcasting_device_filter: Option<Value>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// WiFi broadcast details — extends overview with additional fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiBroadcastDetailsResponse {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub broadcast_type: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel2g_locked_to6: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dtim_period2g_locked_to3: Option<bool>,
    pub security_configuration: Value,
    pub metadata: Value,
    pub network: Option<Value>,
    pub broadcasting_device_filter: Option<Value>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Create or update a WiFi broadcast.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiBroadcastCreateUpdate {
    #[serde(default)]
    pub channel2g_locked_to6: bool,
    #[serde(default)]
    pub dtim_period2g_locked_to3: bool,
    pub name: String,
    #[serde(rename = "type")]
    pub broadcast_type: String,
    pub enabled: bool,
    #[serde(flatten)]
    pub body: serde_json::Map<String, Value>,
}
