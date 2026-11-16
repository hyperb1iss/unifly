use std::net::IpAddr;

pub(super) fn parse_ipv6_from_text(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if let Ok(ip) = trimmed.parse::<IpAddr>()
        && ip.is_ipv6()
    {
        return Some(ip.to_string());
    }

    for token in trimmed.split([',', ';', ' ', '\t', '\n']) {
        let cleaned = token.trim_matches(|c: char| matches!(c, '[' | ']' | '(' | ')' | '"' | '\''));
        if cleaned.is_empty() {
            continue;
        }
        if let Ok(ip) = cleaned.parse::<IpAddr>()
            && ip.is_ipv6()
        {
            return Some(ip.to_string());
        }
        if let Some((_, candidate)) = cleaned.split_once('=')
            && let Ok(ip) = candidate.trim().parse::<IpAddr>()
            && ip.is_ipv6()
        {
            return Some(ip.to_string());
        }
    }

    None
}

pub(super) fn parse_ipv6_from_value(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => parse_ipv6_from_text(s),
        serde_json::Value::Array(items) => items.iter().find_map(parse_ipv6_from_value),
        serde_json::Value::Object(obj) => {
            const PRIORITY_KEYS: &[&str] = &[
                "wan_ip6",
                "wan_ip6s",
                "wan_ipv6",
                "ip6",
                "ip6Address",
                "ip6_address",
                "ipv6",
                "ipv6Address",
                "ipv6_address",
                "address",
                "ipAddress",
                "ip_address",
                "ip",
                "value",
            ];

            for key in PRIORITY_KEYS {
                if let Some(ipv6) = obj.get(*key).and_then(parse_ipv6_from_value) {
                    return Some(ipv6);
                }
            }

            obj.values().find_map(parse_ipv6_from_value)
        }
        _ => None,
    }
}

pub(super) fn fmt_rate_compact(bytes_per_sec: u64) -> String {
    let bits = bytes_per_sec.saturating_mul(8);
    let with_decimal = |value: u64, unit: u64, suffix: &str| -> String {
        let scaled = value.saturating_mul(10) / unit;
        format!("{}.{}{}", scaled / 10, scaled % 10, suffix)
    };

    if bits >= 1_000_000_000 {
        with_decimal(bits, 1_000_000_000, "G")
    } else if bits >= 1_000_000 {
        with_decimal(bits, 1_000_000, "M")
    } else if bits >= 1_000 {
        with_decimal(bits, 1_000, "K")
    } else {
        format!("{bits}b")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ipv6_from_text_tokens() {
        assert_eq!(
            parse_ipv6_from_text("wan=fe80::1, backup=2001:db8::2"),
            Some("fe80::1".into())
        );
    }

    #[test]
    fn formats_compact_rates() {
        assert_eq!(fmt_rate_compact(0), "0b");
        assert_eq!(fmt_rate_compact(125), "1.0K");
        assert_eq!(fmt_rate_compact(125_000), "1.0M");
    }

    #[test]
    fn parses_ipv6_from_nested_json() {
        let value = serde_json::json!({
            "outer": {
                "ip6Address": "2001:db8::42"
            }
        });

        assert_eq!(parse_ipv6_from_value(&value), Some("2001:db8::42".into()));
    }
}
