use std::sync::Arc;

use super::super::ClientsScreen;
use super::super::labels::LinkIndex;
use super::detail::{detail_height, render_detail};

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Row, Table};

use unifly_api::{Client, ClientType};

use crate::tui::action::ClientTypeFilter;
use crate::tui::screens::clients::commands::is_authorized;
use crate::tui::screens::clients::state::Fetch;
use crate::tui::screens::clients::view::{FILTER_LABELS, client_name, empty_note};
use crate::tui::screens::devices::grid;
use crate::tui::table::{Column, plan_columns};
use crate::tui::theme;
use crate::tui::widgets::click::{self, Hint};
use crate::tui::widgets::{bytes_fmt, sub_tabs, toolbar};

pub(super) fn render_screen(screen: &ClientsScreen, frame: &mut Frame, area: Rect) {
    screen.hits.borrow_mut().clear();
    let filtered = screen.filtered_clients();

    let block = Block::default()
        .title(title(screen, filtered.len()))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(if screen.focused {
            theme::border_focused()
        } else {
            theme::border_default()
        });

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // An open detail takes the rows its fields need and the table keeps
    // the rest. On a very short screen the pane takes all of it.
    let detail_client = screen.detail_client(&filtered);
    let (table_area, detail_area) = if let Some(client) = detail_client {
        let detail = detail_height(screen, client, inner.width);
        let list = grid::list_rows_above_detail(inner.height, detail);
        let chunks = Layout::vertical([Constraint::Length(list), Constraint::Min(0)]).split(inner);
        (chunks[0], Some(chunks[1]))
    } else {
        (inner, None)
    };

    let layout = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(table_area);

    render_filter_bar(screen, frame, layout[0]);

    let selected_idx = screen
        .detail_client_index(&filtered)
        .filter(|_| screen.detail_open)
        .unwrap_or_else(|| screen.selected_index());
    if filtered.is_empty() {
        grid::note(frame, layout[1], &empty_state(screen));
    } else {
        render_client_table(screen, frame, layout[1], &filtered, selected_idx);
    }

    if let (Some(detail_area), Some(client)) = (detail_area, detail_client) {
        render_detail(screen, frame, detail_area, client);
    }
}

/// What the status bar offers: the commands the selected client can take,
/// behind the way into its detail or out of it.
pub(in crate::tui::screens::clients) fn status_hints(screen: &ClientsScreen) -> Vec<(u8, Hint)> {
    let filtered = screen.filtered_clients();
    if let Some(client) = screen.detail_client(&filtered) {
        let mut hints = command_hints(screen, client);
        hints.push((0, Hint::button("Esc", "close", KeyCode::Esc)));
        return hints;
    }
    let Some(client) = filtered.get(screen.selected_index()) else {
        return Vec::new();
    };
    let mut hints = vec![(0, Hint::button("Enter", "detail", KeyCode::Enter))];
    hints.extend(command_hints(screen, client));
    hints
}

/// The screen title: how many clients show, of how many, and the search
/// query narrowing them.
fn title(screen: &ClientsScreen, shown: usize) -> Line<'static> {
    let total = screen.clients.len();
    let count = if shown == total {
        format!(" Clients ({total}) ")
    } else {
        format!(" Clients ({shown}/{total}) ")
    };
    let mut spans = vec![Span::styled(count, theme::title_style())];
    if !screen.search_query.is_empty() {
        spans.push(Span::styled(
            format!("[\"{}\"] ", screen.search_query),
            Style::default().fg(theme::warning()),
        ));
    }
    Line::from(spans)
}

/// What the table area says when it has no rows: still waiting for the
/// controller, nobody connected, or nobody passing the filter.
fn empty_state(screen: &ClientsScreen) -> String {
    if screen.access.is_none() && screen.clients.is_empty() {
        return "Waiting for the controller\u{2026}".into();
    }
    if !screen.search_query.is_empty() {
        return format!(
            "No client matches \"{}\" (/ then Esc clears the search)",
            screen.search_query
        );
    }
    if screen.filter == ClientTypeFilter::Blocked {
        match &screen.offline_blocked_status {
            Fetch::Unasked | Fetch::Loading => return "Loading blocked clients\u{2026}".into(),
            Fetch::Unavailable(reason) => {
                return format!("Offline blocked clients are not listed: {reason}");
            }
            Fetch::Loaded => {}
        }
    }
    empty_note(screen.filter).into()
}

/// The type filter strip, with the sort against the right edge.
fn render_filter_bar(screen: &ClientsScreen, frame: &mut Frame, area: Rect) {
    let filters = sub_tabs::sub_tab_strip(&FILTER_LABELS, screen.filter_index());
    // `s` belongs to the table, so the chip is only a button while no
    // detail has the keys.
    let key = (!screen.detail_open).then(|| ("s", click::char_key('s')));
    let sort = toolbar::chip(key, "sort", screen.sort.label());
    toolbar::render(frame, area, filters, sort, &mut screen.hits.borrow_mut());
}

/// Hints for the commands `client` can take in its state and under the
/// controller's access, ranked for a narrow bar. The block hint says
/// which way `b` goes.
pub(super) fn command_hints(screen: &ClientsScreen, client: &Client) -> Vec<(u8, Hint)> {
    let block = if client.blocked { "unblock" } else { "block" };
    let mut hints = vec![(1, Hint::button("b", block, click::char_key('b')))];
    let session = screen.has_session();
    let connected = !screen.is_offline(client);
    if connected && client.client_type == ClientType::Wireless {
        hints.push((1, Hint::button("x", "kick", click::char_key('x'))));
    }
    if session && connected && client.is_guest {
        let guest = if is_authorized(client) {
            "revoke guest"
        } else {
            "authorize"
        };
        hints.push((2, Hint::button("A", guest, click::char_key('A'))));
    }
    if session && connected && ClientsScreen::can_reserve(client) {
        hints.push((4, Hint::button("I", "fixed IP", click::char_key('I'))));
    }
    if session {
        hints.push((5, Hint::button("F", "forget", click::char_key('F'))));
    }
    hints
}

/// Table width from which the Clients table adds its Network and Uplink columns:
/// a 120-column terminal, less the screen border.
const WIDE_TABLE: u16 = 118;

/// Table width below which the MAC column steps aside so client names
/// keep their room: a 90-column terminal, less the screen border.
const MAC_TABLE: u16 = 88;

/// Header of the traffic column: bytes since the client connected, sent
/// then received.
const TRAFFIC_HEADER: &str = "TX/RX total";

/// One client's table cells as text, measured once for column sizing and
/// then styled into a row. Cells of columns the layout hides stay `None`
/// and are never computed.
struct ClientCells {
    name: String,
    ip: String,
    mac: Option<String>,
    network: Option<String>,
    uplink: Option<String>,
    traffic: String,
    duration: String,
}

impl ClientCells {
    fn new(links: &LinkIndex, client: &Client, layout: ClientColumns) -> Self {
        let dash = || "─".to_owned();
        Self {
            name: client_name(client).to_owned(),
            ip: client
                .ip
                .map_or_else(|| "─".into(), |client_ip| client_ip.to_string()),
            mac: layout.mac.then(|| client.mac.to_string()),
            network: layout
                .wide
                .then(|| links.network_label(client).unwrap_or_else(dash)),
            uplink: layout
                .wide
                .then(|| links.uplink_label(client).unwrap_or_else(dash)),
            traffic: match (client.tx_bytes, client.rx_bytes) {
                (None, None) => dash(),
                (tx, rx) => bytes_fmt::fmt_tx_rx(tx.unwrap_or(0), rx.unwrap_or(0)),
            },
            duration: client.connected_at.map_or_else(
                || "─".into(),
                |ts| {
                    let dur = chrono::Utc::now().signed_duration_since(ts);
                    let secs = u64::try_from(dur.num_seconds().max(0)).unwrap_or_default();
                    bytes_fmt::fmt_uptime(secs)
                },
            ),
        }
    }
}

fn render_client_table(
    screen: &ClientsScreen,
    frame: &mut Frame,
    area: Rect,
    filtered: &[&Arc<Client>],
    selected_idx: usize,
) {
    let layout = ClientColumns {
        mac: area.width >= MAC_TABLE,
        wide: area.width >= WIDE_TABLE,
    };
    let cells: Vec<ClientCells> = filtered
        .iter()
        .map(|client| ClientCells::new(&screen.links, client, layout))
        .collect();

    let mut headers = vec!["Type", "Name", "IP"];
    let mut columns = vec![
        Column::fixed(4).tight(),
        Column::fit("Name", cells.iter().map(|c| &c.name), 32).shrink_to(10),
        Column::fit("IP", cells.iter().map(|c| &c.ip), 39).shrink_to(15),
    ];
    if layout.mac {
        headers.push("MAC");
        columns.push(Column::fixed(17));
    }
    if layout.wide {
        headers.extend(["Network", "Uplink"]);
        columns.extend([
            Column::fit(
                "Network",
                cells.iter().filter_map(|c| c.network.as_ref()),
                20,
            )
            .shrink_to(8),
            Column::fit("Uplink", cells.iter().filter_map(|c| c.uplink.as_ref()), 32).shrink_to(10),
        ]);
    }
    headers.extend(["Signal", TRAFFIC_HEADER, "Duration"]);
    columns.extend([
        Column::fixed(6),
        Column::fit(TRAFFIC_HEADER, cells.iter().map(|c| &c.traffic), 15).at_least(11),
        Column::fit("Duration", cells.iter().map(|c| &c.duration), 8),
    ]);

    let header = Row::new(
        headers
            .into_iter()
            .map(|label| Cell::from(label).style(theme::table_header())),
    );
    let widths = plan_columns(&columns, area.width);
    let rows: Vec<Row> = filtered
        .iter()
        .zip(cells)
        .enumerate()
        .map(|(index, (client, mut cells))| {
            cells.name = grid::clipped(&widths, 1, cells.name, 0);
            render_table_row(index == selected_idx, client, cells)
        })
        .collect();

    let len = rows.len();
    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(theme::table_selected());

    let state = screen
        .table_state
        .with_selected(Some(selected_idx.min(len.saturating_sub(1))));
    let state = screen.table_scroll.render(frame, table, area, &state, len);
    screen.hits.borrow_mut().push_table_rows(area, &state, len);
}

/// Optional columns the Clients table has room for.
#[derive(Clone, Copy)]
struct ClientColumns {
    mac: bool,
    wide: bool,
}

/// Marks a blocked client in the Type cell.
const BLOCKED_MARKER: &str = "\u{2298}";

/// The Type cell: the cursor, the client's kind, and a mark when it is
/// blocked.
fn type_cell(is_selected: bool, client: &Client) -> Line<'static> {
    let prefix = if is_selected { "▸" } else { " " };
    let (kind, color) = if client.is_guest {
        ("G", theme::warning())
    } else {
        match client.client_type {
            ClientType::Wireless => ("W", theme::accent_secondary()),
            ClientType::Wired => ("E", theme::text_secondary()),
            ClientType::Vpn => ("V", theme::accent_primary()),
            ClientType::Teleport => ("T", theme::accent_tertiary()),
            _ => ("?", theme::text_secondary()),
        }
    };
    let mut spans = vec![Span::styled(
        format!("{prefix}{kind}"),
        Style::default().fg(color),
    )];
    if client.blocked {
        spans.push(Span::styled(
            BLOCKED_MARKER,
            Style::default()
                .fg(theme::error())
                .add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

fn render_table_row(is_selected: bool, client: &Client, cells: ClientCells) -> Row<'static> {
    let signal_dbm = client
        .wireless
        .as_ref()
        .and_then(|wireless| wireless.signal_dbm);
    let (signal, signal_color) = signal_dbm.map_or(("····", theme::border_unfocused()), |dbm| {
        if dbm >= -50 {
            ("▂▄▆█", theme::success())
        } else if dbm >= -60 {
            ("▂▄▆ ", theme::accent_secondary())
        } else if dbm >= -70 {
            ("▂▄  ", theme::warning())
        } else if dbm >= -80 {
            ("▂   ", theme::accent_tertiary())
        } else {
            ("·   ", theme::error())
        }
    });

    let name_style = Style::default()
        .fg(theme::accent_secondary())
        .add_modifier(if is_selected {
            Modifier::BOLD
        } else {
            Modifier::empty()
        });

    let mut row = vec![
        Cell::from(type_cell(is_selected, client)),
        Cell::from(cells.name).style(name_style),
        Cell::from(cells.ip).style(Style::default().fg(theme::accent_tertiary())),
    ];
    if let Some(mac) = cells.mac {
        row.push(Cell::from(mac));
    }
    if let (Some(network), Some(uplink)) = (cells.network, cells.uplink) {
        row.extend([
            Cell::from(network),
            Cell::from(uplink).style(Style::default().fg(theme::text_secondary())),
        ]);
    }
    row.extend([
        Cell::from(signal).style(Style::default().fg(signal_color)),
        Cell::from(cells.traffic),
        Cell::from(cells.duration),
    ]);

    Row::new(row).style(if is_selected {
        theme::table_selected()
    } else {
        theme::table_row()
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crossterm::event::{KeyCode, KeyEvent};
    use serde_json::json;
    use unifly_api::Client;

    use super::{ClientCells, ClientColumns, status_hints};
    use crate::tui::action::{Access, Action};
    use crate::tui::screens::clients::ClientsScreen;
    use crate::tui::screens::devices::fixtures::{client, network, switch, wireless};
    use crate::tui::table::render_lines;
    use crate::tui::widgets::click::{self, click_label};

    const NETWORK: &str = "11111111-1111-4111-8111-111111111111";

    /// A wired client on switch port 14 of sw-core, on the Render network.
    fn station() -> Arc<Client> {
        client(
            40,
            "dailies-review-station",
            &json!({
                "uplink_device_mac": "aa:bb:cc:dd:ee:02",
                "switch_port": 14,
                "network_id": NETWORK,
                "vlan": 20,
                "tx_bytes": 151_400_000_000_u64,
                "rx_bytes": 352_000_000_000_u64
            }),
        )
    }

    fn screen_of(clients: Vec<Arc<Client>>) -> ClientsScreen {
        let mut screen = ClientsScreen::new();
        screen.apply_action(&Action::AccessUpdated(Access {
            integration: true,
            session: true,
            live_events: true,
        }));
        screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![switch(
            2, "sw-core", 24,
        )])));
        screen.apply_action(&Action::NetworksUpdated(Arc::new(vec![network(
            NETWORK, "Render", 20,
        )])));
        screen.apply_action(&Action::ClientsUpdated(Arc::new(clients)));
        screen
    }

    fn screen() -> ClientsScreen {
        screen_of(vec![station()])
    }

    fn draw(screen: &ClientsScreen, width: u16, height: u16) -> Vec<String> {
        render_lines(width, height, |frame, area| {
            screen.render_screen(frame, area);
        })
    }

    fn render(width: u16) -> String {
        draw(&screen(), width, 10).join("\n")
    }

    fn press(screen: &mut ClientsScreen, code: KeyCode) {
        screen.handle_key_input(KeyEvent::from(code));
    }

    /// What the status bar offers for the screen as it stands.
    fn hints(screen: &ClientsScreen) -> String {
        click::hint_text(&status_hints(screen))
    }

    #[test]
    fn tapping_a_row_selects_it_and_a_second_tap_opens_it() {
        let mut screen = screen_of(vec![
            station(),
            client(41, "edit-bay-render-node", &json!({})),
        ]);
        let row = "edit-bay-render-node";

        let rows = draw(&screen, 120, 12);
        screen.handle_mouse_input(click_label(&rows, row, row));
        assert_eq!(screen.selected_client_id(), Some("client-41".into()));
        assert!(!screen.detail_open);

        let rows = draw(&screen, 120, 12);
        screen.handle_mouse_input(click_label(&rows, row, row));
        assert!(screen.detail_open);
        assert_eq!(screen.detail_client_id, Some("client-41".into()));
    }

    #[test]
    fn wide_table_adds_network_and_uplink_and_keeps_values_whole() {
        for width in [120, 183] {
            let text = render(width);
            for expected in [
                "Type ",
                "dailies-review-station",
                "151.4G/352.0G",
                "82:8b:02:66:7c:28",
                "Render",
                "sw-core port 14",
                "TX/RX total",
            ] {
                assert!(text.contains(expected), "missing {expected:?}:\n{text}");
            }
        }
    }

    #[test]
    fn hidden_columns_are_never_resolved() {
        let screen = screen();
        let client = screen.clients.first().expect("fixture client");

        let narrow = ClientColumns {
            mac: false,
            wide: false,
        };
        let cells = ClientCells::new(&screen.links, client, narrow);
        assert_eq!(cells.mac, None);
        assert_eq!(cells.network, None);
        assert_eq!(cells.uplink, None);

        let wide = ClientColumns {
            mac: true,
            wide: true,
        };
        let cells = ClientCells::new(&screen.links, client, wide);
        assert_eq!(cells.mac.as_deref(), Some("82:8b:02:66:7c:28"));
        assert_eq!(cells.network.as_deref(), Some("Render"));
        assert_eq!(cells.uplink.as_deref(), Some("sw-core port 14"));
    }

    #[test]
    fn narrow_table_favours_names_over_the_mac() {
        let text = render(80);
        for expected in ["dailies-review-station", "10.40.20.40", "151.4G/352.0G"] {
            assert!(text.contains(expected), "missing {expected:?}:\n{text}");
        }
        assert!(
            !text.contains("82:8b:02:66:7c:28"),
            "MAC at 80 columns:\n{text}"
        );
        assert!(!text.contains("Uplink"), "wide-only column at 80:\n{text}");
    }

    #[test]
    fn a_name_too_long_for_its_column_ends_in_an_ellipsis() {
        let text = draw(&screen(), 50, 10).join("\n");
        assert!(text.contains('…'), "{text}");
        assert!(text.contains("10.40.20.40"), "the address is cut:\n{text}");
    }

    #[test]
    fn unknown_traffic_is_a_dash_not_zero_bytes() {
        let screen = screen_of(vec![client(1, "quiet", &json!({}))]);
        let text = draw(&screen, 120, 10).join("\n");
        assert!(!text.contains("0B/0B"), "{text}");
    }

    #[test]
    fn a_blocked_client_is_marked_in_the_table_and_b_offers_to_unblock() {
        let screen = screen_of(vec![
            client(1, "alpha", &json!({})),
            client(2, "bravo", &json!({ "blocked": true })),
        ]);
        let rows = draw(&screen, 100, 12);
        let row = |name: &str| rows.iter().find(|row| row.contains(name)).cloned();
        assert!(
            row("bravo").is_some_and(|row| row.contains('⊘')),
            "{rows:?}"
        );
        assert!(
            row("alpha").is_some_and(|row| !row.contains('⊘')),
            "{rows:?}"
        );
        assert!(hints(&screen).contains("b block"), "{}", hints(&screen));

        let mut screen = screen;
        screen.select(1);
        assert!(hints(&screen).contains("b unblock"), "{}", hints(&screen));
    }

    #[test]
    fn hints_only_offer_what_works_for_the_selected_client() {
        let mut screen = screen_of(vec![
            client(1, "alpha", &json!({})),
            client(2, "bravo", &wireless(-55, "aa:bb:cc:dd:ee:02")),
            client(3, "guest-phone", &json!({ "is_guest": true })),
        ]);
        let wired = hints(&screen);
        assert!(
            wired.contains("Enter detail") && wired.contains("F forget"),
            "{wired}"
        );
        assert!(
            !wired.contains("kick") && !wired.contains("authorize"),
            "{wired}"
        );

        screen.select(1);
        assert!(hints(&screen).contains("x kick"));
        screen.select(2);
        assert!(hints(&screen).contains("A authorize"));

        // Without a session the commands only it serves are not offered.
        screen.apply_action(&Action::AccessUpdated(Access {
            integration: true,
            ..Access::default()
        }));
        let cloud = hints(&screen);
        assert!(cloud.contains("b block"), "{cloud}");
        for dead in ["authorize", "fixed IP", "forget"] {
            assert!(!cloud.contains(dead), "{dead} is offered: {cloud}");
        }
        screen.apply_action(&Action::AccessUpdated(Access {
            integration: true,
            session: true,
            live_events: true,
        }));

        // With a detail open the bar is the detail's, and the screen
        // draws no hint row of its own in either mode.
        press(&mut screen, KeyCode::Enter);
        let bar = hints(&screen);
        assert!(!bar.contains("Enter detail"), "a dead Enter hint: {bar}");
        assert!(bar.contains("x kick") || bar.contains("b block"), "{bar}");
        assert!(bar.ends_with("Esc close"), "{bar}");
        let text = draw(&screen, 120, 24).join("\n");
        assert!(!text.contains("Esc close"), "a second hint bar:\n{text}");
    }

    #[test]
    fn the_blocked_filter_says_whether_it_is_loading_empty_or_unreadable() {
        let mut screen = screen_of(vec![client(1, "alpha", &json!({}))]);
        let note = |screen: &ClientsScreen| draw(screen, 100, 12).join("\n");

        screen.set_filter(crate::tui::action::ClientTypeFilter::Blocked);
        assert!(note(&screen).contains("Loading blocked clients"));

        screen.apply_action(&Action::BlockedClientsLoaded(Arc::new(Vec::new())));
        assert!(note(&screen).contains("No blocked clients"));

        screen.apply_action(&Action::BlockedClientsFailed(
            "no direct controller session".into(),
        ));
        let text = draw(&screen, 80, 12).join("\n");
        assert!(
            text.contains("Offline blocked clients are not listed: no direct controller session"),
            "{text}"
        );

        let offline = client(7, "banned-phone", &json!({ "blocked": true, "ip": null }));
        screen.apply_action(&Action::BlockedClientsLoaded(Arc::new(vec![offline])));
        let rows = draw(&screen, 100, 12);
        assert!(
            rows.iter()
                .any(|row| row.contains("banned-phone") && row.contains('\u{2298}')),
            "{rows:?}"
        );
        let bar = hints(&screen);
        assert!(
            bar.contains("b unblock") && bar.contains("F forget"),
            "{bar}"
        );
        assert!(!bar.contains("fixed IP") && !bar.contains("kick"), "{bar}");
    }

    #[test]
    fn the_title_and_filter_bar_say_what_is_listed() {
        let mut screen = screen_of(vec![
            client(1, "alpha", &json!({})),
            client(2, "bravo", &wireless(-55, "aa:bb:cc:dd:ee:02")),
        ]);
        let top = draw(&screen, 100, 10).join("\n");
        assert!(top.contains("Clients (2)"), "{top}");
        // The sort sits against the right edge, where it stays whatever
        // the filter strip says.
        let bar = top.lines().nth(1).unwrap_or_default();
        assert!(bar.trim_end().ends_with("s sort name ↑ │"), "{bar}");

        screen.set_search_query("bra");
        let top = draw(&screen, 100, 10).join("\n");
        assert!(top.contains("Clients (1/2) [\"bra\"]"), "{top}");
    }

    #[test]
    fn an_empty_table_says_why_instead_of_drawing_a_bare_header() {
        let waiting = ClientsScreen::new();
        let text = draw(&waiting, 80, 12).join("\n");
        assert!(text.contains("Waiting for the controller"), "{text}");
        assert!(!text.contains("Duration"), "a header over nothing:\n{text}");

        let mut screen = screen_of(Vec::new());
        let text = draw(&screen, 80, 12).join("\n");
        assert!(text.contains("No clients connected"), "{text}");

        screen = self::screen();
        press(&mut screen, KeyCode::Char('l'));
        let text = draw(&screen, 80, 12).join("\n");
        assert!(text.contains("No wireless clients connected"), "{text}");

        screen.set_search_query("zzz");
        let text = draw(&screen, 80, 12).join("\n");
        assert!(text.contains("No client matches \"zzz\""), "{text}");
    }

    #[test]
    fn the_detail_names_the_network_and_the_uplink_in_an_aligned_grid() {
        let mut screen = screen();
        press(&mut screen, KeyCode::Enter);
        let rows = draw(&screen, 80, 22);
        let text = rows.join("\n");

        for expected in [
            "dailies-review-station  ·  Wired",
            "Render (VLAN 20)",
            "sw-core port 14",
            "82:8b:02:66:7c:28",
            "no, from DHCP",
            "151.4G / 352.0G",
        ] {
            assert!(text.contains(expected), "missing {expected:?}:\n{text}");
        }
        assert!(!text.contains(NETWORK), "the raw network id:\n{text}");
        assert!(
            !text.contains("SSID"),
            "wireless rows on a wired client:\n{text}"
        );

        let column = |label: &str, value: &str| {
            let row = rows.iter().find(|row| row.contains(label))?;
            Some(row.find(value)? - row.find(label)?)
        };
        assert_eq!(
            column("Network ", "Render"),
            column("Uplink ", "sw-core port 14"),
            "values start in different columns:\n{text}"
        );
    }

    #[test]
    fn a_wireless_detail_adds_the_radio_link() {
        let mut screen = screen_of(vec![client(7, "phone", &{
            let mut fields = wireless(-58, "aa:bb:cc:dd:ee:02");
            fields["hostname"] = json!("pixel-9");
            fields["os_name"] = json!("Android");
            fields
        })]);
        press(&mut screen, KeyCode::Enter);
        let text = draw(&screen, 120, 30).join("\n");
        for expected in [
            "phone  ·  Wireless",
            "Harbor",
            "-58 dBm",
            "44 (5 GHz)",
            "866 / 720 Mbps",
            "97%",
            "pixel-9",
            "Android",
            // Found by BSSID, since it reports no uplink MAC.
            "sw-core",
        ] {
            assert!(text.contains(expected), "missing {expected:?}:\n{text}");
        }
    }

    #[test]
    fn nothing_panics_at_tiny_sizes() {
        let mut screen = screen();
        for (width, height) in [(40, 10), (20, 5), (10, 3), (1, 1)] {
            draw(&screen, width, height);
        }
        press(&mut screen, KeyCode::Enter);
        for (width, height) in [(40, 10), (20, 5), (10, 3), (1, 1)] {
            draw(&screen, width, height);
        }
    }
}
