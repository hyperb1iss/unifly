//! Device and client resolution against the controller snapshot.
//!
//! Every command that targets one device or client goes through these, so
//! all of them accept the same identifiers: ID, name (or client hostname),
//! IP, or MAC in any common spelling. See [`super::lookup`].

use std::net::IpAddr;
use std::sync::Arc;

use unifly_api::{Client, Controller, Device, EntityId, MacAddress};

use super::lookup::{self, Shape};
use crate::cli::error::CliError;

/// The adopted device `identifier` names.
pub fn resolve_device(controller: &Controller, identifier: &str) -> Result<Arc<Device>, CliError> {
    let snapshot = controller.devices_snapshot();
    lookup::resolve(snapshot.as_slice(), identifier).map(Arc::clone)
}

pub fn resolve_device_id(controller: &Controller, identifier: &str) -> Result<EntityId, CliError> {
    resolve_device(controller, identifier).map(|device| device.id.clone())
}

/// The MAC of the device `identifier` names. A MAC absent from the snapshot
/// passes through, so MAC-addressed Session commands can still reach a
/// device the snapshot does not hold; names and IPs never pass through.
pub fn resolve_device_mac(
    controller: &Controller,
    identifier: &str,
) -> Result<MacAddress, CliError> {
    lookup::lookup_mac(controller.devices_snapshot().as_slice(), identifier)
}

/// The client `identifier` names in the current snapshot.
pub fn resolve_client(controller: &Controller, identifier: &str) -> Result<Arc<Client>, CliError> {
    let snapshot = controller.clients_snapshot();
    lookup::resolve(snapshot.as_slice(), identifier).map(Arc::clone)
}

/// The MAC to ask for a client's roam history. Roam history outlives the
/// connection, so a MAC the snapshot lacks passes through unchanged; an IP
/// address or name never becomes a MAC.
pub fn resolve_client_mac(
    controller: &Controller,
    identifier: &str,
) -> Result<MacAddress, CliError> {
    lookup::lookup_mac(controller.clients_snapshot().as_slice(), identifier)
}

/// The IP address to ask for a client's WiFi experience, which the
/// controller keys by IP. An IPv4 address the snapshot lacks passes
/// through unchanged; a MAC or name that matches no client is not found.
pub fn resolve_client_ip(controller: &Controller, identifier: &str) -> Result<IpAddr, CliError> {
    client_ip_in(controller.clients_snapshot().as_slice(), identifier)
}

fn client_ip_in(clients: &[Arc<Client>], identifier: &str) -> Result<IpAddr, CliError> {
    if let Some(client) = lookup::lookup(clients, identifier)? {
        return client.ip.ok_or_else(|| CliError::Validation {
            field: "client".into(),
            reason: format!("client '{identifier}' has no IP address to look up"),
        });
    }
    match Shape::of(identifier) {
        Shape::Ip(ip @ IpAddr::V4(_)) => Ok(ip),
        Shape::Ip(IpAddr::V6(_)) | Shape::Mac(_) | Shape::Name => {
            Err(lookup::not_found::<Client>(identifier))
        }
    }
}

/// An argument that must be a MAC address (any common spelling), such as
/// the pending device `adopt` targets. An IP address is never read as one.
pub fn require_mac(raw: &str, purpose: &str) -> Result<MacAddress, CliError> {
    match Shape::of(raw) {
        Shape::Mac(mac) => Ok(mac),
        Shape::Ip(_) | Shape::Name => Err(CliError::Validation {
            field: "mac".into(),
            reason: format!("'{raw}' is not a MAC address; {purpose}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{client_ip_in, require_mac, resolve_device_mac};
    use crate::cli::commands::util::fixtures::client;
    use crate::cli::commands::util::lookup::lookup_mac;
    use crate::cli::error::CliError;
    use unifly_api::{Controller, ControllerConfig};

    fn crew() -> Vec<std::sync::Arc<unifly_api::Client>> {
        vec![client(
            "Crew Laptop",
            "crew-laptop-01",
            "76:ce:f4:ac:26:ee",
            Some("10.40.0.131"),
        )]
    }

    /// `clients roams` once reparsed an unmatched IPv6 address as a MAC, so
    /// `::76ce:f4ac:26ee` fetched the history of 76:ce:f4:ac:26:ee.
    #[test]
    fn client_roams_mac_never_comes_from_an_ip_address() {
        let clients = crew();
        for needle in ["::76ce:f4ac:26ee", "10.40.0.200"] {
            assert!(
                matches!(lookup_mac(&clients, needle), Err(CliError::NotFound { .. })),
                "{needle} must not become a MAC"
            );
        }
        let absent = lookup_mac(&clients, "76-CE-F4-AC-26-EF").expect("MAC passes through");
        assert_eq!(absent.as_str(), "76:ce:f4:ac:26:ef");
        let known = lookup_mac(&clients, "crew laptop").expect("by name");
        assert_eq!(known.as_str(), "76:ce:f4:ac:26:ee");
    }

    #[test]
    fn client_wifi_ip_passes_only_an_unknown_ipv4_through() {
        let clients = crew();
        let known = client_ip_in(&clients, "76:ce:f4:ac:26:ee").expect("by MAC");
        assert_eq!(known.to_string(), "10.40.0.131");
        let absent = client_ip_in(&clients, "10.40.0.200").expect("IPv4 passes through");
        assert_eq!(absent.to_string(), "10.40.0.200");
        for needle in ["::76ce:f4ac:26ef", "76:ce:f4:ac:26:ef", "nobody"] {
            assert!(
                matches!(
                    client_ip_in(&clients, needle),
                    Err(CliError::NotFound { .. })
                ),
                "{needle} must be not found"
            );
        }
    }

    #[test]
    fn require_mac_accepts_mac_spellings_and_refuses_ip_addresses() {
        let mac = require_mac("AABB.CCDD.EEFF", "adopt").expect("dotted MAC");
        assert_eq!(mac.as_str(), "aa:bb:cc:dd:ee:ff");
        for raw in ["::aabb:ccdd:eeff", "10.0.0.1", "ap-roof"] {
            assert!(
                matches!(require_mac(raw, "adopt"), Err(CliError::Validation { .. })),
                "{raw} is not a MAC"
            );
        }
    }

    #[test]
    fn resolve_device_mac_accepts_valid_fallback_mac() {
        let controller = Controller::new(ControllerConfig::default());
        for spelling in ["AABBCCDDEEFF", "AA-BB-CC-DD-EE-FF", "aabb.ccdd.eeff"] {
            match resolve_device_mac(&controller, spelling) {
                Ok(mac) => assert_eq!(mac.as_str(), "aa:bb:cc:dd:ee:ff"),
                Err(error) => panic!("expected valid MAC fallback, got {error:?}"),
            }
        }
    }

    #[test]
    fn resolve_device_mac_never_passes_an_ip_address_through() {
        let controller = Controller::new(ControllerConfig::default());
        for needle in ["::aabb:ccdd:eeff", "10.40.0.18"] {
            assert!(
                matches!(
                    resolve_device_mac(&controller, needle),
                    Err(crate::cli::error::CliError::NotFound { .. })
                ),
                "{needle} must not pass through as a MAC"
            );
        }
    }

    #[test]
    fn resolve_device_mac_reports_unknown_identifiers_as_not_found() {
        let controller = Controller::new(ControllerConfig::default());
        match resolve_device_mac(&controller, "definitely-not-a-mac") {
            Err(crate::cli::error::CliError::NotFound {
                resource_type,
                list_command,
                ..
            }) => {
                assert_eq!(resource_type, "device");
                assert_eq!(list_command, "devices list");
            }
            Ok(mac) => panic!("expected not found, got {mac:?}"),
            Err(other) => panic!("expected not found, got {other:?}"),
        }
    }
}
