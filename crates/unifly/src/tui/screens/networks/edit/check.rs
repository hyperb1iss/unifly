//! What a network draft has to satisfy before it is sent: the limits the
//! controller's API documents, checked here so the reason lands on the
//! field instead of coming back as a rejected save.

use std::net::Ipv4Addr;

use super::{Field, FormError, NetworkForm, Target};
use crate::tui::widgets::line_input::LineInput;

/// The prefix lengths the controller takes for a network's subnet.
const PREFIXES: std::ops::RangeInclusive<u32> = 8..=30;
/// The VLAN IDs a network other than the default one may use.
const VLANS: std::ops::RangeInclusive<u16> = 2..=4009;
/// The most DNS servers a DHCP server hands out.
const MAX_DNS_SERVERS: usize = 4;

/// A `host/prefix` value as the host, its prefix length, and the
/// subnet's first and last address.
fn parse_gateway(text: &str) -> Option<(Ipv4Addr, u32, Ipv4Addr, Ipv4Addr)> {
    let (host, prefix) = text.split_once('/')?;
    let host: Ipv4Addr = host.parse().ok()?;
    let prefix: u32 = prefix.parse().ok().filter(|prefix| *prefix <= 32)?;
    let mask = u32::MAX.checked_shl(32 - prefix).unwrap_or(0);
    let first = u32::from(host) & mask;
    Some((
        host,
        prefix,
        Ipv4Addr::from(first),
        Ipv4Addr::from(first | !mask),
    ))
}

impl NetworkForm {
    pub(super) fn vlan_id(&self) -> Result<u16, FormError> {
        let vlan = self.vlan.text().trim().parse::<u16>().ok();
        if self.is_default {
            return vlan
                .filter(|vlan| *vlan == 1)
                .ok_or_else(|| FormError::at(Field::Vlan, "The default network keeps VLAN 1"));
        }
        vlan.filter(|vlan| VLANS.contains(vlan)).ok_or_else(|| {
            FormError::at(
                Field::Vlan,
                format!(
                    "VLAN ID must be between {} and {}",
                    VLANS.start(),
                    VLANS.end()
                ),
            )
        })
    }

    /// The DNS servers the DHCP server hands out: up to four IPv4
    /// addresses, or none for the gateway's own.
    pub(super) fn dns_servers(&self) -> Result<Vec<String>, FormError> {
        let servers = self
            .dns
            .text()
            .split([',', ' '])
            .filter(|server| !server.is_empty())
            .map(|server| {
                server
                    .parse::<Ipv4Addr>()
                    .map(|address| address.to_string())
                    .map_err(|_| {
                        FormError::at(Field::Dns, format!("{server} is not an IPv4 address"))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if servers.len() > MAX_DNS_SERVERS {
            return Err(FormError::at(
                Field::Dns,
                format!("A DHCP server takes at most {MAX_DNS_SERVERS} DNS servers"),
            ));
        }
        Ok(servers)
    }

    /// Check the DHCP server's settings as a whole: they need the server
    /// on, a gateway to serve from, and a range inside its subnet.
    fn check_dhcp(&self) -> Result<(), FormError> {
        let settings_changed = [Field::RangeStart, Field::RangeStop, Field::Dns]
            .into_iter()
            .any(|field| self.changed(field));
        if !self.dhcp {
            return if settings_changed {
                Err(FormError::at(
                    Field::Dhcp,
                    "Turn the DHCP server on to change its range or DNS",
                ))
            } else {
                Ok(())
            };
        }
        let gateway = self.gateway.text().trim();
        if gateway.is_empty() && self.fields.contains(&Field::Gateway) {
            return Err(FormError::at(
                Field::Gateway,
                "A DHCP server needs a gateway address to serve from",
            ));
        }
        if !(settings_changed || self.changed(Field::Dhcp) || self.changed(Field::Gateway)) {
            return Ok(());
        }
        let address = |field: Field, input: &LineInput| {
            input
                .text()
                .trim()
                .parse::<Ipv4Addr>()
                .map_err(|_| FormError::at(field, "The range needs an IPv4 address here"))
        };
        let start = address(Field::RangeStart, &self.range_start)?;
        let stop = address(Field::RangeStop, &self.range_stop)?;
        if start > stop {
            return Err(FormError::at(
                Field::RangeStop,
                "DHCP stop must not be before DHCP start",
            ));
        }
        if let Some((host, _, network, broadcast)) = parse_gateway(gateway) {
            // The subnet's own address and its broadcast address are no
            // host's, and the gateway already has its own.
            let first = Ipv4Addr::from(u32::from(network).saturating_add(1));
            let last = Ipv4Addr::from(u32::from(broadcast).saturating_sub(1));
            for (field, bound) in [(Field::RangeStart, start), (Field::RangeStop, stop)] {
                if bound < first || bound > last {
                    return Err(FormError::at(
                        field,
                        format!("Must be inside {first} to {last}"),
                    ));
                }
            }
            if (start..=stop).contains(&host) {
                return Err(FormError::at(
                    Field::RangeStart,
                    format!("The range must not include the gateway's own address, {host}"),
                ));
            }
        }
        self.dns_servers().map(|_| ())
    }

    pub(super) fn check(&self) -> Result<(), FormError> {
        if self.name.text().trim().is_empty() {
            return Err(FormError::at(Field::Name, "A network needs a name"));
        }
        if self.target == Target::Create || self.changed(Field::Vlan) {
            self.vlan_id()?;
        }
        let gateway = self.gateway.text().trim();
        if gateway.is_empty() {
            if self.changed(Field::Gateway) && self.target != Target::Create {
                return Err(FormError::at(
                    Field::Gateway,
                    "The gateway address cannot be removed, only changed",
                ));
            }
        } else {
            let Some((_, prefix, ..)) = parse_gateway(gateway) else {
                return Err(FormError::at(
                    Field::Gateway,
                    "Give the gateway address with its prefix, like 10.0.60.1/24",
                ));
            };
            let typed = self.target == Target::Create || self.changed(Field::Gateway);
            if typed && !PREFIXES.contains(&prefix) {
                return Err(FormError::at(
                    Field::Gateway,
                    format!(
                        "The prefix must be between /{} and /{}",
                        PREFIXES.start(),
                        PREFIXES.end()
                    ),
                ));
            }
        }
        // Without a gateway address the network is created VLAN-only,
        // which has neither setting: they would be dropped unsaid.
        if self.target == Target::Create && gateway.is_empty() {
            for (field, set, setting) in [
                (Field::Isolation, self.isolation, "be isolated"),
                (
                    Field::Internet,
                    !self.internet,
                    "have its internet access cut",
                ),
            ] {
                if set {
                    return Err(FormError::at(
                        field,
                        format!("A network needs a gateway address to {setting}"),
                    ));
                }
            }
        }
        self.check_dhcp()
    }
}
