//! Which `rest/networkconf` record a DHCP reservation belongs to.
//!
//! `rest/user` keys a reservation's network by the networkconf `_id`. The
//! Integration API names the same network by a UUID, which the Session
//! record carries as `external_id`, so an Integration ID has to be mapped
//! before it is written. Without a named network the reservation goes to
//! the LAN whose subnet holds the address.

use std::net::Ipv4Addr;

use serde_json::Value;

use crate::core_error::CoreError;
use crate::model::EntityId;

fn text<'a>(record: &'a Value, key: &str) -> Option<&'a str> {
    record.get(key).and_then(Value::as_str)
}

/// An IPv4 CIDR such as `10.40.20.1/24` as its address and prefix length.
fn parse_cidr(value: &str) -> Option<(Ipv4Addr, u32)> {
    let (address, prefix) = value.split_once('/')?;
    let prefix: u32 = prefix.parse().ok()?;
    (prefix <= 32).then_some((address.parse().ok()?, prefix))
}

fn subnet_holds(subnet: &str, ip: Ipv4Addr) -> bool {
    parse_cidr(subnet).is_some_and(|(address, prefix)| {
        let mask = u32::MAX.checked_shl(32 - prefix).unwrap_or(0);
        u32::from(ip) & mask == u32::from(address) & mask
    })
}

/// Whether a networkconf record is a LAN that hands out leases. WAN and
/// VPN records carry subnets too, and a reservation means nothing there.
fn is_lan(record: &Value) -> bool {
    !text(record, "purpose").is_some_and(|purpose| purpose == "wan" || purpose.contains("vpn"))
}

fn session_id(record: &Value, identifier: &str) -> Result<String, CoreError> {
    text(record, "_id")
        .map(str::to_owned)
        .ok_or_else(|| CoreError::NetworkNotFound {
            identifier: identifier.to_owned(),
        })
}

/// The networkconf `_id` of the network `wanted` names: an Integration
/// UUID is matched on `external_id`, and anything else on `_id`, then on
/// the network's name.
pub(crate) fn named_network(records: &[Value], wanted: &EntityId) -> Result<String, CoreError> {
    match wanted {
        EntityId::Uuid(uuid) => {
            let uuid = uuid.to_string();
            let record = records
                .iter()
                .find(|record| text(record, "external_id") == Some(uuid.as_str()))
                .ok_or_else(|| CoreError::NetworkNotFound {
                    identifier: uuid.clone(),
                })?;
            session_id(record, &uuid)
        }
        EntityId::Legacy(identifier) => {
            if records
                .iter()
                .any(|record| text(record, "_id") == Some(identifier.as_str()))
            {
                return Ok(identifier.clone());
            }
            let named: Vec<&Value> = records
                .iter()
                .filter(|record| {
                    text(record, "name").is_some_and(|name| name.eq_ignore_ascii_case(identifier))
                })
                .collect();
            match named.as_slice() {
                [record] => session_id(record, identifier),
                [] => Err(CoreError::NetworkNotFound {
                    identifier: identifier.clone(),
                }),
                _ => Err(CoreError::ValidationFailed {
                    message: format!(
                        "{} networks are named {identifier:?}; name one by its ID",
                        named.len()
                    ),
                }),
            }
        }
    }
}

/// The networkconf `_id` of the LAN whose `ip_subnet` holds `ip`.
pub(crate) fn network_holding(records: &[Value], ip: Ipv4Addr) -> Result<String, CoreError> {
    let record = records
        .iter()
        .filter(|record| is_lan(record))
        .find(|record| text(record, "ip_subnet").is_some_and(|subnet| subnet_holds(subnet, ip)))
        .ok_or_else(|| CoreError::ValidationFailed {
            message: format!("{ip} is not inside any network's subnet"),
        })?;
    session_id(record, &ip.to_string())
}

/// The networkconf `_id` a fixed IP for `ip` is written under: the
/// network `wanted` names, else the LAN holding the address.
pub(crate) fn reservation_network(
    records: &[Value],
    wanted: Option<&EntityId>,
    ip: Ipv4Addr,
) -> Result<String, CoreError> {
    match wanted {
        Some(wanted) => named_network(records, wanted),
        None => network_holding(records, ip),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use serde_json::json;

    use super::*;

    const STUDIO_UUID: &str = "6f1c1f4e-9a1c-4c55-9f0e-2b8f6f9c0a11";

    fn records() -> Vec<Value> {
        vec![
            json!({"_id": "wan-1", "name": "Internet", "purpose": "wan",
                "ip_subnet": "10.40.20.1/24"}),
            json!({"_id": "5f1a00000000000000000001", "name": "Studio",
                "purpose": "corporate", "ip_subnet": "10.40.20.1/24",
                "external_id": STUDIO_UUID}),
            json!({"_id": "5f1a00000000000000000002", "name": "Guest",
                "purpose": "guest", "ip_subnet": "10.40.60.1/23"}),
            json!({"_id": "5f1a00000000000000000003", "name": "Teleport",
                "purpose": "remote-user-vpn", "ip_subnet": "192.168.5.1/24"}),
            json!({"_id": "5f1a00000000000000000004", "name": "Cameras",
                "purpose": "vlan-only"}),
        ]
    }

    fn pick(wanted: Option<&str>, ip: [u8; 4]) -> Result<String, CoreError> {
        let wanted = wanted.map(EntityId::from);
        reservation_network(&records(), wanted.as_ref(), Ipv4Addr::from(ip))
    }

    #[test]
    fn an_integration_uuid_maps_to_the_session_id_through_external_id() {
        assert_eq!(
            pick(Some(STUDIO_UUID), [10, 40, 20, 7]).unwrap(),
            "5f1a00000000000000000001"
        );
        let unknown = pick(
            Some("6f1c1f4e-9a1c-4c55-9f0e-2b8f6f9c0aff"),
            [10, 40, 20, 7],
        );
        assert!(
            matches!(unknown, Err(CoreError::NetworkNotFound { .. })),
            "a UUID no record carries must never be written as is: {unknown:?}"
        );
    }

    #[test]
    fn a_session_id_or_a_name_resolves_to_the_session_id() {
        assert_eq!(
            pick(Some("5f1a00000000000000000002"), [10, 40, 60, 9]).unwrap(),
            "5f1a00000000000000000002"
        );
        assert_eq!(
            pick(Some("guest"), [10, 40, 60, 9]).unwrap(),
            "5f1a00000000000000000002"
        );
        assert!(matches!(
            pick(Some("nope"), [10, 40, 60, 9]),
            Err(CoreError::NetworkNotFound { .. })
        ));
    }

    #[test]
    fn without_a_network_the_lan_holding_the_address_is_picked() {
        // The WAN record lists the same subnet first and is passed over.
        assert_eq!(
            pick(None, [10, 40, 20, 77]).unwrap(),
            "5f1a00000000000000000001"
        );
        assert_eq!(
            pick(None, [10, 40, 61, 9]).unwrap(),
            "5f1a00000000000000000002"
        );
        for stray in [[192, 168, 5, 9], [172, 16, 0, 1]] {
            let refused = pick(None, stray);
            assert!(
                matches!(&refused, Err(CoreError::ValidationFailed { message })
                    if message.contains("not inside any network's subnet")),
                "{refused:?}"
            );
        }
    }

    #[test]
    fn malformed_subnets_hold_nothing() {
        assert!(!subnet_holds("10.40.20.1/33", Ipv4Addr::new(10, 40, 20, 1)));
        assert!(!subnet_holds("10.40.20.1", Ipv4Addr::new(10, 40, 20, 1)));
        assert!(subnet_holds("0.0.0.0/0", Ipv4Addr::new(10, 40, 20, 1)));
    }
}
