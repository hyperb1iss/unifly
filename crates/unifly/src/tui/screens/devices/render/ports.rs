//! The Ports tab: the switch faceplate where there is room, and a port
//! list with a cursor that reaches every port.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Cell, Paragraph, Row, Table};

use unifly_api::Device;
use unifly_api::model::{PoeMode, Port, PortConnector, PortMode, PortProfile, PortState};

use super::detail::list_rows;
use super::grid::{self, DASH};
use crate::tui::screens::devices::DevicesScreen;
use crate::tui::screens::devices::detail::{Detail, Profiles, ordered_ports};
use crate::tui::table::{Column, plan_columns};
use crate::tui::theme;
use crate::tui::widgets::hyperchart::SwitchFaceplate;

/// Fewest ports worth a faceplate: an access point's one or two say
/// nothing the list does not.
const FACEPLATE_PORTS: usize = 4;

/// Narrowest pane the faceplate is drawn in.
const FACEPLATE_WIDTH: u16 = 44;

/// Port rows the list keeps when the faceplate is drawn above it.
const MIN_LIST_ROWS: u16 = 5;

pub(super) fn render(
    screen: &DevicesScreen,
    frame: &mut Frame,
    area: Rect,
    device: &Device,
    detail: &Detail,
) {
    let ports = ordered_ports(device);
    if ports.is_empty() {
        grid::note(frame, area, "This device reports no ports");
        return;
    }

    let note = profile_note(&detail.profiles);
    let note_rows = u16::from(note.is_some());
    let faceplate = faceplate_rows(&ports, area.width);
    let show_faceplate = faceplate > 0 && area.height >= faceplate + note_rows + 1 + MIN_LIST_ROWS;
    let layout = Layout::vertical([
        Constraint::Length(if show_faceplate { faceplate } else { 0 }),
        Constraint::Length(note_rows),
        Constraint::Min(0),
    ])
    .split(area);

    if show_faceplate {
        frame.render_widget(SwitchFaceplate::new(&device.ports), layout[0]);
    }
    if let Some(note) = note {
        let text = grid::truncate(&format!("  {note}"), usize::from(area.width));
        frame.render_widget(
            Paragraph::new(Span::styled(text, Style::default().fg(theme::text_muted()))),
            layout[1],
        );
    }
    render_list(screen, frame, layout[2], &ports, detail);
}

/// Rows the tab wants at `width`: the faceplate, the note about missing
/// profiles, and every port under the list's header.
pub(super) fn wanted_rows(device: &Device, detail: &Detail, width: u16) -> u16 {
    let ports = ordered_ports(device);
    if ports.is_empty() {
        return list_rows(0);
    }
    let note_rows = u16::from(profile_note(&detail.profiles).is_some());
    faceplate_rows(&ports, width)
        .saturating_add(note_rows)
        .saturating_add(list_rows(ports.len()))
}

/// Rows the faceplate takes at `width`, or none where it is not drawn: too
/// few ports to be worth one, or too narrow a pane.
fn faceplate_rows(ports: &[&Port], width: u16) -> u16 {
    if ports.len() >= FACEPLATE_PORTS && width >= FACEPLATE_WIDTH {
        faceplate_height(ports, width)
    } else {
        0
    }
}

/// Rows the faceplate takes at `width`: four per bank of ports, and the
/// legend. Mirrors how the widget wraps its banks.
fn faceplate_height(ports: &[&Port], width: u16) -> u16 {
    let max_index = ports.iter().map(|port| port.index).max().unwrap_or(0);
    let pairs = max_index.div_ceil(2).max(1);
    let pairs_per_bank = u32::from(width.saturating_sub(2) / 4).max(1);
    let banks = u16::try_from(pairs.div_ceil(pairs_per_bank)).unwrap_or(u16::MAX);
    banks.saturating_mul(4).saturating_add(1)
}

/// Why the profile columns are missing, when they are.
fn profile_note(profiles: &Profiles) -> Option<String> {
    match profiles {
        Profiles::Unasked | Profiles::Loaded(_) => None,
        Profiles::Loading => Some("Loading the port profiles\u{2026}".into()),
        Profiles::Unavailable(reason) => Some(format!("No port profiles: {reason}")),
    }
}

/// One port's cells as text.
struct PortCells {
    index: String,
    name: String,
    link: String,
    link_color: Color,
    poe: String,
    poe_color: Color,
    /// Mode, native network, and tagged networks, once profiles loaded.
    profile: Option<[String; 3]>,
    /// Connector and top speed, shown until the profiles load.
    hardware: [String; 2],
}

impl PortCells {
    fn new(port: &Port, profile: Option<&PortProfile>) -> Self {
        let (link, link_color) = match port.state {
            PortState::Up => (fmt_speed(port.speed_mbps, "up"), theme::success()),
            PortState::Down => ("down".to_owned(), theme::text_muted()),
            PortState::Unknown => (DASH.to_owned(), theme::text_muted()),
        };
        let (poe, poe_color) = poe_cell(port, profile);
        Self {
            index: port.index.to_string(),
            name: profile
                .and_then(|profile| profile.name.clone())
                .or_else(|| port.name.clone())
                .unwrap_or_else(|| DASH.to_owned()),
            link,
            link_color,
            poe,
            poe_color,
            profile: profile.map(profile_cells),
            hardware: [
                connector_label(port.connector.as_ref()).to_owned(),
                fmt_speed(port.max_speed_mbps, DASH),
            ],
        }
    }
}

fn fmt_speed(mbps: Option<u32>, unknown: &str) -> String {
    match mbps {
        Some(speed) if speed >= 1000 && speed % 1000 == 0 => format!("{}G", speed / 1000),
        Some(speed) if speed >= 1000 => format!("{:.1}G", f64::from(speed) / 1000.0),
        Some(speed) => format!("{speed}M"),
        None => unknown.to_owned(),
    }
}

fn connector_label(connector: Option<&PortConnector>) -> &'static str {
    match connector {
        Some(PortConnector::Rj45) => "RJ45",
        Some(PortConnector::Sfp) => "SFP",
        Some(PortConnector::SfpPlus) => "SFP+",
        Some(PortConnector::Sfp28) => "SFP28",
        Some(PortConnector::Qsfp28) => "QSFP28",
        None => DASH,
    }
}

/// The port's PoE setting, and whether it is delivering power.
fn poe_cell(port: &Port, profile: Option<&PortProfile>) -> (String, Color) {
    let Some(poe) = &port.poe else {
        return (DASH.to_owned(), theme::text_muted());
    };
    // A mode the profile does not name follows whether power is on.
    let mode = match profile.and_then(|profile| profile.poe_mode) {
        Some(PoeMode::Passive24V) => "24V",
        Some(PoeMode::Passthrough) => "pass",
        Some(PoeMode::Auto) => "auto",
        Some(PoeMode::Other) | None if poe.enabled => "auto",
        Some(PoeMode::Off | PoeMode::Other) | None => "off",
    };
    if poe.enabled && poe.state == PortState::Up {
        (format!("{mode} \u{26A1}"), theme::accent_secondary())
    } else if poe.enabled {
        (mode.to_owned(), theme::text_secondary())
    } else {
        (mode.to_owned(), theme::text_muted())
    }
}

fn profile_cells(profile: &PortProfile) -> [String; 3] {
    let mode = match profile.mode {
        PortMode::Access => "access",
        PortMode::Trunk => "trunk",
        PortMode::Mirror => "mirror",
        PortMode::Unknown => DASH,
    };
    let native = profile
        .native_network_name
        .clone()
        .or_else(|| profile.native_vlan_id.map(|vlan| format!("VLAN {vlan}")))
        .unwrap_or_else(|| DASH.to_owned());
    let tagged = if profile.tagged_all {
        "all".to_owned()
    } else if !profile.tagged_network_names.is_empty() {
        profile.tagged_network_names.join(", ")
    } else if !profile.tagged_vlan_ids.is_empty() {
        let vlans: Vec<String> = profile
            .tagged_vlan_ids
            .iter()
            .map(ToString::to_string)
            .collect();
        vlans.join(", ")
    } else if profile.mode == PortMode::Unknown {
        DASH.to_owned()
    } else {
        "none".to_owned()
    };
    [mode.to_owned(), native, tagged]
}

fn render_list(
    screen: &DevicesScreen,
    frame: &mut Frame,
    area: Rect,
    ports: &[&Port],
    detail: &Detail,
) {
    let cells: Vec<PortCells> = ports
        .iter()
        .map(|port| PortCells::new(port, detail.profile(port.index)))
        .collect();
    let with_profiles = cells.iter().any(|cells| cells.profile.is_some());
    let extra = |cells: &PortCells, at: usize| -> String {
        match &cells.profile {
            Some(profile) => profile[at.min(2)].clone(),
            None if with_profiles => DASH.to_owned(),
            None => cells.hardware.get(at).cloned().unwrap_or_default(),
        }
    };
    let extra_headers: &[&str] = if with_profiles {
        &["Mode", "Native", "Tagged"]
    } else {
        &["Type", "Max"]
    };

    let mut headers = vec!["", "Port", "Name", "Link", "PoE"];
    let mut columns = vec![
        Column::fixed(1).tight(),
        Column::fit("Port", cells.iter().map(|c| &c.index), 4),
        Column::fit("Name", cells.iter().map(|c| &c.name), 28).shrink_to(8),
        Column::fit("Link", cells.iter().map(|c| &c.link), 5),
        Column::fit("PoE", cells.iter().map(|c| &c.poe), 6),
    ];
    for (at, header) in extra_headers.iter().enumerate() {
        headers.push(header);
        let column = Column::fit(header, cells.iter().map(|c| extra(c, at)), 40);
        columns.push(if at > 0 { column.shrink_to(6) } else { column });
    }

    let widths = plan_columns(&columns, area.width);
    let selected = detail.cursor_index();
    let rows: Vec<Row> = cells
        .iter()
        .enumerate()
        .map(|(index, cells)| {
            let is_selected = index == selected;
            let mut row = vec![
                Cell::from(if is_selected { "▸" } else { " " }),
                Cell::from(cells.index.clone()),
                Cell::from(grid::clipped(&widths, 2, cells.name.clone(), 0))
                    .style(Style::default().fg(theme::accent_secondary())),
                Cell::from(cells.link.clone()).style(Style::default().fg(cells.link_color)),
                Cell::from(cells.poe.clone()).style(Style::default().fg(cells.poe_color)),
            ];
            row.extend((0..extra_headers.len()).map(|at| Cell::from(extra(cells, at))));
            Row::new(row).style(if is_selected {
                theme::table_selected().add_modifier(Modifier::BOLD)
            } else {
                theme::table_row()
            })
        })
        .collect();

    let header = Row::new(
        headers
            .into_iter()
            .map(|label| Cell::from(label).style(theme::table_header())),
    );
    let table = Table::new(rows, widths).header(header);
    let len = cells.len();
    let drawn = detail
        .scroll
        .render(frame, table, area, &detail.cursor, len);
    register_rows(screen, area, drawn.offset(), len);
}

/// Register each visible row of a detail list drawn in `area` under a
/// one-row header, first row `offset`, so a tap moves the cursor to it.
pub(super) fn register_rows(screen: &DevicesScreen, area: Rect, offset: usize, len: usize) {
    let mut hits = screen.detail_hits.borrow_mut();
    let body = Rect {
        y: area.y.saturating_add(1),
        height: area.height.saturating_sub(1),
        ..area
    };
    for (row, index) in (body.y..body.bottom()).zip(offset..len) {
        hits.rows.push(
            Rect {
                y: row,
                height: 1,
                ..body
            },
            index,
        );
    }
}
