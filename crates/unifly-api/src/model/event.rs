// ── Event and alarm domain types ──

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::common::DataSource;
use super::entity_id::{EntityId, MacAddress};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EventCategory {
    Device,
    Client,
    Network,
    System,
    Admin,
    Firewall,
    Vpn,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EventSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

/// Unified event from WebSocket or Session API event log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: Option<EntityId>,
    pub timestamp: DateTime<Utc>,
    pub category: EventCategory,
    pub severity: EventSeverity,
    pub event_type: String,
    pub message: String,

    // Related entities
    pub device_mac: Option<MacAddress>,
    pub client_mac: Option<MacAddress>,
    pub site_id: Option<EntityId>,
    /// Wireless network named by the event, retained for consistent redaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssid: Option<String>,

    // Raw data for consumers that need it
    pub raw_key: Option<String>,

    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) source: DataSource,
}

/// Alarm (a persistent event requiring acknowledgment).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alarm {
    pub id: EntityId,
    pub timestamp: DateTime<Utc>,
    pub category: EventCategory,
    pub severity: EventSeverity,
    pub message: String,
    pub archived: bool,

    pub device_mac: Option<MacAddress>,
    pub site_id: Option<EntityId>,
}
