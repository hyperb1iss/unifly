// ── IPv6 address reach ──
//
// A host on an IPv6 network holds several addresses at once: one or more
// the internet routes to, often one only the site routes, and one valid on
// its link alone. Which is which decides what a reader can use it for.

use std::net::Ipv6Addr;

/// How far an IPv6 address reaches. Orders from widest to narrowest, so
/// sorting by it puts the addresses a reader is most likely after first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ipv6Scope {
    /// Routed on the internet.
    Global,
    /// Routed inside the site only (`fc00::/7`).
    UniqueLocal,
    /// Valid on one link only (`fe80::/10`).
    LinkLocal,
}

impl Ipv6Scope {
    pub fn of(address: &Ipv6Addr) -> Self {
        if address.is_unicast_link_local() {
            Self::LinkLocal
        } else if address.is_unique_local() {
            Self::UniqueLocal
        } else {
            Self::Global
        }
    }
}

/// The addresses in `addresses` that reach as far as `scope`, in order.
pub fn ipv6_in_scope(
    addresses: &[Ipv6Addr],
    scope: Ipv6Scope,
) -> impl Iterator<Item = &Ipv6Addr> + '_ {
    addresses
        .iter()
        .filter(move |address| Ipv6Scope::of(address) == scope)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(text: &str) -> Ipv6Scope {
        Ipv6Scope::of(&text.parse().expect("an IPv6 address"))
    }

    #[test]
    fn each_range_has_its_reach() {
        assert_eq!(scope("2001:db8:3fa::1"), Ipv6Scope::Global);
        assert_eq!(scope("fd00:c0de::2"), Ipv6Scope::UniqueLocal);
        assert_eq!(scope("fc12::9"), Ipv6Scope::UniqueLocal);
        assert_eq!(scope("fe80::1c2d:3eff:fe4f:5a6b"), Ipv6Scope::LinkLocal);
        assert!(Ipv6Scope::Global < Ipv6Scope::UniqueLocal);
        assert!(Ipv6Scope::UniqueLocal < Ipv6Scope::LinkLocal);
    }
}
