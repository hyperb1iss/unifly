//! Switch port profile queries and updates.
//!
//! Port VLAN configuration is a Session-API-only surface (the Integration
//! API does not expose `port_table` / `port_overrides`). This module layers
//! a normalized `PortProfile` view over the raw `stat/device` payload and
//! provides a helper for merging a single port's overrides while preserving
//! every other override on the device.

use std::collections::HashMap;

use serde_json::{Map, Value, json};
use tracing::debug;

use super::Controller;
use super::support::require_session;
use crate::core_error::CoreError;
use crate::model::{
    MacAddress, PoeMode, PortMode, PortProfile, PortSpeedSetting, PortState, StpState,
};

/// Desired update to a single port's profile, as supplied by the CLI or a
/// future TUI editor. Every field is optional -- unset fields leave the
/// existing override value untouched.
#[derive(Debug, Default, Clone)]
pub struct PortProfileUpdate {
    /// User-facing port label.
    pub name: Option<String>,
    /// Operational mode (access / trunk / mirror).
    pub mode: Option<PortMode>,
    /// Session `_id` of the native (untagged) network.
    pub native_network_id: Option<String>,
    /// Session `_id`s of explicitly tagged networks. `Some(vec![])` clears
    /// the tagged list; `None` leaves it untouched.
    pub tagged_network_ids: Option<Vec<String>>,
    /// PoE configuration.
    pub poe_mode: Option<PoeMode>,
    /// Configured link speed.
    pub speed_setting: Option<PortSpeedSetting>,
}

impl Controller {
    /// List normalized port profiles for an adopted switch or gateway with
    /// ports.
    ///
    /// Requires Session API access. Returns ports sorted by port index.
    pub async fn list_device_ports(
        &self,
        device_mac: &MacAddress,
    ) -> Result<Vec<PortProfile>, CoreError> {
        let guard = self.inner.session_client.lock().await;
        let session = require_session(guard.as_ref())?;

        let device = session
            .get_device(device_mac.as_str())
            .await?
            .ok_or_else(|| CoreError::DeviceNotFound {
                identifier: device_mac.to_string(),
            })?;

        let network_lookup = build_network_lookup(&session.list_network_conf().await?);

        let overrides = device
            .extra
            .get("port_overrides")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let port_table = device
            .extra
            .get("port_table")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        // Owned map avoids coupling the lookup's lifetime to the `overrides`
        // Vec — the fallback branch below consumes `overrides` directly.
        let override_map: HashMap<u32, Value> = overrides
            .iter()
            .filter_map(|o| port_idx(o).map(|idx| (idx, o.clone())))
            .collect();

        // If the switch reports no port_table, fall back to overrides as the
        // source of truth (rare for adopted switches but happens mid-provision).
        let mut profiles: Vec<PortProfile> = if port_table.is_empty() {
            overrides
                .iter()
                .filter_map(|o| {
                    let idx = port_idx(o)?;
                    Some(build_profile(idx, None, Some(o), &network_lookup))
                })
                .collect()
        } else {
            port_table
                .iter()
                .filter_map(|row| {
                    let idx = port_idx(row)?;
                    Some(build_profile(
                        idx,
                        Some(row),
                        override_map.get(&idx),
                        &network_lookup,
                    ))
                })
                .collect()
        };

        profiles.sort_by_key(|p| p.index);
        Ok(profiles)
    }

    /// Resolve a network by name or session `_id` to its session identifier
    /// and (optional) VLAN id.
    ///
    /// Used to turn user-friendly CLI inputs (`--native-vlan office`) into
    /// the `networkconf` `_id` that `port_overrides` requires.
    pub async fn resolve_network_session_id(
        &self,
        identifier: &str,
    ) -> Result<(String, Option<u16>), CoreError> {
        let guard = self.inner.session_client.lock().await;
        let session = require_session(guard.as_ref())?;

        let records = session.list_network_conf().await?;

        // Exact `_id` match first so ambiguous names never shadow an ID.
        if let Some(hit) = records.iter().find(|rec| {
            rec.get("_id")
                .and_then(Value::as_str)
                .is_some_and(|id| id == identifier)
        }) {
            return Ok((identifier.to_owned(), parse_vlan_id(hit)));
        }

        let matches: Vec<&Value> = records
            .iter()
            .filter(|rec| {
                rec.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| name.eq_ignore_ascii_case(identifier))
            })
            .collect();

        match matches.len() {
            0 => Err(CoreError::NetworkNotFound {
                identifier: identifier.to_owned(),
            }),
            1 => {
                let rec = matches[0];
                let id = rec
                    .get("_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| CoreError::NetworkNotFound {
                        identifier: identifier.to_owned(),
                    })?
                    .to_owned();
                Ok((id, parse_vlan_id(rec)))
            }
            _ => Err(CoreError::ValidationFailed {
                message: format!(
                    "network name {identifier:?} is ambiguous ({} matches); specify the session _id instead",
                    matches.len()
                ),
            }),
        }
    }

    /// Apply `update` to the override for `port_idx` on the device identified
    /// by MAC, preserving every other port's overrides.
    pub async fn update_device_port(
        &self,
        device_mac: &MacAddress,
        port_idx_target: u32,
        update: &PortProfileUpdate,
    ) -> Result<(), CoreError> {
        if port_idx_target == 0 {
            return Err(CoreError::ValidationFailed {
                message: "port index must be 1-based (UniFi switches number ports starting at 1)"
                    .to_owned(),
            });
        }
        let guard = self.inner.session_client.lock().await;
        let session = require_session(guard.as_ref())?;

        let device = session
            .get_device(device_mac.as_str())
            .await?
            .ok_or_else(|| CoreError::DeviceNotFound {
                identifier: device_mac.to_string(),
            })?;

        // Refuse to write a phantom override for a port the device doesn't
        // physically have. The controller silently accepts entries for any
        // index, so without this check `port-set <8-port-switch> 99 ...`
        // reports success and persists a dangling override forever.
        let known_ports: Vec<u32> = device
            .extra
            .get("port_table")
            .and_then(Value::as_array)
            .map(|table| table.iter().filter_map(port_idx).collect())
            .unwrap_or_default();
        if !known_ports.is_empty() && !known_ports.contains(&port_idx_target) {
            let max = known_ports.iter().max().copied().unwrap_or(0);
            return Err(CoreError::ValidationFailed {
                message: format!(
                    "port {port_idx_target} does not exist on this device (valid range 1..={max})"
                ),
            });
        }

        let mut overrides: Vec<Value> = device
            .extra
            .get("port_overrides")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let slot = overrides
            .iter_mut()
            .find(|entry| port_idx(entry) == Some(port_idx_target));

        let existing = slot.as_ref().map(|value| match value {
            Value::Object(map) => map.clone(),
            _ => Map::new(),
        });
        let mut next = existing.unwrap_or_default();
        next.insert("port_idx".into(), json!(port_idx_target));
        apply_update(&mut next, update);

        match slot {
            Some(entry) => *entry = Value::Object(next),
            None => overrides.push(Value::Object(next)),
        }

        debug!(port_idx_target, "updating port_overrides");
        session
            .update_device_port_overrides(device.id.as_str(), overrides)
            .await?;
        Ok(())
    }
}

// ── helpers ──────────────────────────────────────────────────────────────

fn port_idx(value: &Value) -> Option<u32> {
    #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
    value
        .get("port_idx")
        .and_then(Value::as_u64)
        .map(|v| v as u32)
}

fn parse_vlan_id(rec: &Value) -> Option<u16> {
    #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
    rec.get("vlan")
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
        .map(|v| v as u16)
}

struct NetworkLookup {
    by_id: HashMap<String, NetworkInfo>,
}

struct NetworkInfo {
    name: Option<String>,
    vlan_id: Option<u16>,
}

fn build_network_lookup(records: &[Value]) -> NetworkLookup {
    let mut by_id = HashMap::new();
    for rec in records {
        let Some(id) = rec.get("_id").and_then(Value::as_str) else {
            continue;
        };
        by_id.insert(
            id.to_owned(),
            NetworkInfo {
                name: rec.get("name").and_then(Value::as_str).map(str::to_owned),
                vlan_id: parse_vlan_id(rec),
            },
        );
    }
    NetworkLookup { by_id }
}

impl NetworkLookup {
    fn name(&self, id: &str) -> Option<String> {
        self.by_id.get(id).and_then(|n| n.name.clone())
    }
    fn vlan(&self, id: &str) -> Option<u16> {
        self.by_id.get(id).and_then(|n| n.vlan_id)
    }
}

fn build_profile(
    index: u32,
    row: Option<&Value>,
    override_: Option<&Value>,
    networks: &NetworkLookup,
) -> PortProfile {
    let link_state = row
        .and_then(|r| r.get("up"))
        .and_then(Value::as_bool)
        .map_or(PortState::Unknown, |up| {
            if up { PortState::Up } else { PortState::Down }
        });

    let name = first_string(&[override_, row], "name");

    let native_network_id =
        first_string(&[override_, row], "native_networkconf_id").filter(|s| !s.is_empty());
    let tagged_network_ids = first_array(&[override_, row], "tagged_networkconf_ids")
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect::<Vec<_>>();

    let tagged_vlan_mgmt = first_string(&[override_, row], "tagged_vlan_mgmt");
    let op_mode = first_string(&[override_, row], "op_mode");
    let tagged_all = tagged_vlan_mgmt.as_deref() == Some("auto");

    let mode = classify_mode(
        op_mode.as_deref(),
        tagged_vlan_mgmt.as_deref(),
        &tagged_network_ids,
    );

    let native_vlan_id = native_network_id
        .as_deref()
        .and_then(|id| networks.vlan(id));
    let native_network_name = native_network_id
        .as_deref()
        .and_then(|id| networks.name(id));
    let tagged_vlan_ids = tagged_network_ids
        .iter()
        .filter_map(|id| networks.vlan(id))
        .collect();
    let tagged_network_names = tagged_network_ids
        .iter()
        .filter_map(|id| networks.name(id))
        .collect();

    let poe_mode = first_string(&[override_, row], "poe_mode")
        .as_deref()
        .map(parse_poe_mode);
    let speed_setting = parse_speed(
        first_string(&[override_, row], "speed").as_deref(),
        first_bool(&[override_, row], "autoneg"),
    );
    let link_speed_mbps = row
        .and_then(|r| r.get("speed"))
        .and_then(Value::as_u64)
        .map(|v| {
            #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
            {
                v as u32
            }
        });

    let stp_state = first_string(&[override_, row], "stp_state")
        .as_deref()
        .map_or(StpState::Unknown, parse_stp_state);

    let port_profile_id = first_string(&[override_, row], "portconf_id").filter(|s| !s.is_empty());

    PortProfile {
        index,
        name,
        link_state,
        mode,
        native_network_id,
        native_vlan_id,
        native_network_name,
        tagged_network_ids,
        tagged_vlan_ids,
        tagged_network_names,
        tagged_all,
        poe_mode,
        speed_setting,
        link_speed_mbps,
        stp_state,
        port_profile_id,
    }
}

fn first_string(sources: &[Option<&Value>], key: &str) -> Option<String> {
    sources
        .iter()
        .flatten()
        .find_map(|v| v.get(key).and_then(Value::as_str).map(str::to_owned))
}

fn first_bool(sources: &[Option<&Value>], key: &str) -> Option<bool> {
    sources
        .iter()
        .flatten()
        .find_map(|v| v.get(key).and_then(Value::as_bool))
}

fn first_array<'a>(sources: &[Option<&'a Value>], key: &str) -> Option<&'a Vec<Value>> {
    sources
        .iter()
        .flatten()
        .find_map(|v| v.get(key).and_then(Value::as_array))
}

fn classify_mode(
    op_mode: Option<&str>,
    tagged_vlan_mgmt: Option<&str>,
    tagged_ids: &[String],
) -> PortMode {
    if op_mode == Some("mirror") {
        return PortMode::Mirror;
    }
    match tagged_vlan_mgmt {
        Some("block_all") => PortMode::Access,
        Some("auto" | "custom") => PortMode::Trunk,
        _ => {
            if tagged_ids.is_empty() {
                PortMode::Unknown
            } else {
                PortMode::Trunk
            }
        }
    }
}

fn parse_poe_mode(raw: &str) -> PoeMode {
    match raw {
        "auto" => PoeMode::Auto,
        "off" => PoeMode::Off,
        "pasv24" => PoeMode::Passive24V,
        "passthrough" => PoeMode::Passthrough,
        _ => PoeMode::Other,
    }
}

fn parse_speed(raw: Option<&str>, autoneg: Option<bool>) -> Option<PortSpeedSetting> {
    if autoneg == Some(true) {
        return Some(PortSpeedSetting::Auto);
    }
    match raw {
        Some("auto") => Some(PortSpeedSetting::Auto),
        Some("10") => Some(PortSpeedSetting::Mbps10),
        Some("100") => Some(PortSpeedSetting::Mbps100),
        Some("1000") => Some(PortSpeedSetting::Mbps1000),
        Some("2500") => Some(PortSpeedSetting::Mbps2500),
        Some("5000") => Some(PortSpeedSetting::Mbps5000),
        Some("10000") => Some(PortSpeedSetting::Mbps10000),
        None | Some(_) => None,
    }
}

fn parse_stp_state(raw: &str) -> StpState {
    match raw {
        "disabled" => StpState::Disabled,
        "blocking" => StpState::Blocking,
        "listening" => StpState::Listening,
        "learning" => StpState::Learning,
        "forwarding" => StpState::Forwarding,
        "broken" => StpState::Broken,
        _ => StpState::Unknown,
    }
}

fn apply_update(target: &mut Map<String, Value>, update: &PortProfileUpdate) {
    if let Some(name) = &update.name {
        target.insert("name".into(), json!(name));
    }

    if let Some(mode) = update.mode {
        match mode {
            PortMode::Access => {
                target.insert("op_mode".into(), json!("switch"));
                target.insert("tagged_vlan_mgmt".into(), json!("block_all"));
                target.insert("tagged_networkconf_ids".into(), json!([]));
            }
            PortMode::Trunk => {
                target.insert("op_mode".into(), json!("switch"));
                // If caller provides a tagged list we default to "custom";
                // otherwise "auto" means "all VLANs" which is the UniFi trunk
                // default.
                if update.tagged_network_ids.is_some() {
                    target.insert("tagged_vlan_mgmt".into(), json!("custom"));
                } else {
                    target.insert("tagged_vlan_mgmt".into(), json!("auto"));
                }
            }
            PortMode::Mirror => {
                target.insert("op_mode".into(), json!("mirror"));
            }
            PortMode::Unknown => {}
        }
    }

    if let Some(id) = &update.native_network_id {
        target.insert("native_networkconf_id".into(), json!(id));
    }

    if let Some(tagged) = &update.tagged_network_ids {
        target.insert("tagged_networkconf_ids".into(), json!(tagged));
        if update.mode.is_some_and(|m| matches!(m, PortMode::Trunk)) {
            target.insert("tagged_vlan_mgmt".into(), json!("custom"));
        }
    }

    if let Some(poe) = update.poe_mode {
        target.insert(
            "poe_mode".into(),
            json!(match poe {
                PoeMode::Off => "off",
                PoeMode::Passive24V => "pasv24",
                PoeMode::Passthrough => "passthrough",
                PoeMode::Auto | PoeMode::Other => "auto",
            }),
        );
    }

    if let Some(speed) = update.speed_setting {
        match speed {
            PortSpeedSetting::Auto => {
                target.insert("autoneg".into(), json!(true));
                target.insert("speed".into(), json!("auto"));
            }
            other => {
                target.insert("autoneg".into(), json!(false));
                target.insert(
                    "speed".into(),
                    json!(match other {
                        PortSpeedSetting::Mbps10 => "10",
                        PortSpeedSetting::Mbps100 => "100",
                        PortSpeedSetting::Mbps1000 => "1000",
                        PortSpeedSetting::Mbps2500 => "2500",
                        PortSpeedSetting::Mbps5000 => "5000",
                        PortSpeedSetting::Mbps10000 => "10000",
                        PortSpeedSetting::Auto => "auto",
                    }),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_networks() -> NetworkLookup {
        build_network_lookup(&[
            json!({ "_id": "n1", "name": "infra", "vlan": 10 }),
            json!({ "_id": "n2", "name": "personal", "vlan": 20 }),
        ])
    }

    #[test]
    fn classify_mode_detects_mirror() {
        assert_eq!(classify_mode(Some("mirror"), None, &[]), PortMode::Mirror);
    }

    #[test]
    fn classify_mode_detects_access_and_trunk() {
        assert_eq!(
            classify_mode(Some("switch"), Some("block_all"), &[]),
            PortMode::Access
        );
        assert_eq!(
            classify_mode(Some("switch"), Some("auto"), &[]),
            PortMode::Trunk
        );
        assert_eq!(
            classify_mode(Some("switch"), Some("custom"), &["n2".into()]),
            PortMode::Trunk
        );
    }

    #[test]
    fn parse_speed_autoneg_beats_explicit() {
        assert_eq!(
            parse_speed(Some("1000"), Some(true)),
            Some(PortSpeedSetting::Auto)
        );
        assert_eq!(
            parse_speed(Some("1000"), Some(false)),
            Some(PortSpeedSetting::Mbps1000)
        );
    }

    #[test]
    fn build_profile_uses_overrides_before_live_state() {
        let row = json!({
            "port_idx": 10,
            "up": true,
            "speed": 1000,
            "name": "auto-name",
            "tagged_vlan_mgmt": "auto",
            "native_networkconf_id": "n1",
            "poe_mode": "auto",
            "stp_state": "forwarding",
        });
        let override_ = json!({
            "port_idx": 10,
            "name": "mac-mini",
            "tagged_vlan_mgmt": "custom",
            "tagged_networkconf_ids": ["n2"],
            "native_networkconf_id": "n1",
            "poe_mode": "off",
            "autoneg": false,
            "speed": "1000",
        });
        let profile = build_profile(10, Some(&row), Some(&override_), &sample_networks());
        assert_eq!(profile.name.as_deref(), Some("mac-mini"));
        assert_eq!(profile.mode, PortMode::Trunk);
        assert_eq!(profile.native_vlan_id, Some(10));
        assert_eq!(profile.native_network_name.as_deref(), Some("infra"));
        assert_eq!(profile.tagged_vlan_ids, vec![20]);
        assert_eq!(profile.tagged_network_names, vec!["personal"]);
        assert_eq!(profile.poe_mode, Some(PoeMode::Off));
        assert_eq!(profile.speed_setting, Some(PortSpeedSetting::Mbps1000));
        assert_eq!(profile.link_speed_mbps, Some(1000));
        assert_eq!(profile.stp_state, StpState::Forwarding);
        assert_eq!(profile.link_state, PortState::Up);
    }

    #[test]
    fn apply_update_access_mode_clears_tagged_list() {
        let mut target = Map::new();
        target.insert("port_idx".into(), json!(10));
        target.insert("tagged_networkconf_ids".into(), json!(["old"]));
        apply_update(
            &mut target,
            &PortProfileUpdate {
                mode: Some(PortMode::Access),
                native_network_id: Some("n1".into()),
                ..PortProfileUpdate::default()
            },
        );
        assert_eq!(target.get("tagged_vlan_mgmt"), Some(&json!("block_all")));
        assert_eq!(target.get("tagged_networkconf_ids"), Some(&json!([])));
        assert_eq!(target.get("native_networkconf_id"), Some(&json!("n1")));
        assert_eq!(target.get("op_mode"), Some(&json!("switch")));
    }

    #[test]
    fn apply_update_trunk_with_tagged_list_marks_custom() {
        let mut target = Map::new();
        target.insert("port_idx".into(), json!(10));
        apply_update(
            &mut target,
            &PortProfileUpdate {
                mode: Some(PortMode::Trunk),
                native_network_id: Some("n1".into()),
                tagged_network_ids: Some(vec!["n2".into()]),
                ..PortProfileUpdate::default()
            },
        );
        assert_eq!(target.get("tagged_vlan_mgmt"), Some(&json!("custom")));
        assert_eq!(target.get("tagged_networkconf_ids"), Some(&json!(["n2"])));
    }

    #[test]
    fn apply_update_speed_fixed_disables_autoneg() {
        let mut target = Map::new();
        apply_update(
            &mut target,
            &PortProfileUpdate {
                speed_setting: Some(PortSpeedSetting::Mbps2500),
                ..PortProfileUpdate::default()
            },
        );
        assert_eq!(target.get("autoneg"), Some(&json!(false)));
        assert_eq!(target.get("speed"), Some(&json!("2500")));
    }
}
