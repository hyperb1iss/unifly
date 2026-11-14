use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::tui::screens::events::render::clip;
use crate::tui::theme;
use crate::tui::widgets::bytes_fmt;

use super::super::DashboardScreen;
use super::super::focus::Panel;
use super::super::helpers::{parse_ipv6_from_text, parse_ipv6_from_value};
use super::muted_line;

impl DashboardScreen {
    /// Gateway panel - WAN connection details with IPv6.
    #[allow(clippy::too_many_lines)]
    pub(super) fn render_gateway(&self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        let block = self.panel_block(Panel::Gateway, "Gateway");

        let inner = block.inner(area);
        frame.render_widget(block, area);
        let w = usize::from(inner.width);

        let gateway = self.gateway();
        let wan_health = self.health.iter().find(|h| h.subsystem == "wan");
        let www_health = self.health.iter().find(|h| h.subsystem == "www");

        let isp_name = wan_health
            .and_then(|h| h.extra.get("isp_name").and_then(|v| v.as_str()))
            .or_else(|| {
                wan_health.and_then(|h| h.extra.get("isp_organization").and_then(|v| v.as_str()))
            });
        let dns: Option<Vec<&str>> = wan_health
            .and_then(|h| h.extra.get("nameservers").and_then(|v| v.as_array()))
            .map(|ns| ns.iter().filter_map(|v| v.as_str()).collect());
        let gw_ip = wan_health
            .and_then(|h| h.extra.get("gateways").and_then(|v| v.as_array()))
            .and_then(|a| a.first().and_then(|v| v.as_str()));
        let wan_ipv6 = wan_health
            .and_then(|h| {
                const IPV6_KEYS: &[&str] = &[
                    "wan_ip6",
                    "wan_ip6s",
                    "wan_ipv6",
                    "wan_ipv6s",
                    "ipv6",
                    "ipv6Address",
                    "ipv6_address",
                ];

                for key in IPV6_KEYS {
                    if let Some(ipv6) = h.extra.get(*key).and_then(parse_ipv6_from_value) {
                        return Some(ipv6);
                    }
                }

                if let Some(ipv6) = h
                    .extra
                    .get("wan_ip")
                    .and_then(parse_ipv6_from_value)
                    .or_else(|| h.wan_ip.as_deref().and_then(parse_ipv6_from_text))
                {
                    return Some(ipv6);
                }

                h.gateways
                    .as_ref()
                    .and_then(|gateways| gateways.iter().find_map(|gw| parse_ipv6_from_text(gw)))
            })
            .or_else(|| gateway.and_then(|g| g.wan_ipv6.clone()));

        let gw_version =
            wan_health.and_then(|h| h.extra.get("gw_version").and_then(|v| v.as_str()));
        let latency = www_health.and_then(|h| h.latency);
        let uptime = gateway.and_then(|g| g.stats.uptime_secs);
        let wan_ip = gateway
            .and_then(|g| g.ip)
            .map(|ip| ip.to_string())
            .or_else(|| wan_health.and_then(|h| h.wan_ip.clone()));

        // Most telling first: a short panel shows as many as fit.
        let mut lines = Vec::new();

        if let Some(gw) = gateway {
            let model = gw.model.as_deref().unwrap_or("Gateway");
            let fw = gw_version.or(gw.firmware_version.as_deref()).unwrap_or("─");
            let header = clip(&format!("{model} ({fw})"), w.saturating_sub(4));
            lines.push(Line::from(vec![
                Span::styled(" ◈ ", Style::default().fg(theme::accent_primary())),
                Span::styled(
                    header,
                    Style::default()
                        .fg(theme::accent_secondary())
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        } else {
            lines.push(muted_line(self.empty_text("No UniFi gateway on this site")));
        }

        let room = w.saturating_sub(LABEL_COLUMN);
        let kv = |label: &str, value: &str, color: ratatui::style::Color| -> Line<'static> {
            Line::from(vec![field_label(label), styled(clip(value, room), color)])
        };

        lines.push(kv(
            "WAN",
            &wan_ip.unwrap_or_else(|| "─".into()),
            theme::accent_tertiary(),
        ));
        if let Some(isp) = isp_name {
            lines.push(kv("ISP", isp, theme::text_secondary()));
        }
        let lat_str = latency.map_or_else(|| "─".into(), |l| format!("{l:.0}ms"));
        let up_str = uptime.map_or_else(|| "─".into(), bytes_fmt::fmt_uptime);
        lines.push(Line::from(vec![
            field_label("Latency"),
            styled(lat_str, theme::accent_secondary()),
            styled("   Up ".to_owned(), theme::text_secondary()),
            styled(up_str, theme::accent_secondary()),
        ]));
        let speed = [www_health, wan_health]
            .into_iter()
            .flatten()
            .find_map(|health| SpeedTest::read(&health.extra));
        if let Some(speed) = speed {
            let text = speed.fitted(chrono::Utc::now().timestamp(), room);
            lines.push(Line::from(vec![
                field_label("Speed"),
                styled(text, theme::accent_secondary()),
            ]));
        }
        if let Some(servers) = dns.as_deref().filter(|servers| !servers.is_empty()) {
            let shown = fitted_servers(servers, room);
            lines.push(kv("DNS", &shown, theme::text_secondary()));
        }
        if let Some(gw) = gw_ip {
            lines.push(kv("Gateway", gw, theme::text_secondary()));
        }
        lines.push(kv(
            "IPv6",
            wan_ipv6.as_deref().unwrap_or("─"),
            theme::info(),
        ));

        frame.render_widget(Paragraph::new(lines), inner);
    }
}

/// Columns a field's label takes: a margin, the longest label, and a gap.
const LABEL_COLUMN: usize = 1 + "Latency".len() + 1;

fn field_label(label: &str) -> Span<'static> {
    Span::styled(
        format!(" {label:<width$}", width = LABEL_COLUMN - 1),
        Style::default().fg(theme::text_secondary()),
    )
}

fn styled(text: String, color: ratatui::style::Color) -> Span<'static> {
    Span::styled(text, Style::default().fg(color))
}

/// The first two of `servers` when both fit `width` whole, else the
/// first, then how many are left out. An address is never cut partway
/// while a whole one fits.
fn fitted_servers(servers: &[&str], width: usize) -> String {
    (1..=servers.len().min(2))
        .rev()
        .map(|shown| {
            let hidden = servers.len() - shown;
            let listed = servers[..shown].join(", ");
            if hidden > 0 {
                format!("{listed} +{hidden}")
            } else {
                listed
            }
        })
        .find(|text| Span::raw(text.as_str()).width() <= width)
        .unwrap_or_else(|| {
            servers
                .first()
                .map_or_else(String::new, |s| (*s).to_owned())
        })
}

/// The gateway's last WAN speed test, as site health reports it. A test
/// produces no event: its result replaces these fields about a minute
/// after it starts.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SpeedTest {
    down_mbps: f64,
    up_mbps: f64,
    ping_ms: Option<f64>,
    /// When it ran, in epoch seconds.
    ran_at: Option<i64>,
}

impl SpeedTest {
    /// Read the result out of a health subsystem's fields. A gateway that
    /// never ran a test reports no rates, or zeros.
    fn read(health: &serde_json::Value) -> Option<Self> {
        let number = |key: &str| health.get(key).and_then(serde_json::Value::as_f64);
        let down_mbps = number("xput_down").unwrap_or(0.0);
        let up_mbps = number("xput_up").unwrap_or(0.0);
        if down_mbps <= 0.0 && up_mbps <= 0.0 {
            return None;
        }
        Some(Self {
            down_mbps,
            up_mbps,
            ping_ms: number("speedtest_ping").filter(|ping| *ping > 0.0),
            ran_at: health
                .get("speedtest_lastrun")
                .and_then(serde_json::Value::as_i64)
                .filter(|at| *at > 0)
                // Some firmware reports milliseconds.
                .map(|at| if at > 100_000_000_000 { at / 1000 } else { at }),
        })
    }

    /// The fullest wording that fits `width` columns at `now`, in epoch
    /// seconds: both rates always, then how long ago, then the ping.
    fn fitted(self, now: i64, width: usize) -> String {
        let age = self.ran_at.map(|at| age_text(now.saturating_sub(at)));
        let ago = age.as_ref().map(|age| match age.as_str() {
            "now" => "just now".to_owned(),
            age => format!("{age} ago"),
        });
        let long = format!(
            "\u{2193} {}  \u{2191} {}",
            long_rate(self.down_mbps),
            long_rate(self.up_mbps)
        );
        let short = format!(
            "\u{2193}{} \u{2191}{}",
            short_rate(self.down_mbps),
            short_rate(self.up_mbps)
        );
        let mut wordings = Vec::new();
        if let (Some(ping), Some(ago)) = (self.ping_ms, &ago) {
            wordings.push(format!("{long}  {ping:.0} ms  {ago}"));
        }
        if let Some(ago) = &ago {
            wordings.push(format!("{long}  {ago}"));
            wordings.push(format!("{short} {ago}"));
        }
        if let Some(age) = &age {
            wordings.push(format!("{short} {age}"));
        }
        wordings.push(long);
        wordings
            .into_iter()
            .find(|wording| Span::raw(wording.as_str()).width() <= width)
            .unwrap_or(short)
    }
}

fn long_rate(mbps: f64) -> String {
    if mbps >= 1000.0 {
        format!("{:.2} Gbps", mbps / 1000.0)
    } else {
        format!("{mbps:.0} Mbps")
    }
}

fn short_rate(mbps: f64) -> String {
    if mbps >= 1000.0 {
        format!("{:.1}G", mbps / 1000.0)
    } else {
        format!("{mbps:.0}M")
    }
}

/// How long ago, from seconds: "now", "4m", "5h", "3d".
fn age_text(secs: i64) -> String {
    match secs {
        ..60 => "now".into(),
        60..3_600 => format!("{}m", secs / 60),
        3_600..86_400 => format!("{}h", secs / 3_600),
        _ => format!("{}d", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{SpeedTest, fitted_servers};

    const NOW: i64 = 1_800_000_000;

    #[test]
    fn a_speed_test_result_is_read_from_site_health() {
        let health = json!({
            "subsystem": "www", "xput_down": 2317.2, "xput_up": 941.6,
            "speedtest_ping": 6, "speedtest_lastrun": NOW - 5 * 3600,
            "speedtest_status": "Success"
        });
        let test = SpeedTest::read(&health).expect("a result");

        assert_eq!(
            test.fitted(NOW, 60),
            "\u{2193} 2.32 Gbps  \u{2191} 942 Mbps  6 ms  5h ago"
        );
        assert_eq!(
            test.fitted(NOW, 32),
            "\u{2193} 2.32 Gbps  \u{2191} 942 Mbps  5h ago"
        );
        assert_eq!(test.fitted(NOW, 18), "\u{2193}2.3G \u{2191}942M 5h ago");
        assert_eq!(test.fitted(NOW, 14), "\u{2193}2.3G \u{2191}942M 5h");
        assert_eq!(test.fitted(NOW, 4), "\u{2193}2.3G \u{2191}942M");
    }

    #[test]
    fn name_servers_are_listed_whole_or_counted() {
        let servers = ["192.0.2.53", "198.51.100.53", "203.0.113.53"];
        assert_eq!(
            fitted_servers(&servers[..2], 25),
            "192.0.2.53, 198.51.100.53"
        );
        assert_eq!(fitted_servers(&servers[..2], 24), "192.0.2.53 +1");
        assert_eq!(fitted_servers(&servers, 40), "192.0.2.53, 198.51.100.53 +1");
        assert_eq!(fitted_servers(&servers, 20), "192.0.2.53 +2");
        assert_eq!(fitted_servers(&servers[..1], 4), "192.0.2.53");
    }

    #[test]
    fn a_fresh_result_says_so_and_millisecond_times_are_understood() {
        let health = json!({
            "xput_down": 80.0, "xput_up": 20.0,
            "speedtest_lastrun": (NOW - 30) * 1000
        });
        let test = SpeedTest::read(&health).expect("a result");
        assert_eq!(
            test.fitted(NOW, 60),
            "\u{2193} 80 Mbps  \u{2191} 20 Mbps  just now"
        );
    }

    #[test]
    fn a_gateway_that_never_ran_a_test_shows_no_result() {
        for health in [
            json!({"subsystem": "www"}),
            json!({"xput_down": 0, "xput_up": 0.0, "speedtest_lastrun": 0}),
        ] {
            assert_eq!(SpeedTest::read(&health), None, "{health}");
        }
    }
}
