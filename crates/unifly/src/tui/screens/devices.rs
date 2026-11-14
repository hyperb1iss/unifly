//! Devices screen: the device table with its filter and sort, and a detail
//! pane with a tab per aspect of the selected device.

mod commands;
mod detail;
#[cfg(test)]
pub(in crate::tui) mod fixtures;
mod input;
mod render;
mod state;
pub(in crate::tui::screens) mod view;

use std::cell::RefCell;
use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::TableState;

use unifly_api::{Client, Device, EntityId};

use crate::tui::action::{Access, Action, DeviceDetailTab};
use crate::tui::component::{Component, LIST_HELP};
use crate::tui::table::TableScroll;
use crate::tui::widgets::click::{Click, HitMap};

pub(in crate::tui::screens) use render::grid;

pub struct DevicesScreen {
    focused: bool,
    /// Every device on the site, by name.
    devices: Arc<Vec<Arc<Device>>>,
    clients: Arc<Vec<Arc<Client>>>,
    /// What the controller can reach. `None` until it has connected.
    access: Option<Access>,
    table_state: TableState,
    table_scroll: TableScroll,
    selected_device_id: Option<EntityId>,
    filter: view::DeviceFilter,
    sort: view::DeviceSort,
    search_query: String,
    /// The open detail pane.
    detail: Option<detail::Detail>,
    /// What the last frame drew where, for clicks.
    hits: RefCell<HitMap<Click>>,
    /// The detail pane and its list rows as last drawn.
    detail_hits: RefCell<detail::DetailHits>,
    /// The device and port a rename prompt is open for.
    renaming: Option<(EntityId, u32)>,
}

impl Default for DevicesScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for DevicesScreen {
    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        Ok(self.handle_key_input(key))
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        Ok(self.handle_mouse_input(mouse))
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        Ok(self.apply_action(action))
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        self.render_screen(frame, area);
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn search_query(&self) -> Option<&str> {
        Some(&self.search_query)
    }

    fn help(&self) -> Vec<(&'static str, &'static str)> {
        let Some(detail) = &self.detail else {
            let mut keys = LIST_HELP.to_vec();
            keys.extend(LIST_KEYS);
            keys.extend(DEVICE_KEYS);
            return keys;
        };
        let mut keys = vec![("h/l \u{2190}/\u{2192}", "Previous / next tab")];
        keys.extend(match detail.tab {
            DeviceDetailTab::Ports => PORT_KEYS,
            DeviceDetailTab::Clients => CLIENT_KEYS,
            _ => DEVICE_STEP_KEYS,
        });
        keys.extend(DEVICE_KEYS);
        keys.extend([("F", "Forget the device"), ("Esc", "Close the detail")]);
        keys
    }

    fn id(&self) -> &'static str {
        "Devices"
    }
}

/// Keys of the device table, after the list movement keys.
const LIST_KEYS: &[(&str, &str)] = &[
    ("Enter", "Open the device"),
    ("h/l \u{2190}/\u{2192}", "Previous / next filter"),
    ("s", "Sort by the next column"),
    ("r", "Reverse the sort"),
];

/// Commands for the selected device, in the table and in the detail.
const DEVICE_KEYS: &[(&str, &str)] = &[
    ("A", "Adopt a pending device"),
    ("R", "Restart"),
    ("L", "Locate (LED on / off)"),
    ("U", "Upgrade firmware"),
    ("P", "Provision again"),
    ("T", "Gateway speed test"),
];

/// Movement on the Overview, Performance, and Radios tabs, which hold no
/// list: it steps through the devices, keeping the tab.
const DEVICE_STEP_KEYS: &[(&str, &str)] = &[
    ("j/k \u{2191}/\u{2193}", "Previous / next device"),
    ("g/G", "First / last device"),
];

/// The Ports tab: movement drives its port list, and the port commands
/// act on the port under the cursor.
const PORT_KEYS: &[(&str, &str)] = &[
    ("j/k \u{2191}/\u{2193}", "Move through the ports"),
    ("g/G", "First / last port"),
    ("PgUp/PgDn", "Page through the ports"),
    ("C", "Power cycle the PoE port"),
    ("O", "Port PoE off / on"),
    ("N", "Rename the port"),
];

/// The Clients tab: movement drives its client list.
const CLIENT_KEYS: &[(&str, &str)] = &[
    ("j/k \u{2191}/\u{2193}", "Move through the clients"),
    ("g/G", "First / last client"),
    ("PgUp/PgDn", "Page through the clients"),
    ("Enter", "Open the client"),
];
