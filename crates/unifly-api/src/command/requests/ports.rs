//! Switch port profile apply request types.
//!
//! `ApplyPortsRequest` is the write-only DTO for `--from-file` payloads
//! and `devices ports export` output. It mirrors the controller's per-port
//! override shape but uses CLI-friendly field names and string values.
//! The handler resolves network names to Session `_id`s and converts
//! strings to the model enums (`PortMode`, `PoeMode`, `PortSpeedSetting`)
//! at PUT time.

use serde::{Deserialize, Serialize};

/// One switch's port configuration described as a single resource.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyPortsRequest {
    /// Per-port overrides to splice into the device's `port_overrides`.
    /// Ports not listed here keep their existing override unchanged
    /// (splice semantics — see the from-file plan for details).
    pub ports: Vec<ApplyPortEntry>,
}

/// Per-port override for [`ApplyPortsRequest`].
///
/// Splice semantics: every field except `index` is optional. Missing fields
/// leave the existing override value untouched. `tagged_network_ids:
/// Some([])` clears the tagged list (JSON Merge Patch); `None` leaves it
/// alone.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyPortEntry {
    /// 1-based port index (required). Matches `port_idx` on the wire.
    pub index: u32,

    /// User-facing port label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Operational mode. Accepts `"access"`, `"trunk"`, or `"mirror"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,

    /// Native (untagged) network — accepts a Session `_id` UUID or a
    /// network name. The handler resolves names against the cached
    /// network list. Aliased as `native_vlan` for ergonomics.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "native_vlan"
    )]
    pub native_network_id: Option<String>,

    /// Tagged networks for trunk ports. `Some(vec![])` clears the list;
    /// `None` leaves the existing list untouched. Each entry is a
    /// Session `_id` UUID or network name. Aliased as `tagged_vlans`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "tagged_vlans"
    )]
    pub tagged_network_ids: Option<Vec<String>>,

    /// Whether this trunk port carries all VLANs (the controller's
    /// "auto" tagged-VLAN mode). Mutually exclusive with
    /// `tagged_network_ids` — the handler validates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tagged_all: Option<bool>,

    /// PoE mode. Accepts `"on"`, `"off"`, `"auto"`, `"pasv24"`,
    /// `"passthrough"`. (`"on"` maps to `PoeMode::Auto` — same as the
    /// `--poe` CLI flag.)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poe: Option<String>,

    /// Configured link speed. Accepts `"auto"`, `"10"`, `"100"`,
    /// `"1000"`, `"2500"`, `"5000"`, `"10000"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<String>,

    /// If `true`, drop this port's entry from `port_overrides` entirely
    /// — returning the port to controller defaults. All other fields on
    /// this entry are ignored when `reset` is true.
    #[serde(default, skip_serializing_if = "is_false")]
    pub reset: bool,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

#[cfg(test)]
mod tests {
    use super::{ApplyPortEntry, ApplyPortsRequest};

    #[test]
    fn deserializes_minimal_entry() {
        let req: ApplyPortsRequest = serde_json::from_value(serde_json::json!({
            "ports": [
                { "index": 9, "name": "mac-mini" }
            ]
        }))
        .expect("deserialize");
        assert_eq!(req.ports.len(), 1);
        assert_eq!(req.ports[0].index, 9);
        assert_eq!(req.ports[0].name.as_deref(), Some("mac-mini"));
        assert!(req.ports[0].mode.is_none());
        assert!(!req.ports[0].reset);
    }

    #[test]
    fn deserializes_full_entry_via_aliases() {
        let req: ApplyPortsRequest = serde_json::from_value(serde_json::json!({
            "ports": [
                {
                    "index": 1,
                    "name": "uplink",
                    "mode": "trunk",
                    "native_vlan": "infra",
                    "tagged_vlans": ["personal", "iot"],
                    "tagged_all": false,
                    "poe": "auto",
                    "speed": "auto"
                }
            ]
        }))
        .expect("deserialize");
        let p = &req.ports[0];
        assert_eq!(p.native_network_id.as_deref(), Some("infra"));
        assert_eq!(
            p.tagged_network_ids.as_deref(),
            Some(&["personal".into(), "iot".into()][..])
        );
        assert_eq!(p.tagged_all, Some(false));
    }

    #[test]
    fn empty_tagged_array_round_trips_as_clear_intent() {
        // `tagged_network_ids: Some(vec![])` is the explicit "clear" case
        // (JSON Merge Patch semantics). Distinct from `None`.
        let req: ApplyPortsRequest = serde_json::from_value(serde_json::json!({
            "ports": [{ "index": 5, "tagged_vlans": [] }]
        }))
        .expect("deserialize");
        assert_eq!(req.ports[0].tagged_network_ids.as_deref(), Some(&[][..]));
    }

    #[test]
    fn reset_entry_serializes_compactly() {
        let req = ApplyPortsRequest {
            ports: vec![ApplyPortEntry {
                index: 5,
                name: None,
                mode: None,
                native_network_id: None,
                tagged_network_ids: None,
                tagged_all: None,
                poe: None,
                speed: None,
                reset: true,
            }],
        };
        let value = serde_json::to_value(&req).expect("serialize");
        assert_eq!(
            value,
            serde_json::json!({ "ports": [{ "index": 5, "reset": true }] })
        );
    }

    #[test]
    fn unknown_field_is_rejected() {
        let result: Result<ApplyPortsRequest, _> = serde_json::from_value(serde_json::json!({
            "ports": [{ "index": 1, "vlan": "infra" }]  // typo, not native_vlan
        }));
        let err = result.expect_err("expected unknown-field rejection");
        assert!(err.to_string().contains("unknown field"));
    }
}
