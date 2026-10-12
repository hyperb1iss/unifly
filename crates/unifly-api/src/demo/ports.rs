// ── Demo switch ports: the Session device record behind `devices ports` ──
//
// `devices ports` and `ports-export` read a device's raw `port_table` (live
// link state) and `port_overrides` (what the site configured), plus
// `rest/networkconf` to name the VLANs. The demo builds those records from
// the fleet and runs them through the same normalization a real
// controller's answer takes. Link, speed, and PoE come from the device's
// ports exactly as `devices get` reports them. Each port's configuration
// follows from what plugs into it, so the ports agree with `clients list`
// and `topology`:
//
// - a catalog endpoint gets an access port on its own network, labeled with
//   its name (the dark camera's port stays configured while it is down);
// - an AP gets a trunk carrying every VLAN;
// - a downstream switch gets a trunk tagged with just the networks behind
//   it, while the gateway's link to the core carries every VLAN;
// - a device's own uplink is a trunk carrying every VLAN;
// - gateway WAN ports have no switch profile, and every other port sits at
//   controller defaults (native Mgmt, all VLANs) with no override.
//
// Network `_id`s are the fleet's network IDs, so they match `networks list`.

use serde_json::{Map, Value, json};

use super::catalog::{OFFLINE_CAMERA, WIRED};
use super::fleet::{Fleet, GATEWAY, SWITCH_BASE, network_index};
use super::sim::Simulation;
use crate::model::{DeviceType, MacAddress, Port, PortConnector, PortState};

/// Endpoints whose port the studio pinned to a fixed speed, in Mbps as the
/// wire spells it. The ops printer's old NIC only holds a 100 Mbps link.
const PINNED_SPEEDS: [(&str, &str); 1] = [("printer-ops", "100")];

/// Native network of trunks and unconfigured ports.
const NATIVE: &str = "mgmt";

/// Label the fleet gives a device's own uplink port.
const UPLINK: &str = "Uplink";

/// VLANs a trunk tags beyond its native network.
enum Tagged {
    /// Every network (`tagged_vlan_mgmt: auto`).
    All,
    /// Just these networks, by index into the fleet's networks.
    Only(Vec<usize>),
}

/// A port's configuration, from what plugs into it.
enum Profile {
    /// An endpoint on an access port.
    Access {
        label: String,
        network: usize,
        pinned: Option<&'static str>,
    },
    /// A link to another device, or the device's own uplink.
    Trunk { label: String, tagged: Tagged },
    /// A gateway WAN port: no switch profile.
    Wan,
    /// Nothing configured: controller defaults, no override.
    Default,
}

/// A wired endpoint the catalog plugs into a switch port.
struct Endpoint {
    /// Index of the switch in the fleet's devices.
    switch: usize,
    port: u32,
    name: &'static str,
    network: &'static str,
}

/// Every catalog endpoint, plus the dark camera on its configured port.
fn endpoints() -> impl Iterator<Item = Endpoint> {
    let (camera, camera_port) = OFFLINE_CAMERA;
    WIRED
        .iter()
        .map(|spec| Endpoint {
            switch: SWITCH_BASE + spec.switch,
            port: spec.port,
            name: spec.name,
            network: spec.network,
        })
        .chain(std::iter::once(Endpoint {
            switch: SWITCH_BASE,
            port: camera_port,
            name: camera,
            network: "cameras",
        }))
}

/// Fleet indices of the devices uplinked to device `parent`.
fn children(fleet: &Fleet, parent: usize) -> impl Iterator<Item = usize> + '_ {
    let mac = &fleet.devices[parent].mac;
    fleet
        .devices
        .iter()
        .enumerate()
        .filter(move |(_, device)| device.uplink_device_mac.as_ref() == Some(mac))
        .map(|(index, _)| index)
}

/// Networks a trunk down to switch `index` must tag: its endpoints'
/// networks, the SSIDs' networks when an AP hangs off it, and whatever its
/// own downstream switches need, minus the native network.
fn carried(fleet: &Fleet, index: usize) -> Vec<usize> {
    let mut networks: Vec<usize> = endpoints()
        .filter(|endpoint| endpoint.switch == index)
        .map(|endpoint| network_index(endpoint.network))
        .collect();
    for child in children(fleet, index) {
        match fleet.devices[child].device_type {
            DeviceType::AccessPoint => networks.extend(fleet.wifi.iter().filter_map(|wifi| {
                let id = wifi.network_id.as_ref()?;
                fleet.networks.iter().position(|network| &network.id == id)
            })),
            DeviceType::Switch => networks.extend(carried(fleet, child)),
            _ => {}
        }
    }
    let native = network_index(NATIVE);
    networks.retain(|network| *network != native);
    networks.sort_unstable();
    networks.dedup();
    networks
}

/// How `port` on device `index` is configured, from what plugs into it.
fn profile(fleet: &Fleet, index: usize, port: &Port) -> Profile {
    if index == GATEWAY
        && port
            .name
            .as_deref()
            .is_some_and(|name| name.starts_with("WAN"))
    {
        return Profile::Wan;
    }
    if let Some(endpoint) =
        endpoints().find(|endpoint| endpoint.switch == index && endpoint.port == port.index)
    {
        return Profile::Access {
            label: endpoint.name.into(),
            network: network_index(endpoint.network),
            pinned: PINNED_SPEEDS
                .iter()
                .find(|(name, _)| *name == endpoint.name)
                .map(|(_, speed)| *speed),
        };
    }
    if let Some(child) = children(fleet, index)
        .find(|child| fleet.devices[*child].uplink_port_idx == Some(port.index))
    {
        let device = &fleet.devices[child];
        let tagged = if device.device_type == DeviceType::Switch && index != GATEWAY {
            Tagged::Only(carried(fleet, child))
        } else {
            Tagged::All
        };
        return Profile::Trunk {
            label: device.name.clone().unwrap_or_default(),
            tagged,
        };
    }
    if port.name.as_deref() == Some(UPLINK) {
        return Profile::Trunk {
            label: UPLINK.into(),
            tagged: Tagged::All,
        };
    }
    Profile::Default
}

/// The configuration keys a port carries, shared by its `port_table` row
/// (as the effective setting) and its override (as what was set).
fn config(profile: &Profile, port: &Port, network_ids: &[String]) -> Map<String, Value> {
    let mut config = Map::new();
    let mgmt = &network_ids[network_index(NATIVE)];
    // (native network, `tagged_vlan_mgmt`, explicitly tagged networks)
    let vlans = match profile {
        Profile::Access { network, .. } => Some((&network_ids[*network], "block_all", Vec::new())),
        Profile::Trunk {
            tagged: Tagged::Only(networks),
            ..
        } => Some((
            mgmt,
            "custom",
            networks.iter().map(|index| &network_ids[*index]).collect(),
        )),
        Profile::Trunk {
            tagged: Tagged::All,
            ..
        }
        | Profile::Default => Some((mgmt, "auto", Vec::new())),
        Profile::Wan => None,
    };
    if let Some((native, tagging, tagged)) = vlans {
        config.insert("op_mode".into(), json!("switch"));
        config.insert("native_networkconf_id".into(), json!(native));
        config.insert("tagged_vlan_mgmt".into(), json!(tagging));
        config.insert("tagged_networkconf_ids".into(), json!(tagged));
    }
    if let Profile::Access { label, .. } | Profile::Trunk { label, .. } = profile {
        config.insert("name".into(), json!(label));
    }
    let pinned = match profile {
        Profile::Access { pinned, .. } => *pinned,
        _ => None,
    };
    config.insert("autoneg".into(), json!(pinned.is_none()));
    if let Some(speed) = pinned {
        config.insert("speed".into(), json!(speed));
    }
    if let Some(poe) = &port.poe {
        let mode = if poe.enabled { "auto" } else { "off" };
        config.insert("poe_mode".into(), json!(mode));
    }
    config
}

/// A `port_table` row: the effective configuration plus live link state.
fn table_row(profile: &Profile, port: &Port, mut config: Map<String, Value>) -> Value {
    let up = port.state == PortState::Up;
    // On the live table `speed` is the negotiated rate, not the pinned one.
    config.remove("speed");
    let name = config
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| port.name.clone())
        .unwrap_or_else(|| format!("Port {}", port.index));
    config.insert("port_idx".into(), json!(port.index));
    config.insert("name".into(), json!(name));
    config.insert("up".into(), json!(up));
    if let Some(speed) = port.speed_mbps.filter(|_| up) {
        config.insert("speed".into(), json!(speed));
    }
    if let Some(media) = port.connector.as_ref().map(|connector| match connector {
        PortConnector::Rj45 => "GE",
        PortConnector::Sfp => "SFP",
        PortConnector::SfpPlus => "SFP+",
        PortConnector::Sfp28 => "SFP28",
        PortConnector::Qsfp28 => "QSFP28",
    }) {
        config.insert("media".into(), json!(media));
    }
    if !matches!(profile, Profile::Wan) {
        let stp = if up { "forwarding" } else { "disabled" };
        config.insert("stp_state".into(), json!(stp));
    }
    config.insert("port_poe".into(), json!(port.poe.is_some()));
    if let Some(poe) = &port.poe {
        config.insert("poe_caps".into(), json!(7));
        config.insert("poe_enable".into(), json!(poe.enabled));
        let good = poe.enabled && poe.state == PortState::Up;
        config.insert("poe_good".into(), json!(good));
    }
    Value::Object(config)
}

impl Simulation {
    /// Session `stat/device` record for the device with `mac`: identity,
    /// `port_table`, and `port_overrides`. `None` outside the fleet.
    pub(crate) fn session_device(&self, mac: &MacAddress) -> Option<Value> {
        let index = self.device_index_by_mac(mac)?;
        let fleet = &self.fleet;
        let device = &fleet.devices[index];
        let network_ids: Vec<String> = fleet
            .networks
            .iter()
            .map(|network| network.id.to_string())
            .collect();
        let mut table = Vec::with_capacity(device.ports.len());
        let mut overrides = Vec::new();
        for port in &device.ports {
            let profile = profile(fleet, index, port);
            let config = config(&profile, port, &network_ids);
            if matches!(profile, Profile::Access { .. } | Profile::Trunk { .. }) {
                let mut entry = config.clone();
                entry.insert("port_idx".into(), json!(port.index));
                overrides.push(Value::Object(entry));
            }
            table.push(table_row(&profile, port, config));
        }
        Some(json!({
            "_id": device.id.to_string(),
            "mac": device.mac.as_str(),
            "name": device.name,
            "model": device.model,
            "port_table": table,
            "port_overrides": overrides,
        }))
    }

    /// Session `rest/networkconf` rows for the fleet's networks.
    pub(crate) fn network_conf(&self) -> Vec<Value> {
        self.fleet
            .networks
            .iter()
            .map(|network| {
                json!({
                    "_id": network.id.to_string(),
                    "name": network.name,
                    "vlan": network.vlan_id,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
