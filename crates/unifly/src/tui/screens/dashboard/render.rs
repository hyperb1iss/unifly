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

use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use crate::tui::action::Action;
use crate::tui::screen::ScreenId;
use crate::tui::theme;
use crate::tui::widgets::click::Click;

use super::DashboardScreen;

impl DashboardScreen {
    pub(super) fn render_dashboard(&self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        let refresh_str = self.refresh_age_str();
        let title_line = Line::from(vec![
            Span::styled(" UniFi Dashboard ", theme::title_style()),
            Span::styled(
                format!(" [{refresh_str}] "),
                Style::default().fg(theme::border_unfocused()),
            ),
        ]);

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
        let mut hits = self.hits.borrow_mut();
        hits.clear();

        if inner.width < 40 || inner.height < 10 {
            let summary = format!(
                "Devices: {} │ Clients: {}",
                self.devices.len(),
                self.clients.len()
            );
            frame.render_widget(Paragraph::new(summary).style(theme::table_row()), inner);
            return;
        }

        let rows = Layout::vertical([
            Constraint::Percentage(28),
            Constraint::Percentage(32),
            Constraint::Percentage(26),
            Constraint::Percentage(14),
        ])
        .split(inner);

        self.render_traffic_chart(frame, rows[0]);
        hits.push(rows[0], open(ScreenId::Stats));

        let mid_row = Layout::horizontal([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .split(rows[1]);

        self.render_gateway(frame, mid_row[0]);
        self.render_system_health(frame, mid_row[1]);
        self.render_capacity(frame, mid_row[2]);
        hits.push(mid_row[0], open(ScreenId::Devices));
        hits.push(mid_row[2], open(ScreenId::Devices));

        let bottom_row = Layout::horizontal([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .split(rows[2]);

        self.render_networks(frame, bottom_row[0]);
        self.render_wifi_aps(frame, bottom_row[1]);
        self.render_top_clients(frame, bottom_row[2]);
        hits.push(bottom_row[0], open(ScreenId::Networks));
        hits.push(bottom_row[1], open(ScreenId::Wifi));
        hits.push(bottom_row[2], open(ScreenId::Clients));

        self.render_recent_events(frame, rows[3]);
        hits.push(rows[3], open(ScreenId::Events));
    }
}

/// A tap that opens `screen`.
fn open(screen: ScreenId) -> Click {
    Click::Action(Action::SwitchScreen(screen))
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
