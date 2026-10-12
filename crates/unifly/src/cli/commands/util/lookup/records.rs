//! [`Lookup`] for every record type a command can target by identifier.

use std::fmt::Write as _;
use std::net::IpAddr;

use unifly_api::model::{DnsPolicy, Voucher, WifiBroadcast};
use unifly_api::{
    AclRule, Client, Device, EntityId, FirewallGroup, FirewallPolicy, FirewallZone, MacAddress,
    NatPolicy, Network, TrafficMatchingList,
};

use super::Lookup;

fn same_id(id: &EntityId, needle: &str) -> bool {
    id.to_string().eq_ignore_ascii_case(needle)
}

/// `name (id ..., mac ..., ip ...)` for records with network identity.
fn describe_host(
    name: Option<&str>,
    id: &EntityId,
    mac: &MacAddress,
    ip: Option<IpAddr>,
) -> String {
    let mut line = format!("{} (id {id}, mac {mac}", name.unwrap_or("(unnamed)"));
    if let Some(ip) = ip {
        let _ = write!(line, ", ip {ip}");
    }
    line.push(')');
    line
}

impl Lookup for Device {
    const KIND: &'static str = "device";
    const LIST: &'static str = "devices list";

    fn has_id(&self, needle: &str) -> bool {
        same_id(&self.id, needle)
    }

    fn mac(&self) -> Option<&MacAddress> {
        Some(&self.mac)
    }

    fn ip(&self) -> Option<IpAddr> {
        self.ip
    }

    fn names(&self) -> Vec<&str> {
        self.name.as_deref().into_iter().collect()
    }

    fn describe(&self) -> String {
        describe_host(self.name.as_deref(), &self.id, &self.mac, self.ip)
    }
}

impl Lookup for Client {
    const KIND: &'static str = "client";
    const LIST: &'static str = "clients list";

    fn has_id(&self, needle: &str) -> bool {
        same_id(&self.id, needle)
    }

    fn mac(&self) -> Option<&MacAddress> {
        Some(&self.mac)
    }

    fn ip(&self) -> Option<IpAddr> {
        self.ip
    }

    fn names(&self) -> Vec<&str> {
        self.name
            .as_deref()
            .into_iter()
            .chain(self.hostname.as_deref())
            .collect()
    }

    fn describe(&self) -> String {
        let label = self.name.as_deref().or(self.hostname.as_deref());
        describe_host(label, &self.id, &self.mac, self.ip)
    }
}

/// Records identified by an ID and a single display name.
macro_rules! named_record {
    ($record:ty, $kind:literal, $list:literal) => {
        impl Lookup for $record {
            const KIND: &'static str = $kind;
            const LIST: &'static str = $list;

            fn has_id(&self, needle: &str) -> bool {
                same_id(&self.id, needle)
            }

            fn names(&self) -> Vec<&str> {
                vec![self.name.as_str()]
            }

            fn describe(&self) -> String {
                format!("{} (id {})", self.name, self.id)
            }
        }
    };
}

named_record!(Network, "network", "networks list");
named_record!(WifiBroadcast, "WiFi broadcast", "wifi list");
named_record!(FirewallPolicy, "firewall policy", "firewall policies list");
named_record!(FirewallZone, "firewall zone", "firewall zones list");
named_record!(NatPolicy, "NAT policy", "nat policies list");
named_record!(AclRule, "ACL rule", "acl list");
named_record!(
    TrafficMatchingList,
    "traffic matching list",
    "traffic-lists list"
);

impl Lookup for FirewallGroup {
    const KIND: &'static str = "firewall group";
    const LIST: &'static str = "firewall groups list";

    /// Policies reference groups by `external_id`, so it counts as an ID.
    fn has_id(&self, needle: &str) -> bool {
        same_id(&self.id, needle)
            || self
                .external_id
                .as_deref()
                .is_some_and(|external| external.eq_ignore_ascii_case(needle))
    }

    fn names(&self) -> Vec<&str> {
        vec![self.name.as_str()]
    }

    fn describe(&self) -> String {
        format!("{} (id {})", self.name, self.id)
    }
}

impl Lookup for DnsPolicy {
    const KIND: &'static str = "DNS policy";
    const LIST: &'static str = "dns list";

    fn has_id(&self, needle: &str) -> bool {
        same_id(&self.id, needle)
    }

    /// A DNS record answers to its domain; an A and an AAAA record for the
    /// same name are ambiguous and need the ID.
    fn names(&self) -> Vec<&str> {
        vec![self.domain.as_str()]
    }

    fn describe(&self) -> String {
        format!(
            "{} {} {} (id {})",
            self.domain, self.policy_type, self.value, self.id
        )
    }
}

impl Lookup for Voucher {
    const KIND: &'static str = "voucher";
    const LIST: &'static str = "hotspot list";

    fn has_id(&self, needle: &str) -> bool {
        same_id(&self.id, needle)
    }

    /// Vouchers answer to their note and to the code printed on them.
    fn names(&self) -> Vec<&str> {
        self.name
            .as_deref()
            .into_iter()
            .chain([self.code.as_str()])
            .collect()
    }

    fn describe(&self) -> String {
        match self.name.as_deref() {
            Some(name) => format!("{} {name} (id {})", self.code, self.id),
            None => format!("{} (id {})", self.code, self.id),
        }
    }
}
