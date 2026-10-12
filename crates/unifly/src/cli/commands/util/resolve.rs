//! Device and client resolution against the controller snapshot.
//!
//! Every command that targets one device or client goes through these, so
//! all of them accept the same identifiers: ID, name (or client hostname),
//! IP, or MAC in any common spelling. See [`super::lookup`].

use std::sync::Arc;

use unifly_api::{Client, Controller, Device, EntityId, MacAddress};

use super::lookup;
use crate::cli::error::CliError;

/// The adopted device `identifier` names.
pub fn resolve_device(controller: &Controller, identifier: &str) -> Result<Arc<Device>, CliError> {
    let snapshot = controller.devices_snapshot();
    lookup::resolve(snapshot.as_slice(), identifier).map(Arc::clone)
}

pub fn resolve_device_id(controller: &Controller, identifier: &str) -> Result<EntityId, CliError> {
    resolve_device(controller, identifier).map(|device| device.id.clone())
}

/// The MAC of the device `identifier` names. A well-formed MAC absent from
/// the snapshot passes through, so MAC-addressed Session commands can still
/// reach a device the snapshot does not hold.
pub fn resolve_device_mac(
    controller: &Controller,
    identifier: &str,
) -> Result<MacAddress, CliError> {
    let snapshot = controller.devices_snapshot();
    if let Some(device) = lookup::lookup(snapshot.as_slice(), identifier)? {
        return Ok(device.mac.clone());
    }
    identifier
        .trim()
        .parse::<MacAddress>()
        .map_err(|_| lookup::not_found::<Device>(identifier))
}

/// The client `identifier` names in the current snapshot.
pub fn resolve_client(controller: &Controller, identifier: &str) -> Result<Arc<Client>, CliError> {
    let snapshot = controller.clients_snapshot();
    lookup::resolve(snapshot.as_slice(), identifier).map(Arc::clone)
}

#[cfg(test)]
mod tests {
    use super::resolve_device_mac;
    use unifly_api::{Controller, ControllerConfig};

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
