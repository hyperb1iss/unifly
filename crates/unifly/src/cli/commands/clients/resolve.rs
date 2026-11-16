use unifly_api::{Controller, EntityId};

/// The network a reservation names. A name or ID the store lists becomes
/// that network's ID; anything else goes on as typed for the controller's
/// own network records to match, since a Session-only profile keeps no
/// networks in the store.
pub(super) fn named_network(controller: &Controller, name_or_id: &str) -> EntityId {
    controller
        .networks_snapshot()
        .iter()
        .find(|network| {
            network.name.eq_ignore_ascii_case(name_or_id) || network.id.to_string() == name_or_id
        })
        .map_or_else(|| EntityId::from(name_or_id), |network| network.id.clone())
}
