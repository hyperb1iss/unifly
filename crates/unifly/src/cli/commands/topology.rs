//! Network topology visualization.
//!
//! Builds a tree view of the network: gateway → devices → clients,
//! grouped by uplink device and annotated with VLAN, signal, and type info.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;

use serde::Serialize;
use unifly_api::{Client, ClientType, Controller, Device, DeviceState, DeviceType, Network};

use crate::cli::args::{GlobalOpts, OutputFormat};
use crate::cli::error::CliError;
use crate::cli::output;

/// The tree as data, for the structured output formats.
#[derive(Serialize)]
struct Topology {
    gateway: Option<TopologyDevice>,
    devices: Vec<TopologyDevice>,
    unlinked_clients: Vec<TopologyClient>,
}

#[derive(Serialize)]
struct TopologyDevice {
    id: String,
    mac: String,
    name: Option<String>,
    model: Option<String>,
    device_type: DeviceType,
    state: DeviceState,
    ip: Option<std::net::IpAddr>,
    /// The controller's count, which can include clients it reaches
    /// through other devices.
    client_count: Option<u32>,
    /// Clients whose uplink is this device.
    clients: Vec<TopologyClient>,
}

#[derive(Serialize)]
struct TopologyClient {
    id: String,
    mac: String,
    name: Option<String>,
    ip: Option<std::net::IpAddr>,
    /// The network whose subnet holds the client's IP.
    network: Option<String>,
    client_type: ClientType,
    signal_dbm: Option<i32>,
}

impl TopologyDevice {
    fn new(
        device: &Device,
        clients_by_uplink: &HashMap<String, Vec<&Arc<Client>>>,
        networks: &[Arc<Network>],
    ) -> Self {
        let clients = clients_by_uplink
            .get(device.mac.as_str())
            .map_or(&[][..], Vec::as_slice);
        Self {
            id: device.id.to_string(),
            mac: device.mac.to_string(),
            name: device.name.clone(),
            model: device.model.clone(),
            device_type: device.device_type,
            state: device.state,
            ip: device.ip,
            client_count: device.client_count,
            clients: clients
                .iter()
                .map(|client| TopologyClient::new(client, networks))
                .collect(),
        }
    }
}

impl TopologyClient {
    fn new(client: &Client, networks: &[Arc<Network>]) -> Self {
        let network = vlan_label(client.ip, networks);
        Self {
            id: client.id.to_string(),
            mac: client.mac.to_string(),
            name: client.name.clone().or_else(|| client.hostname.clone()),
            ip: client.ip,
            network: (network != "?").then_some(network),
            client_type: client.client_type,
            signal_dbm: client.wireless.as_ref().and_then(|w| w.signal_dbm),
        }
    }
}

#[allow(clippy::too_many_lines, clippy::unused_async)]
pub async fn handle(controller: &Controller, global: &GlobalOpts) -> Result<(), CliError> {
    // Snapshots come out of a hash map; sort so the tree reads the same on
    // every run (devices and clients by name, as the list commands do).
    let devices = super::util::order::devices(&controller.devices_snapshot());
    let clients = super::util::order::clients(&controller.clients_snapshot());
    let networks = controller.networks_snapshot();
    let p = output::Painter::new(global);

    // Build MAC → device lookup
    let _device_by_mac: HashMap<&str, &Arc<Device>> =
        devices.iter().map(|d| (d.mac.as_str(), d)).collect();

    // Group clients by uplink device MAC
    let mut clients_by_uplink: HashMap<String, Vec<&Arc<Client>>> = HashMap::new();
    let mut unlinked: Vec<&Arc<Client>> = Vec::new();

    for client in &clients {
        if let Some(ref uplink_mac) = client.uplink_device_mac {
            clients_by_uplink
                .entry(uplink_mac.to_string())
                .or_default()
                .push(client);
        } else {
            unlinked.push(client);
        }
    }

    // Find the gateway
    let gateway = devices
        .iter()
        .find(|d| d.device_type == unifly_api::DeviceType::Gateway);

    // Separate infrastructure devices (non-gateway)
    let infra: Vec<&Arc<Device>> = devices
        .iter()
        .filter(|d| d.device_type != unifly_api::DeviceType::Gateway)
        .collect();

    // JSON and YAML get the tree as data. The drawing below lists only the
    // gateway's client count, so here its clients sit under it instead.
    if matches!(
        global.output,
        OutputFormat::Json | OutputFormat::JsonCompact | OutputFormat::Yaml
    ) {
        let topology = Topology {
            gateway: gateway.map(|gw| TopologyDevice::new(gw, &clients_by_uplink, &networks)),
            devices: infra
                .iter()
                .map(|device| TopologyDevice::new(device, &clients_by_uplink, &networks))
                .collect(),
            unlinked_clients: unlinked
                .iter()
                .map(|client| TopologyClient::new(client, &networks))
                .collect(),
        };
        let out = output::render_single(
            &global.output,
            &topology,
            |_| String::new(),
            |_| String::new(),
        );
        output::print_output(&out, global.quiet);
        return Ok(());
    }

    // Print gateway
    if let Some(gw) = gateway {
        let client_count = gw.client_count.unwrap_or(0);
        println!(
            "\u{256d} {} ({}) \u{00b7} {} \u{00b7} {} clients",
            p.keyword(gw.name.as_deref().unwrap_or("gateway")),
            p.muted(gw.model.as_deref().unwrap_or("?")),
            p.ip(&gw.ip.map_or("-".into(), |ip| ip.to_string())),
            p.number(&client_count.to_string()),
        );
    }

    let infra_count = infra.len();
    for (i, device) in infra.iter().enumerate() {
        let is_last_device = i == infra_count - 1 && unlinked.is_empty();
        let branch = if is_last_device {
            "\u{2570}"
        } else {
            "\u{251c}"
        };
        let cont = if is_last_device { " " } else { "\u{2502}" };

        let dev_type = match device.device_type {
            unifly_api::DeviceType::AccessPoint => "AP",
            unifly_api::DeviceType::Switch => "SW",
            _ => "??",
        };
        let dev_clients = clients_by_uplink
            .get(device.mac.as_str())
            .map_or(&[][..], Vec::as_slice);

        let state_str = match device.state {
            unifly_api::DeviceState::Offline => p.error(" [OFFLINE]"),
            unifly_api::DeviceState::Online => String::new(),
            _ => p.warning(" [?]"),
        };

        println!(
            "{branch}\u{2500}\u{2500} {} {} ({}) \u{00b7} {}{} \u{00b7} {} clients",
            p.muted(dev_type),
            p.name(device.name.as_deref().unwrap_or("?")),
            p.muted(device.model.as_deref().unwrap_or("?")),
            p.ip(&device.ip.map_or("-".into(), |ip| ip.to_string())),
            state_str,
            p.number(&dev_clients.len().to_string()),
        );

        // Print clients under this device
        let client_count = dev_clients.len();
        for (j, client) in dev_clients.iter().enumerate() {
            let is_last_client = j == client_count - 1;
            let cbranch = if is_last_client {
                "\u{2570}"
            } else {
                "\u{251c}"
            };

            let vlan = vlan_label(client.ip, &networks);
            let signal = client
                .wireless
                .as_ref()
                .and_then(|w| w.signal_dbm)
                .map(|s| format!(" {s}dBm"))
                .unwrap_or_default();
            let ctype = match client.client_type {
                unifly_api::ClientType::Wired => "wire",
                unifly_api::ClientType::Wireless => "wifi",
                _ => "?",
            };

            let client_name = client
                .name
                .as_deref()
                .or(client.hostname.as_deref())
                .unwrap_or("?");
            let client_ip = client.ip.map_or("-".into(), |ip| ip.to_string());
            println!(
                "{cont}   {cbranch}\u{2500} [{}] {} \u{00b7} {} \u{00b7} {}{}",
                p.muted(&vlan),
                p.name(client_name),
                p.ip(&client_ip),
                p.muted(ctype),
                p.muted(&signal),
            );
        }
    }

    // Unlinked clients (no known uplink)
    if !unlinked.is_empty() {
        println!("\u{2570}\u{2500}\u{2500} (no uplink)");
        let count = unlinked.len();
        for (j, client) in unlinked.iter().enumerate() {
            let is_last = j == count - 1;
            let cbranch = if is_last { "\u{2570}" } else { "\u{251c}" };
            let vlan = vlan_label(client.ip, &networks);
            let client_name = client
                .name
                .as_deref()
                .or(client.hostname.as_deref())
                .unwrap_or("?");
            let client_ip = client.ip.map_or("-".into(), |ip| ip.to_string());
            println!(
                "    {cbranch}\u{2500} [{}] {} \u{00b7} {}",
                p.muted(&vlan),
                p.name(client_name),
                p.ip(&client_ip),
            );
        }
    }

    Ok(())
}

/// Determine VLAN/network label from a client's IP.
fn vlan_label(ip: Option<std::net::IpAddr>, networks: &[Arc<Network>]) -> String {
    let Some(std::net::IpAddr::V4(ip)) = ip else {
        return "?".into();
    };

    let ip_u32 = u32::from(ip);
    for net in networks {
        if let Some(ref subnet_str) = net.subnet
            && let Some((net_addr, prefix)) = parse_cidr(subnet_str)
        {
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            if (ip_u32 & mask) == (u32::from(net_addr) & mask) {
                return net.name.clone();
            }
        }
    }
    "?".into()
}

fn parse_cidr(s: &str) -> Option<(Ipv4Addr, u32)> {
    let (addr_str, prefix_str) = s.split_once('/')?;
    let addr: Ipv4Addr = addr_str.parse().ok()?;
    let prefix: u32 = prefix_str.parse().ok()?;
    Some((addr, prefix))
}
