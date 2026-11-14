//! Dashboard panel rendering split into focused submodules.
//!
//! Each panel keeps its own rendering logic so the main screen shell stays
//! small and the layout remains easy to navigate.

mod capacity;
mod gateway;
mod networks;
mod recent_events;
mod system_health;
mod top_clients;
mod traffic_chart;
mod wifi_aps;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use crate::tui::theme;

use super::DashboardScreen;
use super::focus::Panel;

/// Body rows under which the dashboard trades chrome for content: the
/// events panel becomes one line and the panels share what that frees.
const COMPACT_BELOW: u16 = 26;

/// Most rows the gateway, connectivity, and capacity panels take: the
/// eight lines the fullest of them writes, and its frame. Rows past that
/// go to the lists beneath, which grow with the site.
const STATUS_ROWS: u16 = 10;

impl DashboardScreen {
    pub(super) fn render_dashboard(&self, frame: &mut ratatui::Frame, area: Rect) {
        let refresh_str = self.refresh_age_str();
        let mut title = vec![
            Span::styled(" UniFi Dashboard ", theme::title_style()),
            Span::styled(
                format!(" [{refresh_str}] "),
                Style::default().fg(theme::border_unfocused()),
            ),
        ];
        match self.active_alarms {
            None | Some(0) => {}
            Some(1) => title.push(Span::styled(
                " \u{26A0} 1 alarm ",
                Style::default().fg(theme::warning()),
            )),
            Some(count) => title.push(Span::styled(
                format!(" \u{26A0} {count} alarms "),
                Style::default().fg(theme::warning()),
            )),
        }
        let title_line = Line::from(title);
        self.age_drawn.replace(refresh_str);

        let block = Block::default()
            .title(title_line)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(if self.focused {
                theme::border_focused()
            } else {
                theme::border_default()
            });

        let inner = block.inner(area);
        frame.render_widget(block, area);
        self.hits.borrow_mut().clear();
        self.panel_hits.borrow_mut().clear();

        if inner.width < 40 || inner.height < 10 {
            let summary = format!(
                "Devices: {} │ Clients: {}",
                self.devices.len(),
                self.clients.len()
            );
            frame.render_widget(Paragraph::new(summary).style(theme::table_row()), inner);
            return;
        }

        let body = inner;
        let compact = body.height < COMPACT_BELOW;
        let (traffic, events) = if compact {
            (5, 1)
        } else {
            (body.height * 28 / 100, (body.height * 14 / 100).max(3))
        };
        let rest = body.height.saturating_sub(traffic + events);
        let middle = if compact {
            rest.div_ceil(2)
        } else {
            (rest * 55 / 100).min(STATUS_ROWS)
        };
        let rows = Layout::vertical([
            Constraint::Length(traffic),
            Constraint::Length(middle),
            Constraint::Length(rest - middle),
            Constraint::Length(events),
        ])
        .split(body);
        let thirds = |row: Rect| {
            Layout::horizontal([
                Constraint::Percentage(33),
                Constraint::Percentage(33),
                Constraint::Percentage(34),
            ])
            .split(row)
        };
        let (mid_row, bottom_row) = (thirds(rows[1]), thirds(rows[2]));

        for (panel, area) in [
            (Panel::Traffic, rows[0]),
            (Panel::Gateway, mid_row[0]),
            (Panel::Connectivity, mid_row[1]),
            (Panel::Capacity, mid_row[2]),
            (Panel::Networks, bottom_row[0]),
            (Panel::Wifi, bottom_row[1]),
            (Panel::TopClients, bottom_row[2]),
            (Panel::Events, rows[3]),
        ] {
            self.panel_hits.borrow_mut().push(area, panel);
        }

        self.render_traffic_chart(frame, rows[0]);
        self.render_gateway(frame, mid_row[0]);
        self.render_system_health(frame, mid_row[1]);
        self.render_capacity(frame, mid_row[2]);
        self.render_networks(frame, bottom_row[0]);
        self.render_wifi_aps(frame, bottom_row[1]);
        self.render_top_clients(frame, bottom_row[2]);
        if compact {
            self.render_latest_event(frame, rows[3]);
        } else {
            self.render_recent_events(frame, rows[3]);
        }
    }

    /// Whether `panel` holds keyboard focus on a focused dashboard.
    pub(super) fn has_focus(&self, panel: Panel) -> bool {
        self.focused && self.focus == panel
    }

    /// A panel's title: its name, marked when the panel holds focus.
    pub(super) fn panel_title(&self, panel: Panel, name: &str) -> Span<'static> {
        let marker = if self.has_focus(panel) {
            "\u{25B8} "
        } else {
            ""
        };
        Span::styled(format!(" {marker}{name} "), theme::title_style())
    }

    /// A panel's frame, with the focus ring when it holds focus.
    pub(super) fn panel_block(&self, panel: Panel, name: &str) -> Block<'static> {
        Block::default()
            .title(self.panel_title(panel, name))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(if self.has_focus(panel) {
                theme::border_focused()
            } else {
                theme::border_default()
            })
    }

    /// What an empty panel says: that the controller is not there yet, or
    /// `nothing` once it is.
    pub(super) fn empty_text(&self, nothing: &'static str) -> &'static str {
        self.link.waiting().unwrap_or(nothing)
    }
}

/// One muted line for a panel with nothing to list.
pub(super) fn muted_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {text}"),
        Style::default().fg(theme::border_unfocused()),
    ))
}

#[cfg(test)]
mod tests {
    use crate::tui::action::Action;
    use crate::tui::component::Component;
    use crate::tui::screen::ScreenId;
    use crate::tui::screens::dashboard::DashboardScreen;
    use crate::tui::table::render_lines;
    use crate::tui::widgets::click::click_label;

    #[test]
    fn the_default_demo_sanitizer_hides_every_address_the_gateway_panel_draws() {
        use crate::config::DemoConfig;
        use crate::sanitizer::Sanitizer;

        let health: unifly_api::HealthSummary = serde_json::from_value(serde_json::json!({
            "subsystem": "wan", "status": "ok", "num_adopted": 1, "num_sta": null,
            "tx_bytes_r": 0, "rx_bytes_r": 0, "latency": 9.0,
            "wan_ip": "73.45.67.89", "gateways": ["73.45.67.1"],
            "extra": {
                "subsystem": "wan", "wan_ip": "73.45.67.89",
                "gateways": ["73.45.67.1"], "wan_ip6": "2601:646:8d00::1",
                "nameservers": ["75.75.75.75", "75.75.76.76"], "isp_name": "Comcast"
            }
        }))
        .expect("health fixture");
        let draw = |config: &DemoConfig| {
            let health = Sanitizer::new(config).sanitize_health_vec(std::slice::from_ref(&health));
            let mut screen = DashboardScreen::new();
            screen
                .update(&Action::HealthUpdated(health))
                .expect("health update");
            render_lines(140, 40, |frame, area| screen.render(frame, area)).join("\n")
        };

        let text = draw(&DemoConfig::default());
        assert!(text.contains(" GW "), "the gateway line is drawn:\n{text}");
        for real in ["73.45.67", "2601:646", "75.75.7"] {
            assert!(!text.contains(real), "the panel shows {real}:\n{text}");
        }
        assert!(
            text.contains("Comcast"),
            "ISP names stay unless redact_isp is on"
        );

        let text = draw(&DemoConfig {
            redact_isp: true,
            ..DemoConfig::default()
        });
        assert!(!text.contains("Comcast"), "{text}");
    }

    #[test]
    fn a_panel_tap_opens_its_screen() {
        let mut screen = DashboardScreen::new();
        let rows = render_lines(140, 40, |frame, area| screen.render(frame, area));

        for (title, target) in [
            ("WAN Traffic", ScreenId::Stats),
            ("Top Clients", ScreenId::Clients),
            ("Recent Events", ScreenId::Events),
        ] {
            let action = screen
                .handle_mouse_event(click_label(&rows, title, title))
                .expect("click handling");
            assert!(
                matches!(action, Some(Action::SwitchScreen(screen)) if screen == target),
                "{title}: {action:?}"
            );
        }
    }
}
