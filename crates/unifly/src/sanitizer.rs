use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use dashmap::DashMap;
use regex::Regex;

use unifly_api::model::{AclRule, FirewallPolicy, FirewallZone, NatPolicy, WifiBroadcast};
use unifly_api::session::models::RogueAp;
use unifly_api::{Client, Device, Event, HealthSummary, MacAddress, Network};

use crate::config::DemoConfig;

const CUTE_NAMES: &[&str] = &[
    "Starling", "Moonbeam", "Pixel", "Cosmo", "Mochi", "Nimbus", "Wren", "Ziggy", "Sprocket",
    "Quasar", "Clover", "Ember", "Tinker", "Velvet", "Cricket", "Biscuit", "Phantom", "Stardust",
    "Marble", "Glimmer", "Waffle", "Pebble", "Fizz", "Comet",
];

/// What every IPv6 address is replaced with, from the documentation range.
const IPV6_STAND_IN: &str = "2001:db8::cafe";

const CUTE_SSIDS: &[&str] = &[
    "SilkNet",
    "NeonWave",
    "PixelStream",
    "StarLink-ish",
    "WiFi Fairy",
    "Cloud Nine",
    "Byte Me",
    "LAN of Enchantment",
    "The Promised LAN",
];

#[allow(clippy::struct_excessive_bools)]
pub struct Sanitizer {
    redact_patterns: Vec<String>,
    keep_patterns: HashSet<String>,
    name_regex: Option<Regex>,
    ip_regex: Regex,
    /// Runs of the characters an IPv6 address is written with, where
    /// addresses are looked for.
    ipv6_run: Regex,
    redact_ssids: bool,
    redact_wan_ips: bool,
    redact_macs: bool,
    redact_isp: bool,
    name_map: DashMap<String, String>,
    ssid_map: DashMap<String, String>,
    ip_map: DashMap<IpAddr, IpAddr>,
    mac_map: DashMap<String, String>,
    /// Every stand-in handed out, so text or values that already carry
    /// one are never mapped a second time.
    name_stand_ins: DashMap<String, ()>,
    ssid_stand_ins: DashMap<String, ()>,
    mac_stand_ins: DashMap<String, ()>,
    name_counter: AtomicUsize,
    ssid_counter: AtomicUsize,
    wan_ip_counter: AtomicU8,
}

impl Sanitizer {
    pub fn new(config: &DemoConfig) -> Self {
        let redact_patterns: Vec<String> = config
            .redact_names
            .iter()
            .map(|n| n.to_lowercase())
            .collect();

        let keep_patterns: HashSet<String> =
            config.keep_names.iter().map(|n| n.to_lowercase()).collect();

        let name_regex = if redact_patterns.is_empty() {
            None
        } else {
            let escaped: Vec<String> = config
                .redact_names
                .iter()
                .map(|n| regex::escape(n))
                .collect();
            let pattern = format!("(?i)({})", escaped.join("|"));
            Regex::new(&pattern).ok()
        };

        let ip_regex =
            Regex::new(r"\b(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})\b").expect("valid regex");

        let ipv6_run = Regex::new(r"[0-9A-Fa-f:.]+").expect("valid regex");

        Self {
            redact_patterns,
            keep_patterns,
            name_regex,
            ip_regex,
            ipv6_run,
            redact_ssids: config.redact_ssids,
            redact_wan_ips: config.redact_wan_ips,
            redact_macs: config.redact_macs,
            redact_isp: config.redact_isp,
            name_map: DashMap::new(),
            ssid_map: DashMap::new(),
            ip_map: DashMap::new(),
            mac_map: DashMap::new(),
            name_stand_ins: DashMap::new(),
            ssid_stand_ins: DashMap::new(),
            mac_stand_ins: DashMap::new(),
            name_counter: AtomicUsize::new(1),
            ssid_counter: AtomicUsize::new(1),
            wan_ip_counter: AtomicU8::new(1),
        }
    }

    // ── Primitive sanitizers ───────────────────────────────────────

    pub fn sanitize_name(&self, name: &str) -> String {
        if self.redact_patterns.is_empty() {
            return name.to_owned();
        }

        let lower = name.to_lowercase();

        if self
            .keep_patterns
            .iter()
            .any(|k| lower.contains(k.as_str()))
        {
            return name.to_owned();
        }

        if self
            .redact_patterns
            .iter()
            .any(|p| lower.contains(p.as_str()))
        {
            return self.deterministic_name(name);
        }

        name.to_owned()
    }

    pub fn sanitize_name_opt(&self, name: &Option<String>) -> Option<String> {
        name.as_ref().map(|n| self.sanitize_name(n))
    }

    pub fn sanitize_wan_ip(&self, ip: IpAddr) -> IpAddr {
        if !self.redact_wan_ips || !is_public_ip(ip) {
            return ip;
        }

        if let Some(mapped) = self.ip_map.get(&ip) {
            return *mapped;
        }

        let idx = self.wan_ip_counter.fetch_add(1, Ordering::Relaxed);
        let replacement = IpAddr::V4(Ipv4Addr::new(198, 51, 100, idx.wrapping_add(1)));
        self.ip_map.insert(ip, replacement);
        replacement
    }

    pub fn sanitize_ip_opt(&self, ip: &Option<IpAddr>) -> Option<IpAddr> {
        ip.map(|addr| self.sanitize_wan_ip(addr))
    }

    pub fn sanitize_ssid(&self, ssid: &str) -> String {
        if !self.redact_ssids || self.ssid_stand_ins.contains_key(ssid) {
            return ssid.to_owned();
        }

        if let Some(mapped) = self.ssid_map.get(ssid) {
            return mapped.clone();
        }

        let idx = self.ssid_counter.fetch_add(1, Ordering::Relaxed);
        let cute = CUTE_SSIDS[idx % CUTE_SSIDS.len()];
        let replacement = if idx < CUTE_SSIDS.len() {
            cute.to_owned()
        } else {
            format!("{cute} {}", idx / CUTE_SSIDS.len() + 1)
        };
        self.ssid_map.insert(ssid.to_owned(), replacement.clone());
        self.ssid_stand_ins.insert(replacement.clone(), ());
        replacement
    }

    pub fn sanitize_mac(&self, mac: &MacAddress) -> MacAddress {
        if !self.redact_macs {
            return mac.clone();
        }

        let key = mac.as_str().to_owned();
        if self.mac_stand_ins.contains_key(&key) {
            return mac.clone();
        }
        if let Some(mapped) = self.mac_map.get(&key) {
            return MacAddress::new(mapped.value());
        }

        let hash = simple_hash(mac.as_str());
        let bytes = hash.to_le_bytes();
        let replacement = format!(
            "02:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4]
        );
        self.mac_map.insert(key, replacement.clone());
        self.mac_stand_ins.insert(replacement.clone(), ());
        MacAddress::new(replacement)
    }

    pub fn sanitize_mac_opt(&self, mac: &Option<MacAddress>) -> Option<MacAddress> {
        mac.as_ref().map(|m| self.sanitize_mac(m))
    }

    pub fn sanitize_text(&self, text: &str) -> String {
        let mut result = text.to_owned();

        if let Some(ref re) = self.name_regex
            && re.is_match(&result)
        {
            let stand_ins = self.stand_in_spans(&result);
            result = re
                .replace_all(&result, |caps: &regex::Captures| {
                    let found = caps.get(0).map_or(0..0, |found| found.range());
                    let inside = stand_ins
                        .iter()
                        .any(|span| span.start <= found.start && found.end <= span.end);
                    if inside {
                        caps[0].to_owned()
                    } else {
                        self.deterministic_name(&caps[0])
                    }
                })
                .into_owned();
        }

        if self.redact_wan_ips {
            result = self
                .ip_regex
                .replace_all(&result, |caps: &regex::Captures| {
                    let ip_str = &caps[0];
                    if let Ok(ip) = ip_str.parse::<IpAddr>()
                        && is_public_ip(ip)
                    {
                        return self.sanitize_wan_ip(ip).to_string();
                    }
                    ip_str.to_owned()
                })
                .into_owned();

            result = self.redact_ipv6(&result);
        }

        result
    }

    pub fn sanitize_wan_ip_str(&self, ip_str: &str) -> String {
        if !self.redact_wan_ips {
            return ip_str.to_owned();
        }
        if let Ok(ip) = ip_str.parse::<IpAddr>()
            && is_public_ip(ip)
        {
            return self.sanitize_wan_ip(ip).to_string();
        }
        if ip_str.trim().parse::<Ipv6Addr>().is_ok() {
            return IPV6_STAND_IN.to_owned();
        }
        ip_str.to_owned()
    }

    fn deterministic_name(&self, original: &str) -> String {
        if self.name_stand_ins.contains_key(original) {
            return original.to_owned();
        }
        let key = original.to_lowercase();
        if let Some(mapped) = self.name_map.get(&key) {
            return mapped.clone();
        }

        let idx = self.name_counter.fetch_add(1, Ordering::Relaxed);
        let cute = CUTE_NAMES[idx % CUTE_NAMES.len()];
        let replacement = if idx < CUTE_NAMES.len() {
            cute.to_owned()
        } else {
            format!("{cute}-{}", idx / CUTE_NAMES.len() + 1)
        };
        self.name_map.insert(key, replacement.clone());
        self.name_stand_ins.insert(replacement.clone(), ());
        replacement
    }

    /// Where stand-ins already sit in `text`. A redacted word found inside
    /// one is part of the stand-in, not a real name.
    fn stand_in_spans(&self, text: &str) -> Vec<Range<usize>> {
        self.name_stand_ins
            .iter()
            .chain(self.ssid_stand_ins.iter())
            .flat_map(|stand_in| {
                let len = stand_in.key().len();
                text.match_indices(stand_in.key().as_str())
                    .map(move |(start, _)| start..start + len)
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Replace each IPv6 address in `text` with the documentation stand-in.
    ///
    /// An address is the longest stretch of a hex, colon, and dot run that
    /// parses as one and touches no letter or digit outside it, so it is
    /// found glued to a label (`addr:2001:db8::1`), before a colon or a
    /// prefix length, or at the end of a sentence. MACs and clock times
    /// never parse, and the letters around `::` in a Rust path keep it from
    /// counting. Loopback and the unspecified address identify nothing and
    /// stay.
    fn redact_ipv6(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut copied = 0;
        for run in self.ipv6_run.find_iter(text) {
            if run.as_str().matches(':').count() < 2 {
                continue;
            }
            let mut start = run.start();
            while start < run.end() {
                let Some((end, ip)) = ipv6_at(text, start, run.end()) else {
                    start += 1;
                    continue;
                };
                out.push_str(text.get(copied..start).unwrap_or_default());
                if ip.is_loopback() || ip.is_unspecified() {
                    out.push_str(text.get(start..end).unwrap_or_default());
                } else {
                    out.push_str(IPV6_STAND_IN);
                }
                copied = end;
                start = end;
            }
        }
        out.push_str(text.get(copied..).unwrap_or_default());
        out
    }

    // ── Entity-level sanitizers ────────────────────────────────────

    pub fn sanitize_device(&self, device: &Device) -> Device {
        let mut d = device.clone();
        d.name = self.sanitize_name_opt(&d.name);
        d.ip = self.sanitize_ip_opt(&d.ip);
        if self.redact_wan_ips {
            d.wan_ipv6 = d.wan_ipv6.as_ref().map(|_| IPV6_STAND_IN.to_owned());
        }
        if self.redact_macs {
            d.mac = self.sanitize_mac(&d.mac);
            d.uplink_device_mac = self.sanitize_mac_opt(&d.uplink_device_mac);
        }
        d
    }

    pub fn sanitize_client(&self, client: &Client) -> Client {
        let mut c = client.clone();
        c.name = self.sanitize_name_opt(&c.name);
        c.hostname = self.sanitize_name_opt(&c.hostname);
        // Networks are renamed by `sanitize_network`; the same mapping keeps
        // a client's network name matching its renamed network.
        c.network_name = self.sanitize_name_opt(&c.network_name);
        c.ip = self.sanitize_ip_opt(&c.ip);
        if let Some(ref mut wi) = c.wireless {
            wi.ssid = wi.ssid.as_ref().map(|s| self.sanitize_ssid(s));
        }
        if self.redact_macs {
            c.mac = self.sanitize_mac(&c.mac);
            c.uplink_device_mac = self.sanitize_mac_opt(&c.uplink_device_mac);
        }
        c
    }

    pub fn sanitize_network(&self, network: &Network) -> Network {
        let mut n = network.clone();
        n.name = self.sanitize_name(&n.name);
        n
    }

    pub fn sanitize_event(&self, event: &Event) -> Event {
        let mut e = event.clone();
        e.message = self.sanitize_text(&e.message);
        if self.redact_macs {
            e.device_mac = self.sanitize_mac_opt(&e.device_mac);
            e.client_mac = self.sanitize_mac_opt(&e.client_mac);
        }
        e
    }

    pub fn sanitize_wifi(&self, wifi: &WifiBroadcast) -> WifiBroadcast {
        let mut w = wifi.clone();
        w.name = self.sanitize_ssid(&w.name);
        w
    }

    pub fn sanitize_health(&self, health: &HealthSummary) -> HealthSummary {
        let mut h = health.clone();
        h.wan_ip = h.wan_ip.as_ref().map(|ip| self.sanitize_wan_ip_str(ip));
        h.gateways = h
            .gateways
            .as_ref()
            .map(|gws| gws.iter().map(|g| self.sanitize_wan_ip_str(g)).collect());
        if self.redact_isp {
            h.extra = self.sanitize_health_extra(&h.extra);
        }
        h
    }

    pub fn sanitize_firewall_policy(&self, policy: &FirewallPolicy) -> FirewallPolicy {
        let mut p = policy.clone();
        p.name = self.sanitize_name(&p.name);
        p.description = p.description.as_ref().map(|d| self.sanitize_text(d));
        p.source_summary = p.source_summary.as_ref().map(|s| self.sanitize_text(s));
        p.destination_summary = p
            .destination_summary
            .as_ref()
            .map(|s| self.sanitize_text(s));
        p
    }

    pub fn sanitize_firewall_zone(&self, zone: &FirewallZone) -> FirewallZone {
        let mut z = zone.clone();
        z.name = self.sanitize_name(&z.name);
        z
    }

    pub fn sanitize_acl_rule(&self, rule: &AclRule) -> AclRule {
        let mut r = rule.clone();
        r.name = self.sanitize_name(&r.name);
        r.source_summary = r.source_summary.as_ref().map(|s| self.sanitize_text(s));
        r.destination_summary = r
            .destination_summary
            .as_ref()
            .map(|s| self.sanitize_text(s));
        r
    }

    pub fn sanitize_nat_policy(&self, policy: &NatPolicy) -> NatPolicy {
        let mut p = policy.clone();
        p.name = self.sanitize_name(&p.name);
        p.description = p.description.as_ref().map(|d| self.sanitize_text(d));
        p.dst_address = p.dst_address.as_ref().map(|a| self.sanitize_text(a));
        p.src_address = p.src_address.as_ref().map(|a| self.sanitize_text(a));
        p.translated_address = p.translated_address.as_ref().map(|a| self.sanitize_text(a));
        p
    }

    pub fn sanitize_rogue_ap(&self, ap: &RogueAp) -> RogueAp {
        let mut r = ap.clone();
        if self.redact_ssids {
            r.essid = r.essid.as_ref().map(|s| self.sanitize_ssid(s));
        }
        if self.redact_macs {
            let hash = simple_hash(&r.bssid);
            let bytes = hash.to_le_bytes();
            r.bssid = format!(
                "02:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4]
            );
            r.ap_mac = r.ap_mac.as_ref().map(|m| {
                let h = simple_hash(m);
                let b = h.to_le_bytes();
                format!(
                    "02:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    b[0], b[1], b[2], b[3], b[4]
                )
            });
        }
        r
    }

    pub fn sanitize_rogue_aps(&self, v: &[RogueAp]) -> Vec<RogueAp> {
        v.iter().map(|ap| self.sanitize_rogue_ap(ap)).collect()
    }

    pub fn sanitize_json_value(&self, value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::String(s) => serde_json::Value::String(self.sanitize_text(s)),
            serde_json::Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(|v| self.sanitize_json_value(v)).collect())
            }
            serde_json::Value::Object(map) => {
                let mut out = serde_json::Map::new();
                for (k, v) in map {
                    out.insert(k.clone(), self.sanitize_json_value(v));
                }
                serde_json::Value::Object(out)
            }
            other => other.clone(),
        }
    }

    // ── Collection wrappers (for data bridge) ──────────────────────

    pub fn sanitize_devices(&self, v: &[Arc<Device>]) -> Arc<Vec<Arc<Device>>> {
        Arc::new(
            v.iter()
                .map(|d| Arc::new(self.sanitize_device(d)))
                .collect(),
        )
    }

    pub fn sanitize_clients(&self, v: &[Arc<Client>]) -> Arc<Vec<Arc<Client>>> {
        Arc::new(
            v.iter()
                .map(|c| Arc::new(self.sanitize_client(c)))
                .collect(),
        )
    }

    pub fn sanitize_networks(&self, v: &[Arc<Network>]) -> Arc<Vec<Arc<Network>>> {
        Arc::new(
            v.iter()
                .map(|n| Arc::new(self.sanitize_network(n)))
                .collect(),
        )
    }

    pub fn sanitize_events_vec(&self, v: &[Arc<Event>]) -> Vec<Arc<Event>> {
        v.iter().map(|e| Arc::new(self.sanitize_event(e))).collect()
    }

    pub fn sanitize_wifi_broadcasts(
        &self,
        v: &[Arc<WifiBroadcast>],
    ) -> Arc<Vec<Arc<WifiBroadcast>>> {
        Arc::new(v.iter().map(|w| Arc::new(self.sanitize_wifi(w))).collect())
    }

    pub fn sanitize_firewall_policies(
        &self,
        v: &[Arc<FirewallPolicy>],
    ) -> Arc<Vec<Arc<FirewallPolicy>>> {
        Arc::new(
            v.iter()
                .map(|p| Arc::new(self.sanitize_firewall_policy(p)))
                .collect(),
        )
    }

    pub fn sanitize_firewall_zones(&self, v: &[Arc<FirewallZone>]) -> Arc<Vec<Arc<FirewallZone>>> {
        Arc::new(
            v.iter()
                .map(|z| Arc::new(self.sanitize_firewall_zone(z)))
                .collect(),
        )
    }

    pub fn sanitize_acl_rules(&self, v: &[Arc<AclRule>]) -> Arc<Vec<Arc<AclRule>>> {
        Arc::new(
            v.iter()
                .map(|r| Arc::new(self.sanitize_acl_rule(r)))
                .collect(),
        )
    }

    pub fn sanitize_nat_policies(&self, v: &[Arc<NatPolicy>]) -> Arc<Vec<Arc<NatPolicy>>> {
        Arc::new(
            v.iter()
                .map(|p| Arc::new(self.sanitize_nat_policy(p)))
                .collect(),
        )
    }

    pub fn sanitize_health_vec(&self, v: &[HealthSummary]) -> Arc<Vec<HealthSummary>> {
        Arc::new(v.iter().map(|h| self.sanitize_health(h)).collect())
    }

    // ── JSON sanitization for HealthSummary.extra ──────────────────

    fn sanitize_health_extra(&self, value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::String(s) => serde_json::Value::String(self.sanitize_text(s)),
            serde_json::Value::Array(arr) => serde_json::Value::Array(
                arr.iter().map(|v| self.sanitize_health_extra(v)).collect(),
            ),
            serde_json::Value::Object(map) => {
                let mut out = serde_json::Map::new();
                for (k, v) in map {
                    let new_v = match k.as_str() {
                        "isp_name" | "isp_organization" => {
                            serde_json::Value::String("Demo ISP".to_owned())
                        }
                        "nameservers" => self.sanitize_nameservers(v),
                        _ => self.sanitize_health_extra(v),
                    };
                    out.insert(k.clone(), new_v);
                }
                serde_json::Value::Object(out)
            }
            other => other.clone(),
        }
    }

    fn sanitize_nameservers(&self, value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Array(arr) => serde_json::Value::Array(
                arr.iter()
                    .map(|v| match v {
                        serde_json::Value::String(s) => {
                            serde_json::Value::String(self.sanitize_wan_ip_str(s))
                        }
                        other => other.clone(),
                    })
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !v4.is_private()
                && !v4.is_loopback()
                && !v4.is_link_local()
                && !v4.is_broadcast()
                && !v4.is_unspecified()
                && !is_cgnat(v4)
                && !is_documentation(v4)
        }
        IpAddr::V6(_) => false,
    }
}

fn is_cgnat(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

fn is_documentation(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    matches!(
        (octets[0], octets[1], octets[2]),
        (192, 0, 2) | (198, 51, 100) | (203, 0, 113)
    )
}

/// The longest written IPv6 address is 45 characters, one with an IPv4
/// tail.
const MAX_IPV6_LEN: usize = 45;

/// The longest IPv6 address in `text` from `start` up to `limit` that no
/// letter or digit touches on either side, and where it ends. `start` and
/// `limit` fall inside a run of ASCII hex digits, colons, and dots.
fn ipv6_at(text: &str, start: usize, limit: usize) -> Option<(usize, Ipv6Addr)> {
    let glued = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    if glued(text.get(..start)?.chars().next_back()) {
        return None;
    }
    let longest = limit.min(start + MAX_IPV6_LEN);
    (start + 2..=longest).rev().find_map(|end| {
        if glued(text.get(end..)?.chars().next()) {
            return None;
        }
        let ip = text.get(start..end)?.parse::<Ipv6Addr>().ok()?;
        Some((end, ip))
    })
}

fn simple_hash(input: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in input.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DemoConfig;

    fn test_config() -> DemoConfig {
        DemoConfig {
            enabled: true,
            redact_names: vec!["Alice".into(), "Bob".into()],
            keep_names: vec!["Bliss".into()],
            redact_ssids: true,
            redact_wan_ips: true,
            redact_macs: false,
            redact_isp: false,
            seed: None,
        }
    }

    #[test]
    fn redacted_name_is_replaced() {
        let s = Sanitizer::new(&test_config());
        let result = s.sanitize_name("Alice's iPhone");
        assert_ne!(result, "Alice's iPhone");
        assert!(CUTE_NAMES.iter().any(|&n| result.starts_with(n)));
    }

    #[test]
    fn kept_name_is_preserved() {
        let s = Sanitizer::new(&test_config());
        assert_eq!(s.sanitize_name("Bliss"), "Bliss");
    }

    #[test]
    fn unmatched_name_is_preserved() {
        let s = Sanitizer::new(&test_config());
        assert_eq!(s.sanitize_name("Living Room AP"), "Living Room AP");
    }

    #[test]
    fn deterministic_name_mapping() {
        let s = Sanitizer::new(&test_config());
        let first = s.sanitize_name("Alice");
        let second = s.sanitize_name("Alice");
        assert_eq!(first, second);
    }

    #[test]
    fn public_ip_is_replaced() {
        let s = Sanitizer::new(&test_config());
        let public = "8.8.8.8".parse().expect("valid IP");
        let result = s.sanitize_wan_ip(public);
        assert_ne!(result, public);
        match result {
            IpAddr::V4(v4) => {
                assert_eq!(v4.octets()[0], 198);
                assert_eq!(v4.octets()[1], 51);
                assert_eq!(v4.octets()[2], 100);
            }
            IpAddr::V6(_) => panic!("expected IPv4"),
        }
    }

    #[test]
    fn private_ip_is_kept() {
        let s = Sanitizer::new(&test_config());
        let private: IpAddr = "192.168.1.1".parse().expect("valid IP");
        assert_eq!(s.sanitize_wan_ip(private), private);
    }

    #[test]
    fn ssid_is_replaced_when_enabled() {
        let s = Sanitizer::new(&test_config());
        let result = s.sanitize_ssid("MyHomeWifi");
        assert_ne!(result, "MyHomeWifi");
        assert!(CUTE_SSIDS.iter().any(|&n| result.starts_with(n)));
    }

    #[test]
    fn text_sanitization_replaces_names_and_ips() {
        let s = Sanitizer::new(&test_config());
        let text = "Alice connected from 73.45.67.89 to the network";
        let result = s.sanitize_text(text);
        assert!(!result.contains("Alice"));
        assert!(!result.contains("73.45.67.89"));
        assert!(result.contains("198.51.100."));
    }

    #[test]
    fn text_preserves_private_ips() {
        let s = Sanitizer::new(&test_config());
        let text = "Client at 192.168.1.50";
        let result = s.sanitize_text(text);
        assert!(result.contains("192.168.1.50"));
    }

    #[test]
    fn default_text_keeps_macs_times_and_paths() {
        let s = Sanitizer::new(&DemoConfig::default());
        for text in [
            "Device not found: aa:bb:cc:dd:ee:ff at 12:34:56 via unifly::tui",
            "AA-BB-CC-DD-EE-FF roamed at 09:41",
            "error in unifly_api::controller::refresh: timed out",
            "ratio 10:20:30, std::io::Error, a :: b",
        ] {
            assert_eq!(s.sanitize_text(text), text);
        }
    }

    #[test]
    fn default_text_redacts_real_ipv6_addresses() {
        let s = Sanitizer::new(&DemoConfig::default());
        assert_eq!(
            s.sanitize_text("WAN up on 2001:470:1f0b:5c::2, gateway fe80::1."),
            "WAN up on 2001:db8::cafe, gateway 2001:db8::cafe."
        );
        assert_eq!(
            s.sanitize_text("listening on [2600:1700:abcd::7]:443"),
            "listening on [2001:db8::cafe]:443"
        );
        // Loopback and the unspecified address identify nothing.
        assert_eq!(
            s.sanitize_text("bound to :: and ::1"),
            "bound to :: and ::1"
        );
    }

    #[test]
    fn default_text_redacts_ipv6_glued_to_its_neighbours() {
        let s = Sanitizer::new(&DemoConfig::default());
        for (text, expected) in [
            ("addr:2001:470::1", "addr:2001:db8::cafe"),
            ("IPv6:2001:470:1f0b::2/64", "IPv6:2001:db8::cafe/64"),
            ("2001:470::1_x", "2001:db8::cafe_x"),
            ("WAN 2001:470::1: down", "WAN 2001:db8::cafe: down"),
            ("fe80::1%eth0 is up", "2001:db8::cafe%eth0 is up"),
            ("2001:db8::cafe stays", "2001:db8::cafe stays"),
            (
                "EUI-64 0011:2233:4455:6677:8899:aabb:ccdd:eeff",
                "EUI-64 2001:db8::cafe",
            ),
        ] {
            assert_eq!(s.sanitize_text(text), expected, "{text}");
        }
        for text in [
            "Vec::new()",
            "Self::DEFAULT",
            "std::io::Error",
            "unifly::tui::app",
        ] {
            assert_eq!(s.sanitize_text(text), text);
        }
    }

    #[test]
    fn stand_ins_are_not_mapped_again() {
        let s = Sanitizer::new(&DemoConfig {
            redact_names: vec!["alice".into(), "pixel".into()],
            redact_ssids: true,
            redact_macs: true,
            ..DemoConfig::default()
        });

        let mac = s.sanitize_mac(&MacAddress::new("aa:bb:cc:dd:ee:ff"));
        assert_eq!(s.sanitize_mac(&mac), mac);

        let ssid = s.sanitize_ssid("AliceNet");
        assert_eq!(s.sanitize_ssid(&ssid), ssid);

        // Enough names that a stand-in is itself a redacted word.
        let names: Vec<String> = (0..30)
            .map(|n| s.sanitize_name(&format!("alice-{n}")))
            .collect();
        for name in &names {
            assert_eq!(s.sanitize_name(name), *name, "{names:?}");
            assert_eq!(
                s.sanitize_text(&format!("Restart {name}?")),
                format!("Restart {name}?")
            );
        }
    }

    #[test]
    fn is_public_ip_classification() {
        assert!(is_public_ip("8.8.8.8".parse().expect("valid")));
        assert!(is_public_ip("1.1.1.1".parse().expect("valid")));
        assert!(!is_public_ip("192.168.1.1".parse().expect("valid")));
        assert!(!is_public_ip("10.0.0.1".parse().expect("valid")));
        assert!(!is_public_ip("172.16.0.1".parse().expect("valid")));
        assert!(!is_public_ip("127.0.0.1".parse().expect("valid")));
        assert!(!is_public_ip("100.64.0.1".parse().expect("valid")));
    }
}
