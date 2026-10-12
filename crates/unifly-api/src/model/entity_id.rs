// ── Core identity types ──
//
// EntityId and MacAddress form the foundation of every domain type.
// They unify UUID-based (Integration API) and string-based (Session API)
// identifiers behind a single ergonomic interface.

use serde::{Deserialize, Serialize};
use std::error::Error as StdError;
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

// ── EntityId ────────────────────────────────────────────────────────

/// Canonical identifier for any UniFi entity.
///
/// Transparently wraps either a UUID (Integration API) or a session
/// MongoDB ObjectId string (Session API). Consumers never care which.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EntityId {
    Uuid(Uuid),
    Legacy(String),
}

impl EntityId {
    pub fn as_uuid(&self) -> Option<&Uuid> {
        match self {
            Self::Uuid(u) => Some(u),
            Self::Legacy(_) => None,
        }
    }

    pub fn as_legacy(&self) -> Option<&str> {
        match self {
            Self::Legacy(s) => Some(s),
            Self::Uuid(_) => None,
        }
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uuid(u) => write!(f, "{u}"),
            Self::Legacy(s) => write!(f, "{s}"),
        }
    }
}

impl FromStr for EntityId {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from(s.to_owned()))
    }
}

impl From<Uuid> for EntityId {
    fn from(u: Uuid) -> Self {
        Self::Uuid(u)
    }
}

impl From<String> for EntityId {
    fn from(s: String) -> Self {
        match Uuid::parse_str(&s) {
            Ok(u) => Self::Uuid(u),
            Err(_) => Self::Legacy(s),
        }
    }
}

impl From<&str> for EntityId {
    fn from(s: &str) -> Self {
        Self::from(s.to_owned())
    }
}

// ── MacAddress ──────────────────────────────────────────────────────

/// MAC address, normalized to lowercase colon-separated format (aa:bb:cc:dd:ee:ff).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MacAddress(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseMacAddressError;

impl MacAddress {
    /// Create a normalized MAC address from any common format.
    /// Accepts colon-separated, dash-separated, dotted (`aabb.ccdd.eeff`),
    /// or bare hex, in any case.
    ///
    /// For strict validation, use [`MacAddress::try_new`] or `str::parse`.
    pub fn new(raw: impl AsRef<str>) -> Self {
        let raw = raw.as_ref();
        Self::try_new(raw).unwrap_or_else(|_| Self(raw.trim().to_lowercase().replace('-', ":")))
    }

    pub fn try_new(raw: impl AsRef<str>) -> Result<Self, ParseMacAddressError> {
        normalize_mac(raw.as_ref())
            .map(Self)
            .ok_or(ParseMacAddressError)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn normalize_mac(raw: &str) -> Option<String> {
    let raw = raw.trim();
    // Dotted notation is exactly three groups of four hex digits, so an
    // IPv4 address (four groups) can never pass for a MAC.
    if raw.contains('.') {
        let groups: Vec<&str> = raw.split('.').collect();
        let well_formed = groups.len() == 3
            && groups
                .iter()
                .all(|group| group.len() == 4 && group.chars().all(|ch| ch.is_ascii_hexdigit()));
        if !well_formed {
            return None;
        }
        return normalize_mac(&groups.concat());
    }
    let compact = raw
        .chars()
        .filter(|ch| *ch != ':' && *ch != '-')
        .collect::<String>()
        .to_ascii_lowercase();

    if compact.len() != 12 || !compact.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }

    let bytes = compact.as_bytes();
    let mut normalized = String::with_capacity(17);
    for (idx, &[hi, lo]) in bytes.as_chunks::<2>().0.iter().enumerate() {
        if idx > 0 {
            normalized.push(':');
        }
        normalized.push(char::from(hi));
        normalized.push(char::from(lo));
    }

    Some(normalized)
}

impl fmt::Display for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for ParseMacAddressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "expected a 12-digit hexadecimal MAC address")
    }
}

impl StdError for ParseMacAddressError {}

impl FromStr for MacAddress {
    type Err = ParseMacAddressError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_new(s)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn entity_id_from_uuid_string() {
        let id = EntityId::from("550e8400-e29b-41d4-a716-446655440000".to_owned());
        assert!(id.as_uuid().is_some());
    }

    #[test]
    fn entity_id_from_legacy_string() {
        let id = EntityId::from("507f1f77bcf86cd799439011".to_owned());
        assert!(id.as_legacy().is_some());
    }

    #[test]
    fn entity_id_display() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let id = EntityId::Uuid(uuid);
        assert_eq!(id.to_string(), "550e8400-e29b-41d4-a716-446655440000");
    }

    #[test]
    fn entity_id_from_str() {
        let id: EntityId = "507f1f77bcf86cd799439011".parse().unwrap();
        assert!(id.as_legacy().is_some());
    }

    #[test]
    fn mac_address_normalizes_dashes() {
        let mac = MacAddress::new("AA-BB-CC-DD-EE-FF");
        assert_eq!(mac.as_str(), "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn mac_address_normalizes_case() {
        let mac = MacAddress::new("AA:BB:CC:DD:EE:FF");
        assert_eq!(mac.as_str(), "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn mac_address_from_str() {
        let mac: MacAddress = "AA-BB-CC-DD-EE-FF".parse().unwrap();
        assert_eq!(mac.to_string(), "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn mac_address_normalizes_bare_hex() {
        let mac = MacAddress::new("AABBCCDDEEFF");
        assert_eq!(mac.as_str(), "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn mac_address_normalizes_dotted_groups() {
        let mac: MacAddress = "AABB.CCDD.EEFF".parse().unwrap();
        assert_eq!(mac.as_str(), "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn mac_address_parse_rejects_invalid_input() {
        assert!("not-a-mac".parse::<MacAddress>().is_err());
        assert!("AA:BB:CC:DD:EE".parse::<MacAddress>().is_err());
        assert!("aabb.ccdd.ee:ff".parse::<MacAddress>().is_err());
        // Twelve digits once the dots go, but an IP address, not a MAC.
        assert!("100.100.100.100".parse::<MacAddress>().is_err());
    }
}
