use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU16, AtomicUsize, Ordering};

use dashmap::DashMap;
use regex::Regex;

use unifly_api::model::{
    AclRule, Alarm, FirewallPolicy, FirewallZone, IpSpec, Ipv6Scope, NatPolicy, TrafficFilter,
    WifiBroadcast,
};
use unifly_api::session::models::RogueAp;
use unifly_api::{Client, Device, Event, HealthSummary, MacAddress, Network};

use crate::config::DemoConfig;

const CUTE_NAMES: &[&str] = &[
    "Starling", "Moonbeam", "Pixel", "Cosmo", "Mochi", "Nimbus", "Wren", "Ziggy", "Sprocket",
    "Quasar", "Clover", "Ember", "Tinker", "Velvet", "Cricket", "Biscuit", "Phantom", "Stardust",
    "Marble", "Glimmer", "Waffle", "Pebble", "Fizz", "Comet",
];

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
    mac_regex: Regex,
    quoted_ssid: Regex,
    redact_ssids: bool,
    redact_wan_ips: bool,
    redact_macs: bool,
    redact_isp: bool,
    name_map: DashMap<String, String>,
    ssid_map: DashMap<String, String>,
    ip_map: DashMap<IpAddr, IpAddr>,
    ipv6_map: DashMap<Ipv6Addr, Ipv6Addr>,
    ipv6_stand_ins: DashMap<Ipv6Addr, ()>,
    mac_map: DashMap<String, String>,
    /// Every stand-in handed out, so text or values that already carry
    /// one are never mapped a second time.
    name_stand_ins: DashMap<String, ()>,
    ssid_stand_ins: DashMap<String, ()>,
    mac_stand_ins: DashMap<String, ()>,
    name_counter: AtomicUsize,
    ssid_counter: AtomicUsize,
    wan_ip_counter: AtomicU8,
    ipv6_counter: AtomicU16,
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
        // A MAC in colon or dash pairs, dotted quads (aabb.ccdd.eeff), or
        // twelve bare hex digits; word edges keep longer hex such as hashes
        // from matching.
        let mac_regex = Regex::new(
            r"(?i)\b(?:[0-9a-f]{2}(?:[:-][0-9a-f]{2}){5}|[0-9a-f]{4}\.[0-9a-f]{4}\.[0-9a-f]{4}|[0-9a-f]{12})\b",
        )
        .expect("valid regex");
        // An SSID quoted after "SSID", "connected to", or "disconnected
        // from", the shapes event messages name one in.
        let quoted_ssid =
            Regex::new(r#"(?i)\b(?:ssid|connected to|disconnected from)\s+"([^"\r\n]{1,32})""#)
                .expect("valid regex");

        Self {
            redact_patterns,
            keep_patterns,
            name_regex,
            ip_regex,
            ipv6_run,
            mac_regex,
            quoted_ssid,
            redact_ssids: config.redact_ssids,
            redact_wan_ips: config.redact_wan_ips,
            redact_macs: config.redact_macs,
            redact_isp: config.redact_isp,
            name_map: DashMap::new(),
            ssid_map: DashMap::new(),
            ip_map: DashMap::new(),
            ipv6_map: DashMap::new(),
            ipv6_stand_ins: DashMap::new(),
            mac_map: DashMap::new(),
            name_stand_ins: DashMap::new(),
            ssid_stand_ins: DashMap::new(),
            mac_stand_ins: DashMap::new(),
            name_counter: AtomicUsize::new(1),
            ssid_counter: AtomicUsize::new(1),
            wan_ip_counter: AtomicU8::new(1),
            ipv6_counter: AtomicU16::new(1),
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

        *self.ip_map.entry(ip).or_insert_with(|| {
            let idx = self.wan_ip_counter.fetch_add(1, Ordering::Relaxed);
            IpAddr::V4(Ipv4Addr::new(198, 51, 100, idx.wrapping_add(1)))
        })
    }

    pub fn sanitize_ip_opt(&self, ip: &Option<IpAddr>) -> Option<IpAddr> {
        ip.map(|addr| match addr {
            IpAddr::V6(v6) if self.redacts_ipv6() => IpAddr::V6(self.sanitize_ipv6(v6)),
            addr => self.sanitize_wan_ip(addr),
        })
    }

    /// The stand-in for an IPv6 address: one per address, so the same
    /// address reads the same everywhere and two hosts never share one. A
    /// stand-in keeps its address's reach (global ones come from the
    /// documentation range), so a redacted link-local address still reads
    /// as link-local. Loopback, the unspecified address, documentation
    /// addresses, and stand-ins identify nothing and stay.
    pub fn sanitize_ipv6(&self, ip: Ipv6Addr) -> Ipv6Addr {
        let [first, second, ..] = ip.segments();
        if ip.is_loopback()
            || ip.is_unspecified()
            || (first, second) == (0x2001, 0xdb8)
            || self.ipv6_stand_ins.contains_key(&ip)
        {
            return ip;
        }
        *self.ipv6_map.entry(ip).or_insert_with(|| {
            let index = self.ipv6_counter.fetch_add(1, Ordering::Relaxed);
            let stand_in = match Ipv6Scope::of(&ip) {
                Ipv6Scope::Global => Ipv6Addr::new(0x2001, 0xdb8, 0xcafe, 0, 0, 0, 0, index),
                Ipv6Scope::UniqueLocal => Ipv6Addr::new(0xfd00, 0xdb8, 0xcafe, 0, 0, 0, 0, index),
                Ipv6Scope::LinkLocal => Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0xcafe, index),
            };
            self.ipv6_stand_ins.insert(stand_in, ());
            stand_in
        })
    }

    /// Whether IPv6 addresses are redacted. A SLAAC address built from the
    /// host's MAC spells that MAC out, so redacting MACs redacts them too.
    fn redacts_ipv6(&self) -> bool {
        self.redact_wan_ips || self.redact_macs
    }

    /// `subnet`, an IPv6 address with an optional prefix length, with its
    /// address replaced.
    fn sanitize_ipv6_subnet(&self, subnet: &str) -> String {
        let (address, length) = subnet
            .split_once('/')
            .map_or((subnet, None), |(address, length)| (address, Some(length)));
        let Ok(address) = address.parse::<Ipv6Addr>() else {
            return self.sanitize_text(subnet);
        };
        let address = self.sanitize_ipv6(address);
        length.map_or_else(
            || address.to_string(),
            |length| format!("{address}/{length}"),
        )
    }

    pub fn sanitize_ssid(&self, ssid: &str) -> String {
        if !self.redact_ssids || ssid.trim().is_empty() || self.ssid_stand_ins.contains_key(ssid) {
            return ssid.to_owned();
        }

        self.ssid_map
            .entry(ssid.to_owned())
            .or_insert_with(|| {
                let idx = self.ssid_counter.fetch_add(1, Ordering::Relaxed);
                let cute = CUTE_SSIDS[idx % CUTE_SSIDS.len()];
                let replacement = if idx < CUTE_SSIDS.len() {
                    cute.to_owned()
                } else {
                    format!("{cute} {}", idx / CUTE_SSIDS.len() + 1)
                };
                self.ssid_stand_ins.insert(replacement.clone(), ());
                replacement
            })
            .clone()
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

    /// Sanitize free text, such as an event message or an error. SSIDs
    /// the sanitizer has mapped become their stand-ins first, then names,
    /// WAN addresses, and MACs are redacted as their settings say.
    pub fn sanitize_text(&self, text: &str) -> String {
        if self.redact_ssids {
            for quoted in self.quoted_ssid.captures_iter(text) {
                self.sanitize_ssid(&quoted[1]);
            }
        }
        let mut result = self.redact_known_ssids(text);

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

        // IPv6 first: an IPv4-mapped address must reach its stand-in whole,
        // not with its IPv4 tail already replaced.
        if self.redacts_ipv6() {
            result = self.redact_ipv6(&result);
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
        }

        if self.redact_macs {
            let text = result;
            result = self
                .mac_regex
                .replace_all(&text, |found: &regex::Captures| {
                    let mac = &found[0];
                    let span = found.get(0).map_or(0..0, |whole| whole.range());
                    if chained(&text, span) {
                        mac.to_owned()
                    } else {
                        self.sanitize_mac(&MacAddress::new(mac)).to_string()
                    }
                })
                .into_owned();
        }

        result
    }

    /// Replace every SSID already mapped, wherever it stands as a whole
    /// word in `text`, with its stand-in. Longer SSIDs win, and a stand-in
    /// once written is not scanned again.
    fn redact_known_ssids(&self, text: &str) -> String {
        if self.ssid_map.is_empty() {
            return text.to_owned();
        }
        let mut ssids: Vec<(String, String)> = self
            .ssid_map
            .iter()
            .filter(|entry| !entry.key().is_empty())
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect();
        ssids.sort_by_key(|(real, _)| std::cmp::Reverse(real.len()));

        let word_edge = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        let mut before = None;
        'scan: while let Some(next) = rest.chars().next() {
            if word_edge(before) {
                for (real, stand_in) in &ssids {
                    if let Some(after) = rest.strip_prefix(real.as_str())
                        && word_edge(after.chars().next())
                    {
                        out.push_str(stand_in);
                        before = real.chars().last();
                        rest = after;
                        continue 'scan;
                    }
                }
            }
            out.push(next);
            before = Some(next);
            rest = rest.get(next.len_utf8()..).unwrap_or_default();
        }
        out
    }

    pub fn sanitize_wan_ip_str(&self, ip_str: &str) -> String {
        if let Ok(ip) = ip_str.trim().parse::<Ipv6Addr>() {
            return if self.redacts_ipv6() {
                self.sanitize_ipv6(ip).to_string()
            } else {
                ip_str.to_owned()
            };
        }
        if self.redact_wan_ips
            && let Ok(ip) = ip_str.parse::<IpAddr>()
            && is_public_ip(ip)
        {
            return self.sanitize_wan_ip(ip).to_string();
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

    /// Replace each IPv6 address in `text` with its documentation stand-in.
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
                    out.push_str(&self.sanitize_ipv6(ip).to_string());
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
        // A port is labeled after what plugs into it, so its name gets the
        // stand-in that device or client shows under everywhere else.
        for port in &mut d.ports {
            port.name = self.sanitize_name_opt(&port.name);
        }
        d.ip = self.sanitize_ip_opt(&d.ip);
        if self.redacts_ipv6() {
            d.wan_ipv6 = d
                .wan_ipv6
                .as_deref()
                .map(|ip| self.sanitize_ipv6_subnet(ip));
            for ip in &mut d.ipv6_addresses {
                *ip = self.sanitize_ipv6(*ip);
            }
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
        if self.redacts_ipv6() {
            for ip in &mut c.ipv6_addresses {
                *ip = self.sanitize_ipv6(*ip);
            }
        }
        if let Some(ref mut wi) = c.wireless {
            wi.ssid = wi.ssid.as_ref().map(|s| self.sanitize_ssid(s));
        }
        if self.redact_macs {
            c.mac = self.sanitize_mac(&c.mac);
            c.uplink_device_mac = self.sanitize_mac_opt(&c.uplink_device_mac);
            if let Some(ref mut wi) = c.wireless {
                wi.bssid = self.sanitize_mac_opt(&wi.bssid);
            }
        }
        c
    }

    pub fn sanitize_network(&self, network: &Network) -> Network {
        let mut n = network.clone();
        n.name = self.sanitize_name(&n.name);
        if self.redacts_ipv6() {
            n.ipv6_prefix = n
                .ipv6_prefix
                .as_deref()
                .map(|subnet| self.sanitize_ipv6_subnet(subnet));
            for subnet in &mut n.ipv6_additional_subnets {
                *subnet = self.sanitize_ipv6_subnet(subnet);
            }
            for server in &mut n.ipv6_dns_servers {
                if let IpAddr::V6(v6) = server {
                    *v6 = self.sanitize_ipv6(*v6);
                }
            }
        }
        n
    }

    pub fn sanitize_event(&self, event: &Event) -> Event {
        let mut e = event.clone();
        e.ssid = e.ssid.as_deref().map(|ssid| self.sanitize_ssid(ssid));
        e.message = self.sanitize_text(&e.message);
        if self.redact_macs {
            e.device_mac = self.sanitize_mac_opt(&e.device_mac);
            e.client_mac = self.sanitize_mac_opt(&e.client_mac);
        }
        e
    }

    pub fn sanitize_alarm(&self, alarm: &Alarm) -> Alarm {
        let mut a = alarm.clone();
        a.message = self.sanitize_text(&a.message);
        if self.redact_macs {
            a.device_mac = self.sanitize_mac_opt(&a.device_mac);
        }
        a
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
        // `extra` is the whole raw health payload, and the dashboard draws
        // addresses straight from it: the ISP gateway, the WAN IPv6, and
        // the DNS servers. They follow the WAN address setting like every
        // other address; ISP names go only with redact_isp.
        h.extra = self.sanitize_json_value(&h.extra);
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
        p.source.filter = p.source.filter.as_ref().map(|f| self.sanitize_filter(f));
        p.destination.filter = p
            .destination
            .filter
            .as_ref()
            .map(|f| self.sanitize_filter(f));
        p
    }

    /// A traffic filter with the addresses, MACs, and domains it names
    /// sanitized like any other drawn text: the Firewall screen's detail
    /// panel spells a filter out in full.
    fn sanitize_filter(&self, filter: &TrafficFilter) -> TrafficFilter {
        let clean = |items: &mut Vec<String>| {
            for item in items {
                *item = self.sanitize_text(item);
            }
        };
        let mut filter = filter.clone();
        match &mut filter {
            TrafficFilter::Network { mac_addresses, .. }
            | TrafficFilter::MacAddress { mac_addresses, .. } => clean(mac_addresses),
            TrafficFilter::IpAddress {
                addresses,
                mac_addresses,
                ..
            } => {
                clean(mac_addresses);
                for address in addresses {
                    match address {
                        IpSpec::Address { value } | IpSpec::Subnet { value } => {
                            *value = self.sanitize_text(value);
                        }
                        IpSpec::Range { start, stop } => {
                            *start = self.sanitize_text(start);
                            *stop = self.sanitize_text(stop);
                        }
                        IpSpec::MatchingList { .. } => {}
                    }
                }
            }
            TrafficFilter::Domain { domains, .. } => clean(domains),
            _ => {}
        }
        filter
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
        // The same MAC mapping as every list, so a neighbor's BSSID that
        // is also one of ours shows the same stand-in in both.
        if self.redact_macs {
            r.bssid = self.sanitize_mac(&MacAddress::new(&r.bssid)).to_string();
            r.ap_mac = r
                .ap_mac
                .as_ref()
                .map(|mac| self.sanitize_mac(&MacAddress::new(mac)).to_string());
        }
        r
    }

    pub fn sanitize_rogue_aps(&self, v: &[RogueAp]) -> Vec<RogueAp> {
        v.iter().map(|ap| self.sanitize_rogue_ap(ap)).collect()
    }

    /// Sanitize a JSON payload such as a WiFi detail or a roam history
    /// row. The SSIDs its fields name are mapped first, so its text fields
    /// know every SSID the item carries, whatever order they come in.
    pub fn sanitize_json_value(&self, value: &serde_json::Value) -> serde_json::Value {
        self.map_json_ssids(value, None);
        self.clean_json(value, None)
    }

    fn map_json_ssids(&self, value: &serde_json::Value, parent: Option<&str>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, field) in map {
                    if let Some(ssid) = json_ssid(key, parent, field) {
                        self.sanitize_ssid(ssid);
                    }
                    self.map_json_ssids(field, Some(key));
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    self.map_json_ssids(item, parent);
                }
            }
            _ => {}
        }
    }

    fn clean_json(&self, value: &serde_json::Value, parent: Option<&str>) -> serde_json::Value {
        match value {
            serde_json::Value::String(s) => serde_json::Value::String(self.sanitize_text(s)),
            serde_json::Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(|v| self.clean_json(v, parent)).collect())
            }
            serde_json::Value::Object(map) => {
                let mut out = serde_json::Map::new();
                for (k, v) in map {
                    // Named fields map the way the lists map them; the rest
                    // is free text.
                    let clean = if let Some(ssid) = json_ssid(k, parent, v) {
                        serde_json::Value::String(self.sanitize_ssid(ssid))
                    } else if let (
                        "name" | "hostname" | "display_name" | "device_name",
                        serde_json::Value::String(name),
                    ) = (k.as_str(), v)
                    {
                        // A name can be an address: an access point nobody
                        // named reports its MAC. The text pass catches
                        // what the name mapping lets through.
                        serde_json::Value::String(self.sanitize_text(&self.sanitize_name(name)))
                    } else {
                        self.clean_json(v, Some(k))
                    };
                    out.insert(k.clone(), clean);
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

/// Whether the MAC-shaped text at `span` is only part of a longer chain of
/// hex groups, such as `aabb.ccdd.eeff.0011`, and so not a MAC at all.
fn chained(text: &str, span: Range<usize>) -> bool {
    let joins = |separator: Option<char>, beyond: Option<char>| {
        matches!(separator, Some('.' | ':' | '-')) && beyond.is_some_and(|c| c.is_ascii_hexdigit())
    };
    let mut before = text.get(..span.start).unwrap_or_default().chars().rev();
    let mut after = text.get(span.end..).unwrap_or_default().chars();
    joins(before.next(), before.next()) || joins(after.next(), after.next())
}

/// The SSID a JSON field holds: an `ssid` or `essid` field, or the name of
/// a `WLAN` object, the shape roam history rows carry it in.
fn json_ssid<'a>(key: &str, parent: Option<&str>, value: &'a serde_json::Value) -> Option<&'a str> {
    let wlan_name =
        key == "name" && parent.is_some_and(|parent| parent.eq_ignore_ascii_case("wlan"));
    let holds_ssid =
        wlan_name || key.eq_ignore_ascii_case("ssid") || key.eq_ignore_ascii_case("essid");
    holds_ssid.then(|| value.as_str()).flatten()
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
    fn a_policy_filter_shows_no_real_wan_address_or_mac() {
        let policy: FirewallPolicy = serde_json::from_value(serde_json::json!({
            "id": "p1", "name": "Vendor session", "description": null, "enabled": true,
            "index": 1, "action": "Allow", "ip_version": "Both",
            "source": { "zone_id": null, "filter": {
                "kind": "ip_address", "match_opposite": false,
                "addresses": [
                    { "kind": "address", "value": "8.8.4.4" },
                    { "kind": "range", "start": "8.8.8.1", "stop": "8.8.8.9" }
                ],
                "mac_addresses": ["aa:bb:cc:dd:ee:ff"]
            }},
            "destination": { "zone_id": null, "filter": null },
            "source_summary": null, "destination_summary": null,
            "protocol_summary": null, "schedule": null, "ipsec_mode": null,
            "connection_states": [], "logging_enabled": false, "origin": null
        }))
        .expect("policy fixture");
        let sanitizer = Sanitizer::new(&DemoConfig {
            redact_macs: true,
            ..test_config()
        });

        let shown = sanitizer.sanitize_firewall_policy(&policy);

        let filter = serde_json::to_string(&shown.source.filter).expect("filter JSON");
        for real in ["8.8.4.4", "8.8.8.1", "8.8.8.9", "aa:bb:cc:dd:ee:ff"] {
            assert!(!filter.contains(real), "{real} survived: {filter}");
        }
    }

    #[test]
    fn a_devices_port_labels_are_redacted_like_names() {
        use crate::cli::commands::util::fixtures::device;

        let mut switch = (*device("d1", Some("sw-core"), "aa:bb:cc:00:00:01", None)).clone();
        switch.ports = serde_json::from_value(serde_json::json!([
            { "index": 1, "name": "Alice's desk", "state": "Up", "speed_mbps": 1000,
              "max_speed_mbps": 1000, "connector": "Rj45", "poe": null },
            { "index": 2, "name": "Port 2", "state": "Down", "speed_mbps": null,
              "max_speed_mbps": 1000, "connector": "Rj45", "poe": null },
            { "index": 3, "name": null, "state": "Down", "speed_mbps": null,
              "max_speed_mbps": 1000, "connector": "Rj45", "poe": null }
        ]))
        .expect("port fixtures");
        let sanitizer = Sanitizer::new(&test_config());

        let shown = sanitizer.sanitize_device(&switch);

        let label = shown.ports[0].name.as_deref().expect("a label");
        assert!(!label.contains("Alice"), "{label}");
        assert_eq!(
            label,
            sanitizer.sanitize_name("Alice's desk"),
            "the same stand-in the port's profile shows once it loads"
        );
        assert_eq!(shown.ports[1].name.as_deref(), Some("Port 2"));
        assert_eq!(shown.ports[2].name, None);
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
    fn historical_session_event_seeds_unquoted_ssid_before_redaction() {
        let raw: unifly_api::session_models::SessionEvent =
            serde_json::from_value(serde_json::json!({
                "_id": "event-1", "key": "EVT_WU_Connected", "datetime": "2026-11-06T01:00:00Z",
                "msg": "Client connected to {essid}", "essid": "Retired Secret Network"
            }))
            .expect("session event");
        let event = Event::from(raw);
        assert_eq!(event.ssid.as_deref(), Some("Retired Secret Network"));
        let sanitizer = Sanitizer::new(&test_config());
        let clean = sanitizer.sanitize_event(&event);
        let stand_in = sanitizer.sanitize_ssid("Retired Secret Network");
        assert_eq!(clean.ssid.as_deref(), Some(stand_in.as_str()));
        assert_eq!(clean.message, format!("Client connected to {stand_in}"));
        assert!(
            !serde_json::to_string(&clean)
                .expect("JSON")
                .contains("Retired Secret Network")
        );
    }

    #[test]
    fn modern_and_websocket_events_preserve_ssid_redaction_context() {
        let modern: unifly_api::session::system_log::SystemLogEntry =
            serde_json::from_value(serde_json::json!({
                "timestamp": 1_794_001_000_000_i64,
                "message_raw": "Client connected to {WLAN}",
                "parameters": { "WLAN": { "name": "Historical Modern SSID" } }
            }))
            .expect("modern entry");
        let websocket: unifly_api::websocket::UnifiEvent =
            serde_json::from_value(serde_json::json!({
                "key": "EVT_WU_Connected", "subsystem": "wlan", "site_id": "default",
                "datetime": "2026-11-06T01:00:00Z", "msg": "Client connected to {ssid}",
                "ssid": "Historical Websocket SSID"
            }))
            .expect("WebSocket event");
        let sanitizer = Sanitizer::new(&test_config());
        for event in [
            Event::try_from(modern).expect("modern event"),
            Event::from(websocket),
        ] {
            let raw_ssid = event.ssid.as_deref().expect("SSID retained");
            let clean = sanitizer.sanitize_event(&event);
            let stand_in = sanitizer.sanitize_ssid(raw_ssid);
            assert_eq!(clean.ssid.as_deref(), Some(stand_in.as_str()));
            assert_eq!(clean.message, format!("Client connected to {stand_in}"));
            assert!(
                !serde_json::to_string(&clean)
                    .expect("JSON")
                    .contains(raw_ssid)
            );
        }
    }

    #[test]
    fn modern_wlan_parameter_shapes_seed_retired_ssids_before_rendering() {
        use serde_json::json;
        for wlan in [
            json!("Retired Net"),
            json!({"hostname": "Retired Net"}),
            json!({"name": "", "hostname": "Retired Net"}),
            json!({"id": "Retired Net"}),
            json!({"value": "Retired Net"}),
        ] {
            let entry: unifly_api::session::system_log::SystemLogEntry =
                serde_json::from_value(json!({
                    "timestamp": 1_794_001_000_000_i64,
                    "message_raw": "Client joined {WLAN}",
                    "parameters": {"WLAN": wlan}
                }))
                .expect("system log");
            let event = Event::try_from(entry).expect("event");
            assert_eq!(event.ssid.as_deref(), Some("Retired Net"));
            let sanitizer = Sanitizer::new(&test_config());
            let clean = sanitizer.sanitize_event(&event);
            assert_eq!(
                clean.message,
                format!("Client joined {}", sanitizer.sanitize_ssid("Retired Net"))
            );
            assert!(
                !serde_json::to_string(&clean)
                    .expect("JSON")
                    .contains("Retired Net")
            );
        }
    }

    #[test]
    fn blank_ssids_do_not_consume_stand_ins_or_hide_legacy_fallback() {
        let sanitizer = Sanitizer::new(&test_config());
        assert_eq!(sanitizer.sanitize_ssid(""), "");
        assert_eq!(sanitizer.ssid_counter.load(Ordering::Relaxed), 1);
        let entry: unifly_api::session::models::SessionEvent =
            serde_json::from_value(serde_json::json!({
                "_id": "event", "essid": "", "ssid": "Retired Net", "msg": "Joined {ssid}"
            }))
            .expect("session event");
        let clean = sanitizer.sanitize_event(&Event::from(entry));
        assert!(
            !serde_json::to_string(&clean)
                .expect("JSON")
                .contains("Retired Net")
        );
    }

    #[test]
    fn concurrent_ssid_and_public_ip_calls_publish_one_mapping() {
        let sanitizer = Sanitizer::new(&test_config());
        let barrier = std::sync::Barrier::new(16);
        let values = std::thread::scope(|scope| {
            let tasks: Vec<_> = (0..16)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        (
                            sanitizer.sanitize_ssid("Shared Network"),
                            sanitizer.sanitize_wan_ip("8.8.4.4".parse().expect("public IP")),
                        )
                    })
                })
                .collect();
            tasks
                .into_iter()
                .map(|task| task.join().expect("thread"))
                .collect::<Vec<_>>()
        });
        assert!(values.iter().all(|value| value == &values[0]));
        assert_eq!(sanitizer.ssid_counter.load(Ordering::Relaxed), 2);
        assert_eq!(sanitizer.wan_ip_counter.load(Ordering::Relaxed), 2);
        assert_eq!(sanitizer.sanitize_ssid(&values[0].0), values[0].0);
        assert_eq!(sanitizer.sanitize_wan_ip(values[0].1), values[0].1);
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
            "WAN up on 2001:db8:cafe::1, gateway fe80::cafe:2."
        );
        assert_eq!(
            s.sanitize_text("listening on [2600:1700:abcd::7]:443"),
            "listening on [2001:db8:cafe::3]:443"
        );
        // An address seen again gets the stand-in it got before.
        assert_eq!(
            s.sanitize_text("gateway fe80::1 again"),
            "gateway fe80::cafe:2 again"
        );
        // Loopback and the unspecified address identify nothing.
        assert_eq!(
            s.sanitize_text("bound to :: and ::1"),
            "bound to :: and ::1"
        );
    }

    #[test]
    fn default_text_redacts_ipv6_glued_to_its_neighbours() {
        for (text, expected) in [
            ("addr:2001:470::1", "addr:2001:db8:cafe::1"),
            ("IPv6:2001:470:1f0b::2/64", "IPv6:2001:db8:cafe::1/64"),
            ("2001:470::1_x", "2001:db8:cafe::1_x"),
            ("WAN 2001:470::1: down", "WAN 2001:db8:cafe::1: down"),
            ("fe80::1%eth0 is up", "fe80::cafe:1%eth0 is up"),
            ("2001:db8::cafe stays", "2001:db8::cafe stays"),
            (
                "EUI-64 0011:2233:4455:6677:8899:aabb:ccdd:eeff",
                "EUI-64 2001:db8:cafe::1",
            ),
        ] {
            let s = Sanitizer::new(&DemoConfig::default());
            assert_eq!(s.sanitize_text(text), expected, "{text}");
        }
        let s = Sanitizer::new(&DemoConfig::default());
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
    fn an_ipv6_stand_in_keeps_its_reach_and_is_not_mapped_again() {
        let s = Sanitizer::new(&DemoConfig::default());
        let stand_in = |text: &str| s.sanitize_ipv6(text.parse().expect("an IPv6 address"));
        let global = stand_in("2601:600:8284:c9d1::9");
        let unique_local = stand_in("fd00:c0de:babe:d::9");
        let link_local = stand_in("fe80::1c2d:3eff:fe4f:5a6b");
        assert_eq!(Ipv6Scope::of(&global), Ipv6Scope::Global);
        assert_eq!(Ipv6Scope::of(&unique_local), Ipv6Scope::UniqueLocal);
        assert_eq!(Ipv6Scope::of(&link_local), Ipv6Scope::LinkLocal);
        for redacted in [global, unique_local, link_local] {
            assert_eq!(s.sanitize_ipv6(redacted), redacted);
        }
    }

    /// Text and a field holding the same address show the same stand-in,
    /// an IPv4-mapped one included.
    #[test]
    fn an_ipv6_address_reads_the_same_in_text_and_in_a_field() {
        let s = Sanitizer::new(&DemoConfig::default());
        let mapped: Ipv6Addr = "::ffff:8.8.8.8".parse().expect("an IPv6 address");
        let in_text = s.sanitize_text("peer ::ffff:8.8.8.8 dropped");
        assert_eq!(in_text, format!("peer {} dropped", s.sanitize_ipv6(mapped)));
        let v6_ip = Some(IpAddr::V6(
            "2601:600:8284:c9d1::9".parse().expect("address"),
        ));
        assert_eq!(
            s.sanitize_ip_opt(&v6_ip),
            Some(IpAddr::V6(s.sanitize_ipv6(
                "2601:600:8284:c9d1::9".parse().expect("address")
            )))
        );
        assert_ne!(s.sanitize_ip_opt(&v6_ip), v6_ip);
    }

    /// A SLAAC address spells out the MAC it was built from.
    #[test]
    fn redacting_macs_redacts_ipv6_addresses_too() {
        let s = Sanitizer::new(&DemoConfig {
            redact_wan_ips: false,
            redact_macs: true,
            ..DemoConfig::default()
        });
        let eui64 = "2601:600:8284:c9d1:6e63:f8ff:fe36:5342";
        assert!(
            !s.sanitize_text(&format!("from {eui64}"))
                .contains("fe36:5342")
        );
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
    fn ssids_and_bssids_get_the_list_stand_ins_everywhere() {
        let s = Sanitizer::new(&DemoConfig {
            redact_ssids: true,
            redact_macs: true,
            ..DemoConfig::default()
        });
        let ssid = s.sanitize_ssid("NeonHarbor");
        let bssid = s.sanitize_mac(&MacAddress::new("aa:bb:cc:00:11:22"));

        assert_eq!(
            s.sanitize_text(r#"Kai connected to "NeonHarbor" via AA:BB:CC:00:11:22"#),
            format!(r#"Kai connected to "{ssid}" via {bssid}"#)
        );
        assert_eq!(
            s.sanitize_text("NeonHarbor5G"),
            "NeonHarbor5G",
            "whole words only"
        );

        let detail = s.sanitize_json_value(&serde_json::json!({
            "ssid": "NeonHarbor",
            "nearest_neighbors": [{ "bssid": "aa:bb:cc:00:11:22", "essid": "CafeGuest" }],
        }));
        assert_eq!(detail["ssid"], serde_json::json!(ssid));
        assert_eq!(detail["nearest_neighbors"][0]["bssid"], bssid.as_str());
        assert_eq!(
            detail["nearest_neighbors"][0]["essid"],
            serde_json::json!(s.sanitize_ssid("CafeGuest"))
        );
    }

    #[test]
    fn bare_and_dotted_macs_are_redacted_but_other_hex_is_not() {
        let s = Sanitizer::new(&DemoConfig {
            redact_macs: true,
            ..DemoConfig::default()
        });
        let mac = s.sanitize_mac(&MacAddress::new("aa:bb:cc:dd:ee:ff"));
        for text in ["client aabbccddeeff joined", "client AABB.CCDD.EEFF joined"] {
            assert_eq!(
                s.sanitize_text(text),
                format!("client {mac} joined"),
                "{text}"
            );
        }
        for text in [
            "sha 0123456789abcdef0123456789abcdef",
            "id 0123456789abc and 0123456789a",
            "aabb.ccdd.eeff.0011 and aabb.ccdd",
        ] {
            assert_eq!(s.sanitize_text(text), text);
        }
    }

    fn event(message: &str) -> Event {
        serde_json::from_value(serde_json::json!({
            "id": "evt-1", "timestamp": "2026-10-12T09:00:00Z",
            "category": "Client", "severity": "Info", "event_type": "EVT_WU_Connected",
            "message": message, "device_mac": null, "client_mac": null,
            "site_id": null, "raw_key": null
        }))
        .expect("event fixture")
    }

    #[test]
    fn ssids_an_item_names_are_mapped_before_its_text() {
        let s = Sanitizer::new(&DemoConfig {
            redact_ssids: true,
            ..DemoConfig::default()
        });

        // An event names its SSID only in its message.
        let clean = s.sanitize_event(&event(r#"Kai connected to "OtherNet" on ap-edit"#));
        let other = s.sanitize_ssid("OtherNet");
        assert_eq!(
            clean.message,
            format!(r#"Kai connected to "{other}" on ap-edit"#)
        );
        let clean = s.sanitize_event(&event(
            r#"User[aa:bb:cc:dd:ee:ff] has connected to AP[ap-1] with SSID "Home WiFi" on "channel 36(na)""#,
        ));
        assert!(!clean.message.contains("Home WiFi"), "{}", clean.message);
        assert!(
            clean.message.contains(r#""channel 36(na)""#),
            "{}",
            clean.message
        );

        // A disconnect names the SSID it left the same way.
        let clean = s.sanitize_event(&event(r#"Kai disconnected from "Secret Net""#));
        let secret = s.sanitize_ssid("Secret Net");
        assert_eq!(
            clean.message,
            format!(r#"Kai disconnected from "{secret}""#)
        );

        // A JSON item's text never runs ahead of the SSIDs its fields name.
        let roam = s.sanitize_json_value(&serde_json::json!({
            "msg": "roamed on CafeNet",
            "parameters": { "WLAN": { "name": "CafeNet" } },
        }));
        assert!(!roam.to_string().contains("CafeNet"), "{roam}");
        let detail = s.sanitize_json_value(&serde_json::json!({
            "note": "joined LabNet", "ssid": "LabNet",
        }));
        assert!(!detail.to_string().contains("LabNet"), "{detail}");

        // A neighbor's SSID is known once its record has been sanitized.
        let neighbor: RogueAp = serde_json::from_value(serde_json::json!({
            "bssid": "aa:bb:cc:00:11:22", "essid": "NeighborNet"
        }))
        .expect("rogue AP fixture");
        s.sanitize_rogue_ap(&neighbor);
        assert!(
            !s.sanitize_text("NeighborNet is loud")
                .contains("NeighborNet")
        );
    }

    #[test]
    fn a_json_name_that_is_a_mac_or_wan_address_is_redacted_like_any_other() {
        let s = Sanitizer::new(&DemoConfig {
            enabled: true,
            redact_names: vec!["Alice".into()],
            redact_macs: true,
            redact_wan_ips: true,
            ..DemoConfig::default()
        });
        let ap = s.sanitize_mac(&MacAddress::new("aa:bb:cc:00:11:22"));
        let alice = s.sanitize_name("Alice's Phone");
        assert_ne!(alice, "Alice's Phone");

        // A roam row for an AP nobody named, and a WiFi detail's uplink.
        let roam = s.sanitize_json_value(&serde_json::json!({
            "parameters": {
                "DEVICE_FROM": { "name": "aa:bb:cc:00:11:22" },
                "DEVICE_TO": { "name": "Lobby AP" },
                "CLIENT": { "name": "Alice's Phone" },
            },
            "uplink": { "display_name": "AA-BB-CC-00-11-22", "hostname": "8.8.4.4" },
        }));
        assert_eq!(roam["parameters"]["DEVICE_FROM"]["name"], ap.as_str());
        assert_eq!(roam["parameters"]["DEVICE_TO"]["name"], "Lobby AP");
        assert_eq!(roam["parameters"]["CLIENT"]["name"], alice.as_str());
        assert_eq!(roam["uplink"]["display_name"], ap.as_str());
        let drawn = roam.to_string();
        assert!(!drawn.contains("8.8.4.4"), "{drawn}");
        assert!(
            !drawn.to_lowercase().contains("aa:bb:cc:00:11:22"),
            "{drawn}"
        );
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
