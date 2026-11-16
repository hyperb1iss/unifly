use std::sync::Arc;

use ratatui::style::Style;

use unifly_api::Network;
use unifly_api::model::{FirewallZone, NetworkManagement, NetworkPurpose};

use crate::tui::screens::networks::state::format_lease_time;
use crate::tui::theme;
use crate::tui::widgets::detail_grid::{DetailGrid, NONE};

/// The zone a network belongs to: the one it names, or the one that lists
/// it as a member.
fn zone_name<'a>(network: &Network, zones: &'a [Arc<FirewallZone>]) -> Option<&'a str> {
    zones
        .iter()
        .find(|zone| {
            network.firewall_zone_id.as_ref() == Some(&zone.id)
                || zone.network_ids.contains(&network.id)
        })
        .map(|zone| zone.name.as_str())
}

pub(super) fn management_label(management: Option<NetworkManagement>) -> &'static str {
    match management {
        Some(NetworkManagement::Gateway) => "Gateway",
        Some(NetworkManagement::Switch) => "Switch",
        Some(NetworkManagement::Unmanaged) => "Unmanaged",
        None => NONE,
    }
}

/// How a network hands out addresses: its own server, a relay to
/// another, or neither. A relay is not "off": clients still get leases.
pub(super) fn dhcp_label(network: &Network) -> &'static str {
    match network.dhcp.as_ref() {
        None => NONE,
        Some(dhcp) if dhcp.enabled => "On",
        Some(dhcp) if !dhcp.relay_servers.is_empty() => "Relay",
        Some(_) => "Off",
    }
}

/// Everything known about a network, as the detail panel's fields.
pub(super) fn detail_grid(network: &Network, zones: &[Arc<FirewallZone>]) -> DetailGrid {
    let value = Style::default().fg(theme::accent_secondary());
    let on = Style::default().fg(theme::success());
    let off = Style::default().fg(theme::text_muted());
    let switch = |enabled: bool| {
        if enabled { ("On", on) } else { ("Off", off) }
    };
    let or_none = |text: Option<String>| match text {
        Some(text) => (text, value),
        None => (NONE.to_owned(), off),
    };

    let mut grid = DetailGrid::default();
    let (status, status_style) = if network.enabled {
        ("Enabled", on)
    } else {
        ("Disabled", Style::default().fg(theme::warning()))
    };
    grid.field("Status", status, status_style);
    let purpose = network.purpose.map(|purpose| match purpose {
        NetworkPurpose::Corporate => "Corporate",
        NetworkPurpose::Guest => "Guest",
        NetworkPurpose::Wan => "WAN",
        NetworkPurpose::VlanOnly => "VLAN only",
    });
    let (purpose, style) = or_none(purpose.map(str::to_owned));
    grid.field("Purpose", purpose, style);
    let (vlan, style) = or_none(network.vlan_id.map(|vlan| vlan.to_string()));
    grid.field("VLAN", vlan, style);
    let (gateway, style) = or_none(network.gateway_ip.map(|ip| ip.to_string()));
    grid.field("Gateway", gateway, style);
    let (subnet, style) = or_none(network.subnet.clone());
    grid.field("Subnet", subnet, style);
    // After the addresses, so a gateway-managed network does not read
    // `Type Gateway` right beside the `Gateway` address label.
    grid.field("Type", management_label(network.management), value);
    let (zone, style) = or_none(zone_name(network, zones).map(str::to_owned));
    grid.field("Zone", zone, style);
    grid.field(
        "Default",
        if network.is_default { "Yes" } else { "No" },
        value,
    );

    let dhcp = network.dhcp.as_ref();
    let label = dhcp_label(network);
    grid.field("DHCP", label, if label == "On" { on } else { off });
    let lease = dhcp
        .and_then(|dhcp| dhcp.lease_time_secs)
        .map(format_lease_time);
    let (lease, style) = or_none(lease);
    grid.field("Lease", lease, style);
    if let Some(dhcp) = dhcp.filter(|dhcp| dhcp.enabled) {
        let bound =
            |ip: Option<std::net::Ipv4Addr>| ip.map_or_else(|| "?".to_owned(), |ip| ip.to_string());
        grid.wide(
            "Range",
            format!("{} to {}", bound(dhcp.range_start), bound(dhcp.range_stop)),
            value,
        );
    }
    if let Some(dhcp) = dhcp.filter(|dhcp| !dhcp.enabled && !dhcp.relay_servers.is_empty()) {
        let servers: Vec<String> = dhcp.relay_servers.iter().map(ToString::to_string).collect();
        grid.wide("Relay to", servers.join(", "), value);
    }
    if let Some(dhcp) = dhcp.filter(|dhcp| !dhcp.dns_servers.is_empty()) {
        let servers: Vec<String> = dhcp.dns_servers.iter().map(ToString::to_string).collect();
        grid.wide("DNS", servers.join(", "), value);
    }

    let (label, style) = switch(network.internet_access_enabled);
    grid.field("Internet", label, style);
    let (label, style) = switch(network.isolation_enabled);
    grid.field("Isolation", label, style);
    let (label, style) = switch(network.mdns_forwarding_enabled);
    grid.field("mDNS", label, style);
    let (label, style) = switch(network.cellular_backup_enabled);
    grid.field("Cellular", label, style);
    ipv6_fields(&mut grid, network);
    grid
}

/// The fields that describe the network's IPv6 setup: whether it is on,
/// then its subnets and how hosts are addressed and routed.
fn ipv6_fields(grid: &mut DetailGrid, network: &Network) {
    let value = Style::default().fg(theme::accent_secondary());
    let on = Style::default().fg(theme::success());
    let off = Style::default().fg(theme::text_muted());
    if !network.ipv6_enabled {
        grid.field("IPv6", "Off", off);
        return;
    }
    let mode = network.ipv6_mode.as_ref();
    grid.field(
        "IPv6",
        mode.map_or_else(|| "On".to_owned(), ToString::to_string),
        on,
    );
    // A delegated prefix is known once the gateway reports it.
    match &network.ipv6_prefix {
        Some(prefix) => grid.wide("Prefix", prefix.clone(), value),
        None => grid.wide("Prefix", NONE, off),
    };
    if !network.ipv6_additional_subnets.is_empty() {
        grid.wide("Aliases", network.ipv6_additional_subnets.join(", "), value);
    }
    let dhcpv6 = network.dhcpv6_enabled.then(|| {
        match (&network.dhcpv6_range_start, &network.dhcpv6_range_stop) {
            (Some(start), Some(stop)) => format!("DHCPv6 {start} to {stop}"),
            _ => "DHCPv6".to_owned(),
        }
    });
    let addressing = match (network.slaac_enabled, dhcpv6) {
        (true, Some(dhcpv6)) => Some(format!("SLAAC and {dhcpv6}")),
        (true, None) => Some("SLAAC".to_owned()),
        (false, dhcpv6) => dhcpv6,
    };
    // Without SLAAC or DHCPv6, hosts get no IPv6 address at all.
    let warning = Style::default().fg(theme::warning());
    // A DHCPv6 range needs the width of the pane; SLAAC alone does not.
    match addressing {
        Some(addressing) if network.dhcpv6_range_start.is_some() => {
            grid.wide("Addresses", addressing, value)
        }
        Some(addressing) => grid.field("Addresses", addressing, value),
        None => grid.field("Addresses", "none", warning),
    };
    // Without router advertisements hosts have no IPv6 default route.
    match network.ipv6_ra_priority {
        Some(priority) => grid.field("RA", format!("{priority} priority"), value),
        None => grid.field("RA", "Off", warning),
    };
    // Without an override the gateway picks the servers, which needs no row.
    if !network.ipv6_dns_servers.is_empty() {
        let dns: Vec<String> = network
            .ipv6_dns_servers
            .iter()
            .map(ToString::to_string)
            .collect();
        grid.wide("DNSv6", dns.join(", "), value);
    }
}
