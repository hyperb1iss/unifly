//! The open detail pane: which device and tab it shows, the cursor in the
//! tab's list, and the port profiles fetched for the Ports tab.

use std::sync::Arc;

use ratatui::layout::{Position, Rect};
use ratatui::widgets::TableState;

use unifly_api::model::{Port, PortProfile};
use unifly_api::{Client, Device, EntityId};

use super::DevicesScreen;
use crate::tui::action::{Action, DeviceDetailTab};
use crate::tui::table::TableScroll;
use crate::tui::widgets::click::HitMap;

/// The per-port configuration the controller holds for a switch.
#[derive(Debug, Default)]
pub(super) enum Profiles {
    /// Nothing asked for yet.
    #[default]
    Unasked,
    Loading,
    Loaded(Arc<Vec<PortProfile>>),
    /// Why the profiles cannot be shown.
    Unavailable(String),
}

#[derive(Debug)]
pub(super) struct Detail {
    pub(super) device_id: EntityId,
    pub(super) tab: DeviceDetailTab,
    /// The cursor in the Ports or Clients list.
    pub(super) cursor: TableState,
    pub(super) scroll: TableScroll,
    pub(super) profiles: Profiles,
}

impl Detail {
    pub(super) fn new(device_id: EntityId, tab: DeviceDetailTab) -> Self {
        Self {
            device_id,
            tab,
            cursor: TableState::default().with_selected(Some(0)),
            scroll: TableScroll::default(),
            profiles: Profiles::default(),
        }
    }

    pub(super) fn cursor_index(&self) -> usize {
        self.cursor.selected().unwrap_or(0)
    }

    /// The fetched profile of port `index`.
    pub(super) fn profile(&self, index: u32) -> Option<&PortProfile> {
        match &self.profiles {
            Profiles::Loaded(profiles) => profiles.iter().find(|profile| profile.index == index),
            _ => None,
        }
    }
}

/// Where the detail pane and the rows of its list were last drawn.
#[derive(Debug, Default)]
pub(super) struct DetailHits {
    pub(super) pane: Rect,
    pub(super) rows: HitMap<usize>,
}

impl DetailHits {
    pub(super) fn clear(&mut self) {
        self.pane = Rect::default();
        self.rows.clear();
    }

    pub(super) fn contains(&self, column: u16, row: u16) -> bool {
        self.pane.contains(Position::new(column, row))
    }
}

/// The tabs that apply to `device`: radios only where there are any, and
/// ports only where there are any.
pub(super) fn tabs_for(device: &Device) -> Vec<DeviceDetailTab> {
    let mut tabs = vec![DeviceDetailTab::Overview, DeviceDetailTab::Performance];
    if device.has_access_point || !device.radios.is_empty() {
        tabs.push(DeviceDetailTab::Radios);
    }
    tabs.push(DeviceDetailTab::Clients);
    if !device.ports.is_empty() {
        tabs.push(DeviceDetailTab::Ports);
    }
    tabs
}

pub(super) fn tab_label(tab: DeviceDetailTab) -> &'static str {
    match tab {
        DeviceDetailTab::Overview => "Overview",
        DeviceDetailTab::Performance => "Performance",
        DeviceDetailTab::Radios => "Radios",
        DeviceDetailTab::Clients => "Clients",
        DeviceDetailTab::Ports => "Ports",
    }
}

/// A device's ports in index order.
pub(super) fn ordered_ports(device: &Device) -> Vec<&Port> {
    let mut ports: Vec<&Port> = device.ports.iter().collect();
    ports.sort_by_key(|port| port.index);
    ports
}

impl DevicesScreen {
    pub(super) fn detail_device(&self) -> Option<&Arc<Device>> {
        let id = &self.detail.as_ref()?.device_id;
        self.devices.iter().find(|device| device.id == *id)
    }

    /// Clients connected through `device`: by uplink MAC, else uplink ID,
    /// else, for a wireless client reporting neither, by BSSID.
    pub(super) fn device_clients(&self, device: &Device) -> Vec<&Arc<Client>> {
        self.clients
            .iter()
            .filter(|client| {
                if let Some(mac) = &client.uplink_device_mac {
                    return *mac == device.mac;
                }
                if let Some(id) = &client.uplink_device_id {
                    return *id == device.id;
                }
                client
                    .wireless
                    .as_ref()
                    .and_then(|wireless| wireless.bssid.as_ref())
                    .is_some_and(|bssid| *bssid == device.mac)
            })
            .collect()
    }

    /// Rows in the list of the open tab; zero for a tab without one.
    pub(super) fn detail_list_len(&self) -> usize {
        let (Some(detail), Some(device)) = (&self.detail, self.detail_device()) else {
            return 0;
        };
        match detail.tab {
            DeviceDetailTab::Ports => device.ports.len(),
            DeviceDetailTab::Clients => self.device_clients(device).len(),
            _ => 0,
        }
    }

    /// Whether movement keys drive the open tab's own list.
    pub(super) fn detail_has_list(&self) -> bool {
        self.detail.as_ref().is_some_and(|detail| {
            matches!(
                detail.tab,
                DeviceDetailTab::Ports | DeviceDetailTab::Clients
            )
        })
    }

    pub(super) fn set_detail_cursor(&mut self, index: usize) {
        let last = self.detail_list_len().saturating_sub(1);
        if let Some(detail) = &mut self.detail {
            detail.cursor.select(Some(index.min(last)));
        }
    }

    pub(super) fn move_detail_cursor(&mut self, delta: isize) {
        let current = self.detail.as_ref().map_or(0, Detail::cursor_index);
        self.set_detail_cursor(current.saturating_add_signed(delta));
    }

    /// The port under the Ports tab's cursor.
    pub(super) fn cursor_port(&self) -> Option<(&Arc<Device>, &Port)> {
        let detail = self.detail.as_ref()?;
        if detail.tab != DeviceDetailTab::Ports {
            return None;
        }
        let device = self.detail_device()?;
        let port = ordered_ports(device).get(detail.cursor_index()).copied()?;
        Some((device, port))
    }

    /// The client under the Clients tab's cursor.
    pub(super) fn cursor_client(&self) -> Option<&Arc<Client>> {
        let detail = self.detail.as_ref()?;
        if detail.tab != DeviceDetailTab::Clients {
            return None;
        }
        let device = self.detail_device()?;
        self.device_clients(device)
            .get(detail.cursor_index())
            .copied()
    }

    /// Open the detail of the selected device on its Overview.
    pub(super) fn open_selected_detail(&mut self) {
        if let Some(id) = self.selected_device().map(|device| device.id.clone()) {
            self.detail = Some(Detail::new(id, DeviceDetailTab::Overview));
        }
    }

    pub(super) fn close_detail(&mut self) {
        self.detail = None;
    }

    /// Point the open detail at the selected device, staying on the same
    /// tab where that device has it, so stepping from switch to switch
    /// stays on their ports.
    pub(super) fn retarget_detail(&mut self) -> Option<Action> {
        let device = self.selected_device()?;
        let id = device.id.clone();
        let tabs = tabs_for(device);
        let detail = self.detail.as_ref()?;
        if detail.device_id == id {
            return None;
        }
        let tab = if tabs.contains(&detail.tab) {
            detail.tab
        } else {
            DeviceDetailTab::Overview
        };
        self.detail = Some(Detail::new(id, tab));
        self.load_tab()
    }

    /// Switch the open detail to `tab`.
    pub(super) fn set_detail_tab(&mut self, tab: DeviceDetailTab) -> Option<Action> {
        let detail = self.detail.as_mut()?;
        if detail.tab != tab {
            detail.tab = tab;
            detail.cursor.select(Some(0));
        }
        self.load_tab()
    }

    /// Step the open detail `step` tabs along the ones its device has.
    pub(super) fn step_detail_tab(&mut self, step: isize) -> Option<Action> {
        let tabs = tabs_for(self.detail_device()?);
        let current = self.detail.as_ref()?.tab;
        let at = tabs.iter().position(|tab| *tab == current).unwrap_or(0);
        self.set_detail_tab(tabs[super::view::wrapped(at, step, tabs.len())])
    }

    /// Ask for what the open tab needs from the controller: the port
    /// profiles, the first time the Ports tab shows.
    fn load_tab(&mut self) -> Option<Action> {
        let detail = self.detail.as_mut()?;
        if detail.tab != DeviceDetailTab::Ports || !matches!(detail.profiles, Profiles::Unasked) {
            return None;
        }
        detail.profiles = Profiles::Loading;
        Some(Action::RequestPortProfiles(detail.device_id.clone()))
    }

    /// Keep the open detail valid after its device changed: a tab the
    /// device no longer has falls back to Overview, and the cursor stays
    /// inside the list.
    pub(super) fn reconcile_detail(&mut self) {
        let Some(tabs) = self.detail_device().map(|device| tabs_for(device)) else {
            return;
        };
        if let Some(detail) = &mut self.detail
            && !tabs.contains(&detail.tab)
        {
            detail.tab = DeviceDetailTab::Overview;
        }
        let cursor = self.detail.as_ref().map_or(0, Detail::cursor_index);
        self.set_detail_cursor(cursor);
    }

    pub(super) fn apply_port_profiles(&mut self, device: &EntityId, profiles: Profiles) {
        if let Some(detail) = &mut self.detail
            && detail.device_id == *device
        {
            detail.profiles = profiles;
        }
    }
}
