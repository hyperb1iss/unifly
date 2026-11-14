//! The detail panel: everything about the selected row that its table
//! has no room for.

use std::collections::HashMap;

use ratatui::style::Style;
use ratatui::text::Line;
use unifly_api::EntityId;
use unifly_api::model::{
    AclAction, AclRule, EntityOrigin, FirewallPolicy, FirewallZone, IpSpec, IpVersion, NatPolicy,
    PolicyEndpoint, PortSpec, TrafficFilter,
};

use super::policies::{action_color, action_label, zone_networks_label};
use super::rules::{acl_type_label, nat_endpoint};
use crate::tui::action::FirewallSubTab;
use crate::tui::screens::firewall::FirewallScreen;
use crate::tui::theme;
use crate::tui::widgets::detail_grid::{DetailGrid, NONE};

fn value() -> Style {
    Style::default().fg(theme::accent_secondary())
}

fn muted() -> Style {
    Style::default().fg(theme::text_muted())
}

/// `On`/`Off` (or other words) in the color that says which.
fn switch(on: bool, yes: &'static str, no: &'static str) -> (&'static str, Style) {
    if on {
        (yes, Style::default().fg(theme::success()))
    } else {
        (no, muted())
    }
}

fn origin_label(origin: Option<EntityOrigin>) -> &'static str {
    match origin {
        Some(EntityOrigin::UserDefined) => "User-defined",
        Some(EntityOrigin::SystemDefined) => "System-defined, fixed",
        Some(EntityOrigin::Derived) => "Derived, fixed",
        Some(EntityOrigin::Orchestrated) => "Orchestrated, fixed",
        None => NONE,
    }
}

fn ports_text(ports: &PortSpec) -> String {
    match ports {
        PortSpec::Values {
            items,
            match_opposite,
        } => {
            let lead = if *match_opposite {
                "every port except"
            } else {
                "ports"
            };
            format!("{lead} {}", items.join(", "))
        }
        PortSpec::MatchingList {
            list_id,
            match_opposite,
        } => {
            let lead = if *match_opposite {
                "every port not in list"
            } else {
                "ports in list"
            };
            format!("{lead} {list_id}")
        }
    }
}

/// A traffic filter in full: every network, address, MAC, and port it
/// names, where the table shows a count or a cut-off summary.
fn filter_text(filter: &TrafficFilter, networks: &HashMap<&EntityId, &str>) -> String {
    let listed = |lead: &str, except: &str, opposite: bool, items: Vec<String>| {
        let lead = if opposite { except } else { lead };
        format!("{lead} {}", items.join(", "))
    };
    let (mut clauses, macs, port_spec) = match filter {
        TrafficFilter::Network {
            network_ids,
            match_opposite,
            mac_addresses,
            ports,
        } => {
            let resolved = network_ids
                .iter()
                .map(|id| {
                    networks
                        .get(id)
                        .map_or_else(|| id.to_string(), |name| (*name).to_owned())
                })
                .collect();
            let text = listed(
                "networks",
                "every network except",
                *match_opposite,
                resolved,
            );
            (vec![text], mac_addresses.as_slice(), ports.as_ref())
        }
        TrafficFilter::IpAddress {
            addresses,
            match_opposite,
            mac_addresses,
            ports,
        } => {
            let items = addresses
                .iter()
                .map(|address| match address {
                    IpSpec::Address { value } | IpSpec::Subnet { value } => value.clone(),
                    IpSpec::Range { start, stop } => format!("{start} to {stop}"),
                    IpSpec::MatchingList { list_id } => format!("list {list_id}"),
                })
                .collect();
            let text = listed("addresses", "every address except", *match_opposite, items);
            (vec![text], mac_addresses.as_slice(), ports.as_ref())
        }
        TrafficFilter::MacAddress {
            mac_addresses,
            ports,
        } => (Vec::new(), mac_addresses.as_slice(), ports.as_ref()),
        TrafficFilter::Port { ports } => (Vec::new(), [].as_slice(), Some(ports)),
        TrafficFilter::Region { regions, ports } => (
            vec![format!("regions {}", regions.join(", "))],
            [].as_slice(),
            ports.as_ref(),
        ),
        TrafficFilter::Application {
            application_ids,
            ports,
        } => {
            let ids: Vec<String> = application_ids.iter().map(ToString::to_string).collect();
            (
                vec![format!("applications {}", ids.join(", "))],
                [].as_slice(),
                ports.as_ref(),
            )
        }
        TrafficFilter::ApplicationCategory {
            category_ids,
            ports,
        } => {
            let ids: Vec<String> = category_ids.iter().map(ToString::to_string).collect();
            (
                vec![format!("application categories {}", ids.join(", "))],
                [].as_slice(),
                ports.as_ref(),
            )
        }
        TrafficFilter::Domain { domains, ports } => (
            vec![format!("domains {}", domains.join(", "))],
            [].as_slice(),
            ports.as_ref(),
        ),
        TrafficFilter::Other { raw_type } => (vec![raw_type.clone()], [].as_slice(), None),
    };
    if !macs.is_empty() {
        clauses.push(format!("MACs {}", macs.join(", ")));
    }
    if let Some(ports) = port_spec {
        clauses.push(ports_text(ports));
    }
    clauses.join("; ")
}

/// One side of a policy in full. A side with no filter matches the whole
/// zone, and shows the controller's own summary of that.
fn endpoint_text(
    endpoint: &PolicyEndpoint,
    summary: Option<&str>,
    networks: &HashMap<&EntityId, &str>,
) -> String {
    match &endpoint.filter {
        Some(filter) => filter_text(filter, networks),
        None => summary.unwrap_or("Anything in the zone").to_owned(),
    }
}

impl FirewallScreen {
    /// The detail panel for the selected row: its title and its lines for
    /// a `width`-cell interior.
    pub(super) fn detail(&self, width: u16) -> Option<(String, Vec<Line<'static>>)> {
        let (title, grid) = match self.sub_tab {
            FirewallSubTab::Policies => {
                let policy = self.selected_policy()?;
                (policy.name.clone(), self.policy_grid(&policy))
            }
            FirewallSubTab::Zones => {
                let zone = self.selected_zone()?;
                (zone.name.clone(), self.zone_grid(zone))
            }
            FirewallSubTab::AclRules => {
                let rule = self.selected_acl_rule()?;
                (rule.name.clone(), self.acl_grid(rule))
            }
            FirewallSubTab::NatPolicies => {
                let rule = self.selected_nat_policy()?;
                (rule.name.clone(), self.nat_grid(rule))
            }
        };
        Some((title, grid.lines(width)))
    }

    fn network_names(&self) -> HashMap<&EntityId, &str> {
        self.networks
            .iter()
            .map(|network| (&network.id, network.name.as_str()))
            .collect()
    }

    fn policy_grid(&self, policy: &FirewallPolicy) -> DetailGrid {
        let networks = self.network_names();
        let mut grid = DetailGrid::default();
        grid.field(
            "Action",
            action_label(policy.action),
            Style::default().fg(action_color(policy.action)),
        );
        let (status, style) = switch(policy.enabled, "Enabled", "Disabled");
        grid.field("Status", status, style);
        grid.field("Zones", self.zone_pair_label(policy), value());
        let position = self
            .policy_position()
            .map_or_else(|| NONE.to_owned(), |(at, of)| format!("{at} of {of}"));
        grid.field("Position", position, value());
        let protocol = policy.protocol_summary.as_deref().unwrap_or("Any");
        grid.field("Protocol", protocol, value());
        grid.field(
            "IP version",
            match policy.ip_version {
                IpVersion::Ipv4 => "IPv4",
                IpVersion::Ipv6 => "IPv6",
                IpVersion::Both => "IPv4 and IPv6",
            },
            value(),
        );
        let (logging, style) = switch(policy.logging_enabled, "On", "Off");
        grid.field("Logging", logging, style);
        grid.field("Origin", origin_label(policy.origin), value());
        let connections = if policy.connection_states.is_empty() {
            "Any".to_owned()
        } else {
            policy.connection_states.join(", ")
        };
        grid.field("States", connections, value());
        grid.field(
            "Schedule",
            policy.schedule.as_deref().unwrap_or("Always"),
            value(),
        );
        if let Some(ipsec) = policy.ipsec_mode.as_deref() {
            grid.field("IPsec", ipsec, value());
        }
        grid.wide(
            "Source",
            endpoint_text(&policy.source, policy.source_summary.as_deref(), &networks),
            value(),
        );
        grid.wide(
            "Destination",
            endpoint_text(
                &policy.destination,
                policy.destination_summary.as_deref(),
                &networks,
            ),
            value(),
        );
        if let Some(description) = policy
            .description
            .as_deref()
            .filter(|text| !text.is_empty())
        {
            grid.wide("Description", description, value());
        }
        grid
    }

    fn zone_grid(&self, zone: &FirewallZone) -> DetailGrid {
        let (from, to) = self.zone_policy_counts(zone);
        let mut grid = DetailGrid::default();
        grid.field("Origin", origin_label(zone.origin), value());
        grid.field("Policies from", from.to_string(), value());
        grid.field("Policies to", to.to_string(), value());
        grid.wide(
            "Networks",
            zone_networks_label(zone, &self.network_names()),
            value(),
        );
        grid
    }

    fn acl_grid(&self, rule: &AclRule) -> DetailGrid {
        let position = self.acl_table.selected().unwrap_or(0) + 1;
        let mut grid = DetailGrid::default();
        let (action, color) = match rule.action {
            AclAction::Allow => ("Allow", theme::success()),
            AclAction::Block => ("Block", theme::error()),
        };
        grid.field("Action", action, Style::default().fg(color));
        let (status, style) = switch(rule.enabled, "Enabled", "Disabled");
        grid.field("Status", status, style);
        grid.field("Type", acl_type_label(rule.rule_type), value());
        grid.field(
            "Position",
            format!("{position} of {}", self.acl_rules.len()),
            value(),
        );
        grid.field("Origin", origin_label(rule.origin), value());
        grid.wide(
            "Source",
            rule.source_summary.as_deref().unwrap_or("Any"),
            value(),
        );
        grid.wide(
            "Destination",
            rule.destination_summary.as_deref().unwrap_or("Any"),
            value(),
        );
        grid
    }

    fn nat_grid(&self, rule: &NatPolicy) -> DetailGrid {
        let position = self.nat_table.selected().unwrap_or(0) + 1;
        let or_any = |text: String| {
            if text == NONE {
                ("Any".to_owned(), muted())
            } else {
                (text, value())
            }
        };
        let mut grid = DetailGrid::default();
        grid.field("Type", format!("{:?}", rule.nat_type), value());
        let (status, style) = switch(rule.enabled, "Enabled", "Disabled");
        grid.field("Status", status, style);
        grid.field(
            "Protocol",
            rule.protocol.as_deref().unwrap_or("Any"),
            value(),
        );
        grid.field(
            "Position",
            format!("{position} of {}", self.nat_policies.len()),
            value(),
        );
        grid.field("Origin", origin_label(rule.origin), value());
        let (source, style) = or_any(nat_endpoint(
            rule.src_address.as_ref(),
            rule.src_port.as_ref(),
        ));
        grid.wide("Source", source, style);
        let (destination, style) = or_any(nat_endpoint(
            rule.dst_address.as_ref(),
            rule.dst_port.as_ref(),
        ));
        grid.wide("Destination", destination, style);
        let (translation, style) = or_any(nat_endpoint(
            rule.translated_address.as_ref(),
            rule.translated_port.as_ref(),
        ));
        grid.wide("Translation", translation, style);
        if let Some(interface) = &rule.interface_id {
            grid.wide("Interface", interface.to_string(), value());
        }
        if let Some(description) = rule.description.as_deref().filter(|text| !text.is_empty()) {
            grid.wide("Description", description, value());
        }
        grid
    }
}
