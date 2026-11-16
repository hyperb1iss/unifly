//! Screen implementations. Each screen is a top-level Component.

pub mod clients;
pub mod dashboard;
pub mod devices;
pub mod events;
pub mod firewall;
mod link;
pub mod networks;
pub mod onboarding;
pub mod settings;
pub mod stats;
pub mod topology;
pub mod wifi;

use crate::tui::component::Component;
use crate::tui::screen::ScreenId;

/// Create all primary screen components, returning them as boxed trait objects.
pub fn create_screens() -> Vec<(ScreenId, Box<dyn Component>)> {
    vec![
        (
            ScreenId::Dashboard,
            Box::new(dashboard::DashboardScreen::new()),
        ),
        (ScreenId::Devices, Box::new(devices::DevicesScreen::new())),
        (ScreenId::Clients, Box::new(clients::ClientsScreen::new())),
        (
            ScreenId::Networks,
            Box::new(networks::NetworksScreen::new()),
        ),
        (
            ScreenId::Firewall,
            Box::new(firewall::FirewallScreen::new()),
        ),
        (
            ScreenId::Topology,
            Box::new(topology::TopologyScreen::new()),
        ),
        (ScreenId::Events, Box::new(events::EventsScreen::new())),
        (ScreenId::Stats, Box::new(stats::StatsScreen::new())),
        (ScreenId::Wifi, Box::new(wifi::WifiScreen::new())),
    ]
}
