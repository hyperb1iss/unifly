//! The subnet a prefix-delegation network serves, which only the gateway
//! reports.

use crate::model::{Ipv6Mode, Ipv6Scope, Network};
use crate::session::models::SessionDevice;

/// Fill in the subnet each prefix-delegation network serves from the
/// gateway's network table, which is the only place the controller says
/// what the WAN was delegated. The table names each network as the
/// Integration record does; its record IDs are session ones, which never
/// match an Integration network's UUID.
pub(super) fn merge(networks: &mut [Network], session_devices: &[SessionDevice]) {
    let rows = session_devices
        .iter()
        .filter_map(|device| device.network_table.as_ref()?.as_array())
        .flatten();
    for row in rows {
        let name = row.get("name").and_then(serde_json::Value::as_str);
        let Some(network) = networks
            .iter_mut()
            .find(|network| name.is_some_and(|name| network.name == name))
        else {
            continue;
        };
        if network.ipv6_mode != Some(Ipv6Mode::PrefixDelegation) || network.ipv6_prefix.is_some() {
            continue;
        }
        network.ipv6_prefix = row
            .get("ipv6_subnets")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .find(|subnet| {
                let address = subnet.split('/').next().unwrap_or(subnet);
                address
                    .parse()
                    .is_ok_and(|address| Ipv6Scope::of(&address) == Ipv6Scope::Global)
            })
            .map(String::from);
    }
}

#[cfg(test)]
mod tests;
