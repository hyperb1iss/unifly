use std::sync::Arc;

use super::ClientsScreen;
use super::view::{ClientSort, filter_matches};

use ratatui::widgets::TableState;
use unifly_api::{Client, EntityId};

use crate::cli::commands::util::order;
use crate::tui::action::{Action, ClientTypeFilter, Notification};
use crate::tui::screens::devices::view::wrapped;
use crate::tui::table::{TableScroll, reselect};

/// Where a list the screen asks the controller for stands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) enum Fetch {
    /// Nothing asked for yet.
    #[default]
    Unasked,
    Loading,
    Loaded,
    /// Why the list cannot be shown.
    Unavailable(String),
}

impl ClientsScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            clients: Arc::new(Vec::new()),
            links: super::labels::LinkIndex::default(),
            access: None,
            table_state: TableState::default(),
            table_scroll: TableScroll::default(),
            filter: ClientTypeFilter::All,
            sort: ClientSort::default(),
            hits: std::cell::RefCell::default(),
            search_query: String::new(),
            detail_open: false,
            detail_client_id: None,
            asking: None,
            offline_blocked: Arc::new(Vec::new()),
            offline_blocked_status: Fetch::default(),
            blocked_rows: Vec::new(),
        }
    }

    /// Clients shown in the table: the ones passing the type filter and
    /// the search query, in the chosen order. The Blocked filter also
    /// lists the blocked clients that are off the network.
    pub(super) fn filtered_clients(&self) -> Vec<&Arc<Client>> {
        let query = self.search_query.to_lowercase();
        let rows: &[Arc<Client>] = if self.filter == ClientTypeFilter::Blocked {
            &self.blocked_rows
        } else {
            &self.clients
        };
        let mut clients: Vec<&Arc<Client>> = rows
            .iter()
            .filter(|client| filter_matches(self.filter, client))
            .filter(|client| query.is_empty() || matches_query(client, &query))
            .collect();
        self.sort.apply(&mut clients);
        clients
    }

    pub(super) fn selected_index(&self) -> usize {
        self.table_state.selected().unwrap_or(0)
    }

    pub(super) fn detail_client_index(&self, filtered: &[&Arc<Client>]) -> Option<usize> {
        let id = self.detail_client_id.as_ref()?;
        filtered.iter().position(|client| client.id == *id)
    }

    pub(super) fn detail_client<'a>(
        &self,
        filtered: &'a [&Arc<Client>],
    ) -> Option<&'a Arc<Client>> {
        let idx = self.detail_client_index(filtered)?;
        filtered.get(idx).copied()
    }

    pub(super) fn select(&mut self, idx: usize) {
        let last = self.filtered_clients().len().saturating_sub(1);
        self.table_state.select(Some(idx.min(last)));
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        self.select(self.selected_index().saturating_add_signed(delta));
    }

    /// The filter `step` places along the filter bar, wrapping.
    pub(super) fn filter_after(&self, step: isize) -> ClientTypeFilter {
        let all = ClientTypeFilter::ALL;
        all[wrapped(self.filter_index(), step, all.len())]
    }

    /// Show the clients `filter` lets through. An open detail closes when
    /// its client is no longer among them. The Blocked filter asks the
    /// controller for the blocked clients that are off the network.
    pub(super) fn set_filter(&mut self, filter: ClientTypeFilter) -> Option<Action> {
        let selected = self.selected_client_id();
        self.filter = filter;
        self.reconcile_selection(selected.as_ref(), true);
        self.request_offline_blocked()
    }

    /// Ask for the offline blocked clients while the Blocked filter
    /// shows, one request at a time. The rows on screen stay until the
    /// answer replaces them.
    fn request_offline_blocked(&mut self) -> Option<Action> {
        if self.filter != ClientTypeFilter::Blocked || self.offline_blocked_status == Fetch::Loading
        {
            return None;
        }
        self.offline_blocked_status = Fetch::Loading;
        Some(Action::RequestBlockedClients)
    }

    /// Whether `client` is a blocked client that is off the network: it
    /// can be unblocked or forgotten, and nothing else.
    pub(super) fn is_offline(&self, client: &Client) -> bool {
        self.offline_blocked
            .iter()
            .any(|offline| offline.id == client.id)
    }

    /// Rebuild the Blocked filter's rows from the connected clients and
    /// the offline ones, by name like the rest of the table.
    fn rebuild_blocked_rows(&mut self) {
        let connected = self.clients.iter().filter(|client| client.blocked);
        let offline = self
            .offline_blocked
            .iter()
            .filter(|offline| !self.clients.iter().any(|client| client.id == offline.id));
        let rows: Vec<Arc<Client>> = connected.chain(offline).cloned().collect();
        self.blocked_rows = order::clients(&rows);
    }

    fn apply_offline_blocked(&mut self, offline: Arc<Vec<Arc<Client>>>, status: Fetch) {
        let selected = self.selected_client_id();
        self.offline_blocked = offline;
        self.offline_blocked_status = status;
        self.rebuild_blocked_rows();
        self.reconcile_selection(selected.as_ref(), false);
    }

    pub(super) fn set_sort(&mut self, sort: ClientSort) {
        let selected = self.selected_client_id();
        self.sort = sort;
        self.reconcile_selection(selected.as_ref(), true);
    }

    pub(super) fn set_search_query(&mut self, query: &str) {
        let selected = self.selected_client_id();
        self.search_query.clear();
        self.search_query.push_str(query);
        self.reconcile_selection(selected.as_ref(), true);
    }

    /// Store a refreshed client list in table order, keeping the selection
    /// on the client it was on. A block or an unblock shows up as a
    /// client leaving or joining, so the offline blocked ones are asked
    /// for again while their filter shows.
    pub(super) fn apply_clients_update(&mut self, clients: &[Arc<Client>]) -> Option<Action> {
        let selected = self.selected_client_id();
        self.clients = Arc::new(order::clients(clients));
        self.rebuild_blocked_rows();
        self.reconcile_selection(selected.as_ref(), false);
        self.request_offline_blocked()
    }

    /// Re-point the cursor after the rows changed. An open detail keeps
    /// the cursor on its client and closes when the rows no longer hold
    /// it, so the pane never describes a client the table hides.
    /// Otherwise the cursor follows `follow`; when that client is gone it
    /// goes to the top after a view change (`reset_to_top`) and holds its
    /// row after a refresh.
    fn reconcile_selection(&mut self, follow: Option<&EntityId>, reset_to_top: bool) {
        let (detail_idx, followed) = {
            let filtered = self.filtered_clients();
            let detail_idx = if self.detail_open {
                self.detail_client_index(&filtered)
            } else {
                None
            };
            let previous = if reset_to_top {
                Some(0)
            } else {
                self.table_state.selected()
            };
            let followed = reselect(filtered.iter().map(|client| &client.id), follow, previous);
            (detail_idx, followed)
        };

        if self.detail_open {
            if let Some(index) = detail_idx {
                self.table_state.select(Some(index));
                return;
            }
            self.close_detail();
        }

        self.table_state.select(Some(followed.unwrap_or(0)));
    }

    pub(super) fn filter_index(&self) -> usize {
        ClientTypeFilter::ALL
            .iter()
            .position(|filter| *filter == self.filter)
            .unwrap_or_default()
    }

    pub(super) fn selected_client(&self) -> Option<&Arc<Client>> {
        self.filtered_clients().get(self.selected_index()).copied()
    }

    pub(super) fn selected_client_id(&self) -> Option<EntityId> {
        self.selected_client().map(|client| client.id.clone())
    }

    /// The client a command key acts on: the open detail's, else the
    /// selected row's.
    pub(super) fn action_client(&self) -> Option<&Arc<Client>> {
        if self.detail_open {
            let id = self.detail_client_id.as_ref()?;
            return self
                .clients
                .iter()
                .chain(self.offline_blocked.iter())
                .find(|client| client.id == *id);
        }
        self.selected_client()
    }

    pub(super) fn open_selected_detail(&mut self) {
        if let Some(id) = self.selected_client_id() {
            self.detail_open = true;
            self.detail_client_id = Some(id);
        }
    }

    pub(super) fn close_detail(&mut self) {
        self.detail_open = false;
        self.detail_client_id = None;
    }

    /// Jump to `id`: drop a search or filter that hides it, select it, and
    /// open its detail. Says so when the client is not connected.
    pub(super) fn show_client(&mut self, id: &EntityId) -> Option<Action> {
        if !self.clients.iter().any(|client| client.id == *id) {
            return Some(Action::Notify(Notification::warning(
                "That client is no longer connected",
            )));
        }
        let listed = |screen: &Self| {
            screen
                .filtered_clients()
                .iter()
                .any(|client| client.id == *id)
        };
        if !listed(self) {
            self.search_query.clear();
        }
        if !listed(self) {
            self.filter = ClientTypeFilter::All;
        }
        self.detail_open = true;
        self.detail_client_id = Some(id.clone());
        self.reconcile_selection(Some(id), false);
        None
    }

    /// Forget everything the previous controller reported. The sort
    /// stays: it is how the user likes the table, not data.
    fn reset(&mut self) {
        *self = Self {
            focused: self.focused,
            sort: self.sort,
            ..Self::new()
        };
    }

    pub(super) fn apply_action(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::ClientsUpdated(clients) => return self.apply_clients_update(clients),
            Action::BlockedClientsLoaded(offline) => {
                self.apply_offline_blocked(Arc::new(order::clients(offline)), Fetch::Loaded);
            }
            Action::BlockedClientsFailed(reason) => {
                self.apply_offline_blocked(
                    Arc::new(Vec::new()),
                    Fetch::Unavailable(reason.clone()),
                );
            }
            Action::DevicesUpdated(devices) => self.links.set_devices(devices),
            Action::NetworksUpdated(networks) => self.links.set_networks(networks),
            Action::AccessUpdated(access) => self.access = Some(*access),
            Action::ControllerChanged => self.reset(),
            Action::ShowClient(id) => return self.show_client(id),
            Action::SearchInput(query) => self.set_search_query(query),
            Action::CloseSearch => self.set_search_query(""),
            Action::CloseDetail => self.close_detail(),
            Action::PromptSubmitted(text) => return self.submit_prompt(text),
            _ => {}
        }
        None
    }
}

/// Whether `client` matches a lowercased search query by name, hostname,
/// any of its IP addresses, or MAC.
fn matches_query(client: &Client, query: &str) -> bool {
    let holds = |text: Option<&str>| text.is_some_and(|text| text.to_lowercase().contains(query));
    holds(client.name.as_deref())
        || holds(client.hostname.as_deref())
        || client.addresses().any(|ip| ip.to_string().contains(query))
        || client.mac.to_string().to_lowercase().contains(query)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::ClientsScreen;

    use crossterm::event::{KeyCode, KeyEvent};
    use serde_json::json;
    use unifly_api::{Client, Command, EntityId};

    use crate::tui::action::{Action, ClientTypeFilter, NotificationLevel};
    use crate::tui::component::Component;
    use crate::tui::screens::devices::fixtures::{client, wireless};

    fn screen(clients: Vec<Arc<Client>>) -> ClientsScreen {
        let mut screen = ClientsScreen::new();
        screen.apply_action(&Action::ClientsUpdated(Arc::new(clients)));
        screen
    }

    fn press(screen: &mut ClientsScreen, key: char) -> Option<Action> {
        screen.handle_key_input(KeyEvent::from(KeyCode::Char(key)))
    }

    fn enter(screen: &mut ClientsScreen) {
        screen.handle_key_input(KeyEvent::from(KeyCode::Enter));
    }

    fn names(screen: &ClientsScreen) -> Vec<String> {
        screen
            .filtered_clients()
            .iter()
            .map(|client| client.name.clone().unwrap_or_default())
            .collect()
    }

    fn id(index: u8) -> EntityId {
        EntityId::from(format!("client-{index}"))
    }

    /// The toast an action carries, with its level.
    fn toast(action: Option<Action>) -> (NotificationLevel, String) {
        match action {
            Some(Action::Notify(notification)) => (notification.level, notification.message),
            other => panic!("expected a toast, got {other:?}"),
        }
    }

    #[test]
    fn detail_actions_follow_client_identity_after_refresh() {
        let mut screen = screen(vec![
            client(1, "alpha", &json!({})),
            client(2, "bravo", &wireless(-60, "aa:bb:cc:dd:ee:01")),
        ]);
        screen.select(1);
        enter(&mut screen);
        assert_eq!(screen.detail_client_id, Some(id(2)));

        // alpha is renamed and now sorts after bravo.
        screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![
            client(1, "zulu", &json!({})),
            client(2, "bravo", &wireless(-60, "aa:bb:cc:dd:ee:01")),
        ])));

        assert_eq!(screen.detail_client_id, Some(id(2)));
        assert_eq!(screen.table_state.selected(), Some(0));
        assert!(matches!(
            press(&mut screen, 'x'),
            Some(Action::RequestKickClient(kicked)) if kicked == id(2)
        ));
    }

    #[test]
    fn clients_list_in_cli_order_and_selection_follows_the_client() {
        let mut screen = screen(vec![
            client(3, "charlie", &json!({})),
            client(1, "Alpha", &json!({})),
            client(2, "bravo", &json!({})),
        ]);
        assert_eq!(names(&screen), ["Alpha", "bravo", "charlie"]);

        screen.select(1);
        screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![
            client(2, "bravo", &json!({})),
            client(4, "aaron", &json!({})),
            client(3, "charlie", &json!({})),
            client(1, "Alpha", &json!({})),
        ])));

        assert_eq!(names(&screen), ["aaron", "Alpha", "bravo", "charlie"]);
        assert_eq!(screen.table_state.selected(), Some(2));
        assert_eq!(screen.selected_client_id(), Some(id(2)));
    }

    #[test]
    fn the_cursor_holds_its_row_when_the_selected_client_leaves() {
        let mut screen = screen(vec![
            client(1, "alpha", &json!({})),
            client(2, "bravo", &json!({})),
            client(3, "charlie", &json!({})),
        ]);
        screen.select(1);

        screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![
            client(1, "alpha", &json!({})),
            client(3, "charlie", &json!({})),
        ])));

        assert_eq!(screen.selected_client_id(), Some(id(3)));
    }

    #[test]
    fn detail_closes_when_target_falls_out_of_filter() {
        let mut screen = screen(vec![
            client(1, "alpha", &json!({})),
            client(2, "bravo", &json!({})),
        ]);
        screen.select(1);
        enter(&mut screen);

        screen.apply_action(&Action::SearchInput("alpha".to_owned()));

        assert!(!screen.detail_open);
        assert!(screen.detail_client_id.is_none());
        assert_eq!(screen.table_state.selected(), Some(0));
    }

    #[test]
    fn a_view_change_keeps_the_cursor_on_its_client_while_it_is_listed() {
        let mut screen = screen(vec![
            client(1, "alpha", &json!({})),
            client(2, "bravo", &wireless(-60, "aa:bb:cc:dd:ee:01")),
            client(3, "charlie", &wireless(-60, "aa:bb:cc:dd:ee:01")),
        ]);
        screen.select(2);

        screen.set_filter(ClientTypeFilter::Wireless);
        assert_eq!(screen.selected_client_id(), Some(id(3)));

        // charlie is not wired, so the cursor starts at the top.
        screen.set_filter(ClientTypeFilter::Wired);
        assert_eq!(screen.selected_client_id(), Some(id(1)));
    }

    #[test]
    fn b_blocks_or_unblocks_by_the_clients_state() {
        let mut screen = screen(vec![
            client(1, "alpha", &json!({})),
            client(2, "bravo", &json!({ "blocked": true })),
        ]);
        assert!(matches!(
            press(&mut screen, 'b'),
            Some(Action::RequestBlockClient(blocked)) if blocked == id(1)
        ));
        screen.select(1);
        assert!(matches!(
            press(&mut screen, 'b'),
            Some(Action::RequestUnblockClient(unblocked)) if unblocked == id(2)
        ));
        assert!(press(&mut screen, 'B').is_none(), "one key, two directions");
    }

    #[test]
    fn kick_refuses_a_client_that_is_not_on_wifi() {
        let mut screen = screen(vec![
            client(1, "alpha", &json!({})),
            client(2, "bravo", &wireless(-60, "aa:bb:cc:dd:ee:01")),
        ]);
        let (level, message) = toast(press(&mut screen, 'x'));
        assert_eq!(level, NotificationLevel::Info);
        assert_eq!(
            message,
            "alpha is not on WiFi; only wireless clients can be disconnected"
        );
        screen.select(1);
        assert!(matches!(
            press(&mut screen, 'x'),
            Some(Action::RequestKickClient(_))
        ));
        assert!(matches!(
            press(&mut screen, 'F'),
            Some(Action::RequestForgetClient(forgotten)) if forgotten == id(2)
        ));
    }

    #[test]
    fn a_guest_is_authorized_for_the_minutes_typed() {
        let mut screen = screen(vec![
            client(1, "alpha", &json!({})),
            client(2, "guest-phone", &json!({ "is_guest": true })),
        ]);
        let (_, message) = toast(press(&mut screen, 'A'));
        assert!(message.contains("alpha is not a guest"), "{message}");

        screen.select(1);
        let Some(Action::OpenPrompt(prompt)) = press(&mut screen, 'A') else {
            panic!("authorizing asks for minutes");
        };
        assert_eq!(prompt.title, "Authorize guest-phone");
        assert_eq!(prompt.initial, "60");

        // A bad value is handed back with the reason.
        let again = screen.apply_action(&Action::PromptSubmitted("soon".into()));
        let Some(Action::OpenPrompt(prompt)) = again else {
            panic!("a bad value reopens the prompt, got {again:?}");
        };
        assert_eq!(prompt.initial, "soon");
        assert!(
            prompt
                .hint
                .is_some_and(|hint| hint.contains("not a number"))
        );
        let again = screen.apply_action(&Action::PromptSubmitted("0".into()));
        assert!(matches!(again, Some(Action::OpenPrompt(_))), "{again:?}");

        let run = screen.apply_action(&Action::PromptSubmitted("90".into()));
        let Some(Action::Run { command, notice }) = run else {
            panic!("expected the command, got {run:?}");
        };
        assert!(matches!(
            *command,
            Command::AuthorizeGuest { ref client_id, time_limit_minutes: Some(90), .. }
                if *client_id == id(2)
        ));
        assert_eq!(notice.done, "Authorized guest-phone for 90 minutes");
        assert!(
            screen
                .apply_action(&Action::PromptSubmitted("15".into()))
                .is_none(),
            "an answer nobody asked for did something"
        );
    }

    #[test]
    fn an_authorized_guest_is_revoked_after_a_confirmation() {
        let mut screen = screen(vec![client(
            1,
            "guest-phone",
            &json!({
                "is_guest": true,
                "guest_auth": {
                    "authorized": true, "method": "voucher", "expires_at": null,
                    "tx_bytes": null, "rx_bytes": null, "elapsed_minutes": null
                }
            }),
        )]);
        let Some(Action::ShowConfirm(confirm)) = press(&mut screen, 'A') else {
            panic!("revoking asks first");
        };
        assert_eq!(confirm.prompt, "Revoke guest access for guest-phone?");
        assert!(matches!(
            *confirm.action,
            Action::Run { ref command, .. }
                if matches!(**command, Command::UnauthorizeGuest { ref client_id } if *client_id == id(1))
        ));
    }

    #[test]
    fn the_fixed_ip_prompt_starts_from_the_address_and_checks_the_answer() {
        let mut screen = screen(vec![
            client(1, "alpha", &json!({})),
            client(
                2,
                "nas-vault",
                &json!({ "use_fixedip": true, "fixed_ip": "10.40.20.250" }),
            ),
        ]);

        let Some(Action::OpenPrompt(prompt)) = press(&mut screen, 'I') else {
            panic!("the fixed IP key opens a prompt");
        };
        assert_eq!(prompt.title, "Fixed IP for alpha");
        assert_eq!(prompt.initial, "10.40.20.1", "the address it holds now");

        let again = screen.apply_action(&Action::PromptSubmitted("10.40.20".into()));
        let Some(Action::OpenPrompt(prompt)) = again else {
            panic!("a bad address reopens the prompt, got {again:?}");
        };
        assert_eq!(prompt.initial, "10.40.20");
        assert!(
            prompt
                .hint
                .is_some_and(|hint| hint.contains("not an IPv4 address"))
        );

        let set = screen.apply_action(&Action::PromptSubmitted("10.40.20.77".into()));
        assert!(
            matches!(set, Some(Action::RequestSetFixedIp { ref client, ip })
                if *client == id(1) && ip == std::net::Ipv4Addr::new(10, 40, 20, 77)),
            "{set:?}"
        );

        // Without a reservation, an empty answer changes nothing.
        press(&mut screen, 'I');
        let (_, message) = toast(screen.apply_action(&Action::PromptSubmitted(String::new())));
        assert!(message.contains("alpha keeps taking its address from DHCP"));

        // With one, the prompt starts from it and an empty answer removes it.
        screen.select(1);
        let Some(Action::OpenPrompt(prompt)) = press(&mut screen, 'I') else {
            panic!("the fixed IP key opens a prompt");
        };
        assert_eq!(prompt.initial, "10.40.20.250");
        let removed = screen.apply_action(&Action::PromptSubmitted(String::new()));
        assert!(
            matches!(removed, Some(Action::RequestRemoveFixedIp(ref client)) if *client == id(2)),
            "{removed:?}"
        );
    }

    fn access(session: bool) -> Action {
        Action::AccessUpdated(crate::tui::action::Access {
            integration: true,
            session,
            live_events: false,
        })
    }

    #[test]
    fn the_blocked_filter_lists_clients_that_are_off_the_network() {
        let mut screen = screen(vec![
            client(1, "alpha", &json!({})),
            client(2, "kiosk", &json!({ "blocked": true })),
        ]);

        // Stepping onto the filter asks for the offline blocked clients,
        // once: a second ask waits for the first answer.
        assert!(matches!(
            screen.set_filter(ClientTypeFilter::Blocked),
            Some(Action::RequestBlockedClients)
        ));
        assert_eq!(names(&screen), ["kiosk"], "the connected one shows at once");
        let refreshed = screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![
            client(1, "alpha", &json!({})),
            client(2, "kiosk", &json!({ "blocked": true })),
        ])));
        assert!(refreshed.is_none(), "{refreshed:?}");

        let offline = wireless(-60, "aa:bb:cc:dd:ee:01");
        let offline = client(
            7,
            "banned-phone",
            &json!({ "blocked": true, "ip": null, "client_type": offline["client_type"] }),
        );
        screen.apply_action(&Action::BlockedClientsLoaded(Arc::new(vec![offline])));
        assert_eq!(names(&screen), ["banned-phone", "kiosk"]);

        // The offline one can be unblocked and forgotten, and nothing else.
        screen.select(0);
        assert!(matches!(
            press(&mut screen, 'b'),
            Some(Action::RequestUnblockClient(unblocked)) if unblocked == id(7)
        ));
        assert!(matches!(
            press(&mut screen, 'F'),
            Some(Action::RequestForgetClient(forgotten)) if forgotten == id(7)
        ));
        for key in ['x', 'I'] {
            let (_, message) = toast(press(&mut screen, key));
            assert_eq!(message, "banned-phone is not connected", "{key}");
        }
        enter(&mut screen);
        assert!(matches!(
            press(&mut screen, 'b'),
            Some(Action::RequestUnblockClient(unblocked)) if unblocked == id(7)
        ));

        // A refresh asks again, since an unblock shows up as a rejoin.
        let refreshed = screen.apply_action(&Action::ClientsUpdated(Arc::new(vec![
            client(1, "alpha", &json!({})),
            client(7, "banned-phone", &json!({})),
        ])));
        assert!(matches!(refreshed, Some(Action::RequestBlockedClients)));
        assert_eq!(
            names(&screen),
            Vec::<String>::new(),
            "it is connected and unblocked"
        );

        // Other filters never ask.
        assert!(screen.set_filter(ClientTypeFilter::All).is_none());
    }

    #[test]
    fn commands_the_controller_cannot_serve_say_so_instead_of_failing() {
        let mut screen = screen(vec![client(
            1,
            "guest-phone",
            &json!({ "is_guest": true, "client_type": "Wireless" }),
        )]);
        screen.apply_action(&access(false));

        for (key, what) in [
            ('F', "Forgetting a client"),
            ('A', "Guest access"),
            ('I', "A fixed IP"),
        ] {
            let (level, message) = toast(press(&mut screen, key));
            assert_eq!(level, NotificationLevel::Info);
            assert!(
                message.starts_with(what) && message.contains("direct controller session"),
                "{key}: {message}"
            );
        }
        assert!(screen.asking.is_none(), "no prompt was armed");
        // Block and kick have a path on every profile.
        assert!(matches!(
            press(&mut screen, 'b'),
            Some(Action::RequestBlockClient(_))
        ));
        assert!(matches!(
            press(&mut screen, 'x'),
            Some(Action::RequestKickClient(_))
        ));

        screen.apply_action(&access(true));
        assert!(matches!(
            press(&mut screen, 'I'),
            Some(Action::OpenPrompt(_))
        ));
    }

    #[test]
    fn a_vpn_client_takes_no_fixed_ip() {
        let mut screen = screen(vec![
            client(1, "road-warrior", &json!({ "client_type": "Vpn" })),
            client(2, "tunnel", &json!({ "client_type": "Teleport" })),
        ]);
        for row in 0..2 {
            screen.select(row);
            let (_, message) = toast(press(&mut screen, 'I'));
            assert!(
                message.contains("only wired and wireless clients take a fixed IP"),
                "{message}"
            );
        }
        assert!(screen.asking.is_none());
    }

    #[test]
    fn show_client_drops_what_hides_it_and_opens_its_detail() {
        let mut screen = screen(vec![
            client(1, "alpha", &json!({})),
            client(2, "bravo", &wireless(-60, "aa:bb:cc:dd:ee:01")),
        ]);
        screen.set_filter(ClientTypeFilter::Wireless);
        screen.set_search_query("bra");

        let follow_up = screen.apply_action(&Action::ShowClient(id(1)));

        assert!(follow_up.is_none());
        assert!(screen.detail_open);
        assert_eq!(screen.detail_client_id, Some(id(1)));
        assert_eq!(screen.selected_client_id(), Some(id(1)));
        assert_eq!(screen.filter, ClientTypeFilter::All);
        assert_eq!(screen.search_query, "");

        // A client the view already lists keeps the view as it is.
        screen.close_detail();
        screen.set_filter(ClientTypeFilter::Wireless);
        screen.apply_action(&Action::ShowClient(id(2)));
        assert_eq!(screen.filter, ClientTypeFilter::Wireless);
        assert_eq!(screen.detail_client_id, Some(id(2)));

        let (level, message) = toast(screen.apply_action(&Action::ShowClient(id(9))));
        assert_eq!(level, NotificationLevel::Warning);
        assert_eq!(message, "That client is no longer connected");
    }

    #[test]
    fn a_controller_change_drops_everything_but_the_sort() {
        let mut screen = screen(vec![client(1, "alpha", &json!({}))]);
        press(&mut screen, 'r');
        screen.set_search_query("a");
        enter(&mut screen);
        press(&mut screen, 'I');

        screen.apply_action(&Action::ControllerChanged);

        assert!(screen.clients.is_empty());
        assert!(!screen.detail_open && screen.detail_client_id.is_none());
        assert!(screen.asking.is_none() && screen.access.is_none());
        assert_eq!(screen.search_query, "");
        assert!(screen.sort.descending, "the sort is the user's, not data");
    }

    #[test]
    fn help_follows_the_mode() {
        let mut screen = screen(vec![client(1, "alpha", &json!({}))]);
        let describes = |screen: &ClientsScreen, text: &str| {
            screen.help().iter().any(|(_, what)| *what == text)
        };
        assert!(describes(&screen, "Open the client"));
        assert!(describes(&screen, "Block / unblock"));
        assert!(!describes(&screen, "Close the detail"));

        enter(&mut screen);
        assert!(describes(&screen, "Previous / next client"));
        assert!(describes(&screen, "Close the detail"));
        assert!(!describes(&screen, "Open the client"));
        assert!(!describes(&screen, "Sort by the next column"));

        // The overlay gives a description 24 columns before it clips.
        for (key, what) in screen.help() {
            assert!(what.chars().count() <= 24, "{key}: {what:?} is clipped");
        }
    }
}
