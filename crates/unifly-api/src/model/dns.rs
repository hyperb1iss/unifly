// ── DNS domain types ──

use std::fmt;

use serde::{Deserialize, Serialize};

use super::common::{DataSource, EntityOrigin};
use super::entity_id::EntityId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DnsPolicyType {
    #[serde(alias = "A_RECORD", alias = "A")]
    ARecord,
    #[serde(alias = "AAAA_RECORD", alias = "AAAA")]
    AaaaRecord,
    #[serde(alias = "CNAME_RECORD", alias = "CNAME")]
    CnameRecord,
    #[serde(alias = "MX_RECORD", alias = "MX")]
    MxRecord,
    #[serde(alias = "TXT_RECORD", alias = "TXT")]
    TxtRecord,
    #[serde(alias = "SRV_RECORD", alias = "SRV")]
    SrvRecord,
    #[serde(alias = "FORWARD_DOMAIN", alias = "FORWARD", alias = "Forward")]
    ForwardDomain,
}

impl DnsPolicyType {
    /// The `type` token the Integration API uses on the wire.
    #[must_use]
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::ARecord => "A_RECORD",
            Self::AaaaRecord => "AAAA_RECORD",
            Self::CnameRecord => "CNAME_RECORD",
            Self::MxRecord => "MX_RECORD",
            Self::TxtRecord => "TXT_RECORD",
            Self::SrvRecord => "SRV_RECORD",
            Self::ForwardDomain => "FORWARD_DOMAIN",
        }
    }

    /// Parse a wire `type` token, accepting both the `*_RECORD` form and the
    /// bare short form some firmware variants emit.
    #[must_use]
    pub fn from_wire(name: &str) -> Option<Self> {
        match name {
            "A_RECORD" | "A" => Some(Self::ARecord),
            "AAAA_RECORD" | "AAAA" => Some(Self::AaaaRecord),
            "CNAME_RECORD" | "CNAME" => Some(Self::CnameRecord),
            "MX_RECORD" | "MX" => Some(Self::MxRecord),
            "TXT_RECORD" | "TXT" => Some(Self::TxtRecord),
            "SRV_RECORD" | "SRV" => Some(Self::SrvRecord),
            "FORWARD_DOMAIN" | "FORWARD" | "Forward" => Some(Self::ForwardDomain),
            _ => None,
        }
    }
}

/// A DNS policy carried a `type` token no known mapping recognizes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unrecognized DNS record type `{token}`")]
pub struct UnrecognizedDnsRecordType {
    pub token: String,
}

impl fmt::Display for DnsPolicyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::ARecord => "A",
            Self::AaaaRecord => "AAAA",
            Self::CnameRecord => "CNAME",
            Self::MxRecord => "MX",
            Self::TxtRecord => "TXT",
            Self::SrvRecord => "SRV",
            Self::ForwardDomain => "Forward",
        };
        f.write_str(name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsPolicy {
    pub id: EntityId,
    pub policy_type: DnsPolicyType,
    pub domain: String,
    pub value: String,
    pub ttl_seconds: Option<u32>,

    pub origin: Option<EntityOrigin>,

    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) source: DataSource,
}
