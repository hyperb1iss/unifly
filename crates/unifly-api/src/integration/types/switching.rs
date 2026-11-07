//! Switching inventory returned by the Integration API.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// A site-wide link aggregation group, discriminated by its membership model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LagResponse {
    pub id: Uuid,
    pub metadata: Value,
    #[serde(flatten)]
    pub configuration: LagConfiguration,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "SCREAMING_SNAKE_CASE",
    rename_all_fields = "camelCase"
)]
pub enum LagConfiguration {
    Local {
        members: Vec<LagMember>,
    },
    MultiChassis {
        mc_lag_domain_id: Uuid,
        members: Vec<LagMember>,
    },
    SwitchStack {
        switch_stack_id: Uuid,
        members: Vec<SwitchStackLagMember>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LagMember {
    pub device_id: Uuid,
    pub port_idxs: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchStackLagMember {
    pub unit_id: i32,
    pub unit_mac_address: String,
    pub port_idxs: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McLagDomainResponse {
    pub id: Uuid,
    pub name: String,
    pub metadata: Value,
    pub peers: Vec<McLagPeer>,
    pub lags: Vec<McLag>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McLagPeer {
    pub device_id: Uuid,
    pub link_port_idxs: Vec<i32>,
    pub role: McLagPeerRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum McLagPeerRole {
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McLag {
    pub id: Uuid,
    pub metadata: Value,
    pub members: Vec<LagMember>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchStackResponse {
    pub id: Uuid,
    pub name: String,
    pub metadata: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<Uuid>,
    pub units: Vec<SwitchStackUnit>,
    pub lags: Vec<SwitchStackLag>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchStackUnit {
    pub id: i32,
    pub mac_address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<SwitchStackUnitRole>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SwitchStackUnitRole {
    ActiveController,
    BackupController,
    Member,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchStackLag {
    pub id: Uuid,
    pub metadata: Value,
    pub members: Vec<SwitchStackLagMember>,
}
