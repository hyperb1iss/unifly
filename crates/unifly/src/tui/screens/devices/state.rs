use std::sync::Arc;

use super::DevicesScreen;
use super::detail::{Detail, Profiles};
use super::view::{DeviceFilter, DeviceSort};

use ratatui::widgets::TableState;
use unifly_api::{Device, EntityId};

use crate::cli::commands::util::order;
use crate::tui::action::{Action, DeviceDetailTab, Notification};
use crate::tui::table::{TableScroll, reselect};

impl DevicesScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            devices: Arc::new(Vec::new()),
            clients: Arc::new(Vec::new()),
            access: None,
            table_state: TableState::default(),
            table_scroll: TableScroll::default(),
            selected_device_id: None,
            filter: DeviceFilter::default(),
            sort: DeviceSort::default(),
            search_query: String::new(),
            detail: None,
            hits: std::cell::RefCell::default(),
            detail_hits: std::cell::RefCell::default(),
            renaming: None,
        }
    }

    /// Devices shown in the table: the ones passing the filter and the
    /// search query, in the chosen order.
    pub(super) fn filtered_devices(&self) -> Vec<&Arc<Device>> {
        let query = self.search_query.to_lowercase();
        let mut devices: Vec<&Arc<Device>> = self
            .devices
            .iter()
            .filter(|device| self.filter.matches(device))
            .filter(|device| query.is_empty() || matches_query(device, &query))
            .collect();
        self.sort.apply(&mut devices);
        devices
    }

    pub(super) fn selected_row_index(&self, filtered: &[&Arc<Device>]) -> Option<usize> {
        reselect(
            filtered.iter().map(|device| &device.id),
            self.selected_device_id.as_ref(),
            self.table_state.selected(),
        )
    }

    pub(super) fn selected_device(&self) -> Option<&Arc<Device>> {
        let filtered = self.filtered_devices();
        let index = self.selected_row_index(&filtered)?;
        filtered.get(index).copied()
    }

    /// The device a command key acts on: the open detail's, else the
    /// selected row's.
    pub(super) fn action_device(&self) -> Option<&Arc<Device>> {
        if self.detail.is_some() {
            self.detail_device()
        } else {
            self.selected_device()
        }
    }

    pub(super) fn select(&mut self, idx: usize) {
        let next = {
            let filtered = self.filtered_devices();
            let clamped = idx.min(filtered.len().saturating_sub(1));
            filtered
                .get(clamped)
                .map(|device| (clamped, device.id.clone()))
        };
        self.set_selection(next);
    }

    fn set_selection(&mut self, selection: Option<(usize, EntityId)>) {
        let (index, id) = selection.unzip();
        self.table_state.select(index);
        self.selected_device_id = id;
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        let current = {
            let filtered = self.filtered_devices();
            self.selected_row_index(&filtered).unwrap_or(0)
        };
        self.select(current.saturating_add_signed(delta));
    }

    pub(super) fn select_last(&mut self) {
        self.select(usize::MAX);
    }

    pub(super) fn apply_devices_update(&mut self, devices: &[Arc<Device>]) {
        self.devices = Arc::new(order::devices(devices));
        self.sync_selection(false);
        self.reconcile_detail();
    }

    pub(super) fn set_search_query(&mut self, query: &str) {
        self.search_query.clear();
        self.search_query.push_str(query);
        self.sync_selection(true);
    }

    pub(super) fn set_filter(&mut self, filter: DeviceFilter) {
        self.filter = filter;
        self.sync_selection(true);
    }

    pub(super) fn set_sort(&mut self, sort: DeviceSort) {
        self.sort = sort;
        self.sync_selection(true);
    }

    /// Re-point the cursor after the rows changed. An open detail keeps
    /// the cursor on its device and closes when the rows no longer hold
    /// it, so the pane never describes a device the table hides.
    /// Otherwise the cursor follows the selected device; when that one is
    /// gone it goes to the top after a view change (`reset_to_top`) and
    /// holds its row after a refresh.
    fn sync_selection(&mut self, reset_to_top: bool) {
        let (next, detail_hidden) = {
            let filtered = self.filtered_devices();
            let ids = || filtered.iter().map(|device| &device.id);
            let detail_row = self.detail.as_ref().map(|detail| {
                filtered
                    .iter()
                    .position(|device| device.id == detail.device_id)
            });
            let previous = if reset_to_top {
                Some(0)
            } else {
                self.table_state.selected()
            };
            let index = detail_row
                .flatten()
                .or_else(|| reselect(ids(), self.selected_device_id.as_ref(), previous));
            (
                index.map(|index| (index, filtered[index].id.clone())),
                detail_row == Some(None),
            )
        };
        if detail_hidden {
            self.close_detail();
        }
        self.set_selection(next);
    }

    /// Jump to `id`: drop a filter or search that hides it, select it, and
    /// open its detail. Says so when the device is not on the site.
    pub(super) fn show_device(&mut self, id: &EntityId) -> Option<Action> {
        if !self.devices.iter().any(|device| device.id == *id) {
            return Some(Action::Notify(Notification::warning(
                "That device is no longer on the controller",
            )));
        }
        let listed = |screen: &Self| {
            screen
                .filtered_devices()
                .iter()
                .any(|device| device.id == *id)
        };
        if !listed(self) {
            self.search_query.clear();
        }
        if !listed(self) {
            self.filter = DeviceFilter::All;
        }
        self.selected_device_id = Some(id.clone());
        self.detail = Some(Detail::new(id.clone(), DeviceDetailTab::Overview));
        self.sync_selection(false);
        None
    }

    /// Forget everything the previous controller reported. The sort
    /// stays: it is how the user likes the table, not data.
    pub(super) fn reset(&mut self) {
        *self = Self {
            focused: self.focused,
            sort: self.sort,
            ..Self::new()
        };
    }

    pub(super) fn apply_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::DevicesUpdated(devices) => self.apply_devices_update(devices),
            Action::ClientsUpdated(clients) => {
                self.clients = Arc::clone(clients);
                self.reconcile_detail();
            }
            Action::AccessUpdated(access) => self.access = Some(*access),
            Action::ControllerChanged => self.reset(),
            Action::ShowDevice(id) => return self.show_device(id),
            Action::CloseDetail => self.close_detail(),
            Action::SearchInput(query) => self.set_search_query(query),
            Action::CloseSearch => self.set_search_query(""),
            Action::PortProfilesLoaded { device, ports } => {
                self.apply_port_profiles(device, Profiles::Loaded(Arc::clone(ports)));
            }
            Action::PortProfilesFailed { device, error } => {
                self.apply_port_profiles(device, Profiles::Unavailable(error.clone()));
            }
            Action::PromptSubmitted(text) => return self.submit_prompt(text),
            _ => {}
        }
        None
    }
}

/// Whether `device` matches a lowercased search query by name, model, IP,
/// or MAC.
fn matches_query(device: &Device, query: &str) -> bool {
    let holds = |text: Option<&str>| text.is_some_and(|text| text.to_lowercase().contains(query));
    holds(device.name.as_deref())
        || holds(device.model.as_deref())
        || device.ip.is_some_and(|ip| ip.to_string().contains(query))
        || device.mac.to_string().to_lowercase().contains(query)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crossterm::event::{KeyCode, KeyEvent};
    use serde_json::json;
    use unifly_api::model::PoeMode;
    use unifly_api::{Device, EntityId};

    use super::DevicesScreen;
    use crate::tui::action::{Action, DeviceDetailTab, NotificationLevel, PortChange};
    use crate::tui::component::Component;
    use crate::tui::screens::devices::fixtures::{client, device, switch};
    use crate::tui::screens::devices::view::{DeviceFilter, SortKey};

    fn screen(devices: &[Arc<Device>]) -> DevicesScreen {
        let mut screen = DevicesScreen::new();
        screen.apply_devices_update(devices);
        screen
    }

    /// Two access points and a switch, in name order: alpha, bravo, core.
    fn fleet() -> DevicesScreen {
        screen(&[
            switch(3, "core", 24),
            device(1, "alpha", &json!({ "client_count": 4 })),
            device(
                2,
                "bravo",
                &json!({ "client_count": 9, "firmware_updatable": true }),
            ),
        ])
    }

    fn press(screen: &mut DevicesScreen, key: char) -> Option<Action> {
        screen.handle_key_input(KeyEvent::from(KeyCode::Char(key)))
    }

    fn enter(screen: &mut DevicesScreen) {
        screen.handle_key_input(KeyEvent::from(KeyCode::Enter));
    }

    fn names(screen: &DevicesScreen) -> Vec<String> {
        screen
            .filtered_devices()
            .into_iter()
            .map(|device| device.name.clone().unwrap_or_default())
            .collect()
    }

    fn selected(screen: &DevicesScreen) -> Option<String> {
        screen
            .selected_device()
            .and_then(|device| device.name.clone())
    }

    fn shown(screen: &DevicesScreen) -> Option<String> {
        screen
            .detail_device()
            .and_then(|device| device.name.clone())
    }

    fn tab(screen: &DevicesScreen) -> Option<DeviceDetailTab> {
        screen.detail.as_ref().map(|detail| detail.tab)
    }

    /// The toast an action carries, with its level.
    fn toast(action: Option<Action>) -> (NotificationLevel, String) {
        match action {
            Some(Action::Notify(notification)) => (notification.level, notification.message),
            other => panic!("expected a toast, got {other:?}"),
        }
    }

    #[test]
    fn filtered_devices_are_sorted_by_name() {
        let screen = screen(&[
            device(2, "Zulu AP", &json!({})),
            device(1, "alpha AP", &json!({})),
        ]);
        assert_eq!(names(&screen), ["alpha AP", "Zulu AP"]);
    }

    #[test]
    fn selection_tracks_device_when_sort_position_changes() {
        let mut screen = screen(&[
            device(1, "Alpha AP", &json!({})),
            device(2, "Zulu AP", &json!({})),
        ]);
        screen.select(1);

        screen.apply_devices_update(&[
            device(1, "Zulu AP", &json!({})),
            device(2, "Aaron AP", &json!({})),
        ]);

        assert_eq!(selected(&screen).as_deref(), Some("Aaron AP"));
        assert_eq!(screen.table_state.selected(), Some(0));
    }

    #[test]
    fn the_cursor_holds_its_row_when_the_selected_device_disappears() {
        let mut screen = fleet();
        screen.select(1);

        screen.apply_devices_update(&[device(1, "alpha", &json!({})), switch(3, "core", 24)]);

        assert_eq!(
            selected(&screen).as_deref(),
            Some("core"),
            "the cursor jumped away from row 1"
        );
    }

    #[test]
    fn a_search_that_hides_the_open_device_closes_its_detail() {
        let mut screen = fleet();
        screen.select(1);
        enter(&mut screen);
        assert_eq!(shown(&screen).as_deref(), Some("bravo"));

        screen.set_search_query("alp");

        assert!(
            screen.detail.is_none(),
            "the pane describes a hidden device"
        );
        assert_eq!(selected(&screen).as_deref(), Some("alpha"));

        // A search that still lists the device keeps its detail.
        screen.set_search_query("");
        screen.select(1);
        enter(&mut screen);
        screen.set_search_query("bra");
        assert_eq!(shown(&screen).as_deref(), Some("bravo"));
        assert_eq!(selected(&screen).as_deref(), Some("bravo"));
    }

    #[test]
    fn s_cycles_the_sort_and_r_reverses_it_keeping_the_selected_device() {
        let mut screen = fleet();
        screen.select(1);
        assert_eq!(screen.sort.label(), "name \u{2191}");

        press(&mut screen, 'r');
        assert_eq!(names(&screen), ["core", "bravo", "alpha"]);
        assert_eq!(selected(&screen).as_deref(), Some("bravo"));

        press(&mut screen, 's');
        press(&mut screen, 's');
        assert_eq!(screen.sort.key, SortKey::Clients);
        assert_eq!(screen.sort.label(), "clients \u{2193}");
        // The switch reports no count, so it goes last either way.
        assert_eq!(names(&screen), ["bravo", "alpha", "core"]);
        press(&mut screen, 'r');
        assert_eq!(names(&screen), ["alpha", "bravo", "core"]);
        assert_eq!(selected(&screen).as_deref(), Some("bravo"));
    }

    #[test]
    fn sorting_by_status_puts_trouble_first() {
        let mut screen = screen(&[
            device(1, "alpha", &json!({})),
            device(2, "bravo", &json!({ "state": "Offline" })),
            device(3, "charlie", &json!({ "state": "Updating" })),
        ]);
        press(&mut screen, 's');
        assert_eq!(screen.sort.key, SortKey::Status);
        assert_eq!(names(&screen), ["bravo", "charlie", "alpha"]);
    }

    #[test]
    fn h_and_l_step_the_filter_and_the_table_narrows() {
        let mut screen = fleet();
        press(&mut screen, 'l');
        assert_eq!(screen.filter, DeviceFilter::Gateways);
        assert_eq!(names(&screen), Vec::<String>::new());
        press(&mut screen, 'l');
        assert_eq!(names(&screen), ["core"]);
        screen.handle_key_input(KeyEvent::from(KeyCode::Left));
        screen.handle_key_input(KeyEvent::from(KeyCode::Left));
        screen.handle_key_input(KeyEvent::from(KeyCode::Left));
        assert_eq!(screen.filter, DeviceFilter::Updates, "the bar wraps");
        assert_eq!(names(&screen), ["bravo"]);
        assert!(
            screen
                .handle_key_input(KeyEvent::from(KeyCode::Tab))
                .is_none(),
            "Tab switches screens everywhere"
        );
    }

    #[test]
    fn show_device_drops_what_hides_it_and_opens_its_detail() {
        let mut screen = fleet();
        screen.set_filter(DeviceFilter::Updates);
        screen.set_search_query("bra");

        let follow_up = screen.apply_action(&Action::ShowDevice(EntityId::from("device-3")));

        assert!(follow_up.is_none());
        assert_eq!(shown(&screen).as_deref(), Some("core"));
        assert_eq!(selected(&screen).as_deref(), Some("core"));
        assert_eq!(screen.filter, DeviceFilter::All);
        assert_eq!(screen.search_query, "");

        // A device the filter already lists keeps the view as it is.
        screen.close_detail();
        screen.set_filter(DeviceFilter::Switches);
        screen.apply_action(&Action::ShowDevice(EntityId::from("device-3")));
        assert_eq!(screen.filter, DeviceFilter::Switches);
        assert_eq!(shown(&screen).as_deref(), Some("core"));

        let gone = screen.apply_action(&Action::ShowDevice(EntityId::from("device-9")));
        let (level, message) = toast(gone);
        assert_eq!(level, NotificationLevel::Warning);
        assert!(message.contains("no longer on the controller"), "{message}");
    }

    #[test]
    fn a_controller_change_drops_everything_but_the_sort() {
        let mut screen = fleet();
        press(&mut screen, 'r');
        screen.set_search_query("a");
        enter(&mut screen);
        screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![client(
            1,
            "laptop",
            &json!({}),
        )])));

        screen.apply_action(&Action::ControllerChanged);

        assert!(screen.devices.is_empty() && screen.clients.is_empty());
        assert!(screen.detail.is_none() && screen.selected_device_id.is_none());
        assert!(screen.access.is_none());
        assert_eq!(screen.search_query, "");
        assert!(screen.sort.descending, "the sort is the user's, not data");
    }

    #[test]
    fn tabs_follow_what_the_device_has() {
        use crate::tui::screens::devices::detail::tabs_for;

        let ap = device(1, "ap", &json!({}));
        assert!(tabs_for(&ap).contains(&DeviceDetailTab::Radios));
        assert!(!tabs_for(&ap).contains(&DeviceDetailTab::Ports));
        let switch = switch(2, "sw", 8);
        assert!(!tabs_for(&switch).contains(&DeviceDetailTab::Radios));
        assert!(tabs_for(&switch).contains(&DeviceDetailTab::Ports));
    }

    #[test]
    fn in_a_detail_movement_steps_devices_except_on_the_list_tabs() {
        let mut screen = fleet();
        enter(&mut screen);
        assert_eq!(shown(&screen).as_deref(), Some("alpha"));

        // Overview holds no list: j moves to the next device.
        press(&mut screen, 'j');
        assert_eq!(shown(&screen).as_deref(), Some("bravo"));
        press(&mut screen, 'G');
        assert_eq!(shown(&screen).as_deref(), Some("core"));
        assert_eq!(tab(&screen), Some(DeviceDetailTab::Overview));

        // h wraps to the switch's last tab, Ports, which asks for the
        // port profiles once.
        let load = press(&mut screen, 'h');
        assert_eq!(tab(&screen), Some(DeviceDetailTab::Ports));
        assert!(
            matches!(load, Some(Action::RequestPortProfiles(ref id)) if *id == EntityId::from("device-3")),
            "{load:?}"
        );

        // On Ports the same keys drive the port list and the device stays.
        for _ in 0..5 {
            press(&mut screen, 'j');
        }
        assert_eq!(shown(&screen).as_deref(), Some("core"));
        assert_eq!(screen.cursor_port().map(|(_, port)| port.index), Some(6));
        press(&mut screen, 'G');
        assert_eq!(screen.cursor_port().map(|(_, port)| port.index), Some(24));
        screen.handle_key_input(KeyEvent::new(
            KeyCode::Char('u'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        assert_eq!(screen.cursor_port().map(|(_, port)| port.index), Some(14));
        press(&mut screen, 'g');
        assert_eq!(screen.cursor_port().map(|(_, port)| port.index), Some(1));

        // Esc closes the detail and is used up, so it does not also go back.
        let closed = screen.handle_key_input(KeyEvent::from(KeyCode::Esc));
        assert!(matches!(closed, Some(Action::CloseDetail)));
        assert!(screen.detail.is_none());
        assert!(
            screen
                .handle_key_input(KeyEvent::from(KeyCode::Esc))
                .is_none(),
            "with nothing open, Esc is the app's"
        );
    }

    #[test]
    fn stepping_to_a_device_without_the_open_tab_falls_back_to_overview() {
        let mut screen = fleet();
        screen.select(2);
        enter(&mut screen);
        press(&mut screen, 'h');
        assert_eq!(tab(&screen), Some(DeviceDetailTab::Ports));

        // A tap on another row retargets the detail; an access point has
        // no Ports tab.
        screen.select(0);
        screen.retarget_detail();

        assert_eq!(shown(&screen).as_deref(), Some("alpha"));
        assert_eq!(tab(&screen), Some(DeviceDetailTab::Overview));
    }

    #[test]
    fn enter_on_the_clients_tab_opens_the_client_under_the_cursor() {
        let mut screen = fleet();
        screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![
            client(
                1,
                "laptop",
                &json!({ "uplink_device_mac": "aa:bb:cc:dd:ee:03" }),
            ),
            client(2, "nas", &json!({ "uplink_device_id": "device-3" })),
            client(
                3,
                "elsewhere",
                &json!({ "uplink_device_mac": "aa:bb:cc:dd:ee:01" }),
            ),
        ])));
        screen.select(2);
        enter(&mut screen);
        screen.set_detail_tab(DeviceDetailTab::Clients);
        assert_eq!(
            screen.detail_list_len(),
            2,
            "by uplink MAC and by uplink ID"
        );

        press(&mut screen, 'j');
        let open = screen.handle_key_input(KeyEvent::from(KeyCode::Enter));

        assert!(
            matches!(open, Some(Action::ShowClient(ref id)) if *id == EntityId::from("client-2")),
            "{open:?}"
        );
    }

    #[test]
    fn commands_are_refused_with_a_reason_when_the_state_rules_them_out() {
        let mut screen = screen(&[
            device(1, "alpha", &json!({})),
            device(
                2,
                "bravo",
                &json!({ "state": "Offline", "firmware_updatable": true }),
            ),
            device(3, "charlie", &json!({ "state": "PendingAdoption" })),
        ]);

        // Up to date: no upgrade dialog.
        let (level, message) = toast(press(&mut screen, 'U'));
        assert_eq!(level, NotificationLevel::Info);
        assert!(message.contains("no firmware update waiting"), "{message}");
        assert!(matches!(
            press(&mut screen, 'R'),
            Some(Action::RequestRestart(_))
        ));
        let (_, message) = toast(press(&mut screen, 'A'));
        assert!(message.contains("already adopted"), "{message}");
        let (_, message) = toast(press(&mut screen, 'T'));
        assert!(message.contains("not a gateway"), "{message}");

        // Offline: nothing can reach it.
        screen.select(1);
        for key in ['R', 'L', 'U', 'P'] {
            let (_, message) = toast(press(&mut screen, key));
            assert!(message.contains("bravo is offline"), "{key}: {message}");
        }

        // Pending: only adoption applies.
        screen.select(2);
        let (_, message) = toast(press(&mut screen, 'R'));
        assert!(message.contains("not adopted yet"), "{message}");
        assert!(
            matches!(press(&mut screen, 'A'), Some(Action::RequestAdoptDevice(ref id)) if *id == EntityId::from("device-3"))
        );
    }

    #[test]
    fn an_updatable_device_upgrades_and_a_gateway_runs_a_speed_test() {
        let mut screen = screen(&[
            device(1, "ap", &json!({ "firmware_updatable": true })),
            device(2, "gw", &json!({ "device_type": "Gateway" })),
        ]);
        assert!(matches!(
            press(&mut screen, 'U'),
            Some(Action::RequestUpgrade(_))
        ));
        assert!(matches!(
            press(&mut screen, 'P'),
            Some(Action::RequestProvision(_))
        ));

        screen.select(1);
        let Some(Action::ShowConfirm(confirm)) = press(&mut screen, 'T') else {
            panic!("the speed test asks first");
        };
        assert_eq!(confirm.prompt, "Run a speed test on gw?");
        assert!(matches!(
            *confirm.action,
            Action::Run { ref command, .. } if matches!(**command, unifly_api::Command::SpeedtestDevice)
        ));
    }

    #[test]
    fn forgetting_a_device_is_only_offered_from_its_detail() {
        let mut screen = fleet();
        assert!(press(&mut screen, 'F').is_none(), "F acted from the table");
        enter(&mut screen);
        assert!(
            matches!(press(&mut screen, 'F'), Some(Action::RequestUnadopt(ref id)) if *id == EntityId::from("device-1"))
        );
    }

    /// The fleet with the switch's Ports tab open, cursor on `port`.
    fn on_port(port: usize) -> DevicesScreen {
        let mut screen = fleet();
        screen.select(2);
        enter(&mut screen);
        screen.set_detail_tab(DeviceDetailTab::Ports);
        screen.set_detail_cursor(port - 1);
        screen
    }

    #[test]
    fn a_power_cycle_needs_a_port_that_supplies_poe() {
        let mut screen = on_port(2);
        assert!(matches!(
            press(&mut screen, 'C'),
            Some(Action::RequestPortPowerCycle(ref id, 2)) if *id == EntityId::from("device-3")
        ));

        // Port 6 has PoE switched off, port 12 has none.
        for port in [6, 12] {
            let mut screen = on_port(port);
            let (level, message) = toast(press(&mut screen, 'C'));
            assert_eq!(level, NotificationLevel::Info);
            assert_eq!(message, format!("Port {port} is not supplying PoE"));
        }
    }

    #[test]
    fn poe_turns_on_at_once_and_asks_before_turning_off() {
        let mut screen = on_port(6);
        assert!(matches!(
            press(&mut screen, 'O'),
            Some(Action::RequestPortUpdate {
                port: 6,
                change: PortChange::Poe(PoeMode::Auto),
                ..
            })
        ));

        let mut screen = on_port(1);
        let Some(Action::ShowConfirm(confirm)) = press(&mut screen, 'O') else {
            panic!("turning PoE off asks first");
        };
        assert_eq!(confirm.prompt, "Turn PoE off on port 1 of core?");
        assert!(matches!(
            *confirm.action,
            Action::RequestPortUpdate {
                port: 1,
                change: PortChange::Poe(PoeMode::Off),
                ..
            }
        ));

        let mut screen = on_port(12);
        let (_, message) = toast(press(&mut screen, 'O'));
        assert_eq!(message, "Port 12 has no PoE");
    }

    #[test]
    fn renaming_a_port_prompts_with_its_name_and_sends_the_answer() {
        let mut screen = on_port(3);
        let Some(Action::OpenPrompt(prompt)) = press(&mut screen, 'N') else {
            panic!("rename opens a prompt");
        };
        assert_eq!(prompt.title, "Rename port 3 on core");
        assert_eq!(prompt.initial, "Port 3");

        let sent = screen.apply_action(&Action::PromptSubmitted("cam-dock".into()));
        assert!(
            matches!(
                sent,
                Some(Action::RequestPortUpdate { port: 3, change: PortChange::Rename(ref name), .. })
                    if name == "cam-dock"
            ),
            "{sent:?}"
        );
        assert!(
            screen
                .apply_action(&Action::PromptSubmitted("stray".into()))
                .is_none(),
            "an answer nobody asked for did something"
        );

        // A name the controller would cut is handed back to fix.
        press(&mut screen, 'N');
        let long = "x".repeat(65);
        let again = screen.apply_action(&Action::PromptSubmitted(long.clone()));
        let Some(Action::OpenPrompt(prompt)) = again else {
            panic!("a long name reopens the prompt, got {again:?}");
        };
        assert_eq!(prompt.initial, long);
        assert!(prompt.hint.is_some_and(|hint| hint.contains("too long")));
    }

    #[test]
    fn port_profiles_land_on_the_device_they_were_asked_for() {
        use crate::tui::screens::devices::detail::Profiles;

        let mut screen = on_port(1);
        let stale = EntityId::from("device-1");
        screen.apply_action(&Action::PortProfilesFailed {
            device: stale,
            error: "late answer".into(),
        });
        assert!(
            !matches!(
                screen.detail.as_ref().map(|detail| &detail.profiles),
                Some(Profiles::Unavailable(_))
            ),
            "another device's answer was shown"
        );

        screen.apply_action(&Action::PortProfilesFailed {
            device: EntityId::from("device-3"),
            error: "needs a session".into(),
        });
        assert!(matches!(
            screen.detail.as_ref().map(|detail| &detail.profiles),
            Some(Profiles::Unavailable(reason)) if reason == "needs a session"
        ));
    }

    #[test]
    fn help_names_what_movement_does_in_each_mode() {
        let mut screen = fleet();
        let describes = |screen: &DevicesScreen, text: &str| {
            screen.help().iter().any(|(_, what)| *what == text)
        };
        assert!(describes(&screen, "Open the device"));
        assert!(describes(&screen, "Sort by the next column"));
        assert!(!describes(&screen, "Close the detail"));

        screen.select(2);
        enter(&mut screen);
        assert!(describes(&screen, "Previous / next device"));
        assert!(!describes(&screen, "Open the device"));
        assert!(!describes(&screen, "Sort by the next column"));

        screen.set_detail_tab(DeviceDetailTab::Ports);
        assert!(describes(&screen, "Move through the ports"));
        assert!(describes(&screen, "Power cycle the PoE port"));
        assert!(!describes(&screen, "Previous / next device"));

        screen.set_detail_tab(DeviceDetailTab::Clients);
        assert!(describes(&screen, "Open the client"));

        // The overlay gives a description 24 columns before it clips.
        for tab in DeviceDetailTab::ALL {
            screen.set_detail_tab(tab);
            for (key, what) in screen.help() {
                assert!(what.chars().count() <= 24, "{key}: {what:?} is clipped");
            }
        }
    }
}
