//! How a search query matches the addresses screens search by.

use std::net::IpAddr;

/// Whether a lowercased search `query` matches one of `addresses`: as part
/// of one written out, or, when the query is a whole address, by value, so
/// any spelling of an IPv6 address finds its holder.
pub fn address_matches(addresses: impl IntoIterator<Item = IpAddr>, query: &str) -> bool {
    let whole = query.parse::<IpAddr>().ok();
    addresses
        .into_iter()
        .any(|ip| Some(ip) == whole || ip.to_string().contains(query))
}

#[cfg(test)]
mod tests {
    use super::address_matches;

    #[test]
    fn a_whole_address_matches_by_value_and_a_fragment_as_text() {
        let held = [
            "10.4.20.198".parse().expect("v4"),
            "2001:db8::1".parse().expect("v6"),
        ];
        assert!(address_matches(held, "2001:0db8:0:0:0:0:0:1"));
        assert!(address_matches(held, "db8::1"));
        assert!(address_matches(held, "10.4.20.1"));
        assert!(!address_matches(held, "2001:db8::2"));
    }
}
