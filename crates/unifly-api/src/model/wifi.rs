// ── WiFi broadcast domain types ──

use serde::{Deserialize, Serialize};

use super::common::{DataSource, EntityOrigin};
use super::entity_id::EntityId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WifiBroadcastType {
    Standard,
    IotOptimized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum WifiSecurityMode {
    Open,
    Wpa2Personal,
    Wpa3Personal,
    Wpa2Wpa3Personal,
    Wpa2Enterprise,
    Wpa3Enterprise,
    Wpa2Wpa3Enterprise,
}

/// The canonical WiFi Broadcast (SSID/WLAN) type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct WifiBroadcast {
    pub id: EntityId,
    pub name: String,
    pub enabled: bool,
    pub broadcast_type: WifiBroadcastType,
    pub security: WifiSecurityMode,

    // Network association
    pub network_id: Option<EntityId>,

    // Frequencies
    pub frequencies_ghz: Vec<f32>,

    // Features. The controller's list summary leaves these out, so each is
    // `None` until the broadcast's detail has been read: unknown, not off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_isolation: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub band_steering: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mlo_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast_roaming: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel2g_locked_to6: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dtim_period2g_locked_to3: Option<bool>,

    /// How many networks have a preshared key of their own on this SSID.
    /// Above zero the SSID has no single passphrase to change.
    #[serde(default)]
    pub preshared_key_count: usize,

    // Hotspot
    pub hotspot_enabled: bool,

    pub origin: Option<EntityOrigin>,

    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) source: DataSource,
}

impl WifiBroadcast {
    /// This broadcast as the controller's list returns it: without the
    /// fields only its detail carries.
    #[must_use]
    pub fn summary(&self) -> Self {
        Self {
            hidden: None,
            client_isolation: None,
            band_steering: None,
            mlo_enabled: None,
            fast_roaming: None,
            channel2g_locked_to6: None,
            dtim_period2g_locked_to3: None,
            ..self.clone()
        }
    }
}
