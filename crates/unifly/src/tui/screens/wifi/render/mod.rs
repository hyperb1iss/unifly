mod empty;
mod panels;
mod tables;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use unifly_api::DeviceState;

use crate::tui::action::{Action, WifiBand, WifiSubTab};
use crate::tui::screens::devices::grid;
use crate::tui::theme;
use crate::tui::widgets::click::{self, Click, ClickLine};
use crate::tui::widgets::wrap::wrap;
use crate::tui::widgets::{sub_tabs, toolbar};

use super::{OverviewPanel, WifiScreen};

/// Panel size from which a detail sits beside its table instead of under it.
const SIDE_PANEL_MIN: (u16, u16) = (110, 18);

/// Rows a panel takes, frame included, when all it has to say is one
/// note: that nothing is selected, or why it cannot be read.
const NOTE_PANEL_ROWS: u16 = 5;

/// Rows a table keeps for its header and the note that it has no rows.
const EMPTY_TABLE_ROWS: u16 = 3;

/// Rows a table keeps above a panel that wants `panel` rows, in `height`
/// rows of screen: what the panel leaves, never more than the `table`
/// has rows for, and its share when the panel would take everything. A
/// screen too short to share goes to the panel whole.
fn rows_above_panel(height: u16, table: u16, panel: u16) -> u16 {
    let share = grid::list_rows_beside_detail(height).min(table);
    height.saturating_sub(panel).clamp(share, table)
}

/// Where a table's panel goes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PanelPlace {
    /// Nowhere: the table has the screen.
    Closed,
    /// Under the table.
    Under,
    /// Beside the table on a screen with room, and under it otherwise.
    BesideOrUnder,
}

impl WifiScreen {
    pub(super) fn render_screen(&self, frame: &mut Frame, area: Rect) {
        self.hits.borrow_mut().clear();
        // The title starts one cell in, past the corner.
        let title_row = Rect {
            x: area.x.saturating_add(1),
            width: area.width.saturating_sub(2),
            height: area.height.min(1),
            ..area
        };
        let title = self.title().draw(title_row, &mut self.hits.borrow_mut());
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(if self.focused {
                theme::border_focused()
            } else {
                theme::border_default()
            });

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // The health line is about access points, so only their tab
        // gives it a row.
        let banner = u16::from(self.sub_tab == WifiSubTab::Overview);
        let [toolbar_row, banner_row, body] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(banner),
            Constraint::Min(0),
        ])
        .areas(inner);

        toolbar::render_chips(
            frame,
            toolbar_row,
            self.tab_strip(),
            self.chips(),
            &mut self.hits.borrow_mut(),
        );
        frame.render_widget(Paragraph::new(self.health_banner()), banner_row);

        match self.sub_tab {
            WifiSubTab::Overview => self.render_overview(frame, body),
            WifiSubTab::Clients => self.render_clients(frame, body),
            WifiSubTab::Ssids => self.render_ssids(frame, body),
            WifiSubTab::Neighbors => self.render_neighbors(frame, body),
            WifiSubTab::Roaming => self.render_roaming(frame, body),
        }
    }

    /// Rows the active table has to draw: its header and its rows, or
    /// room for the note that it has none.
    fn table_rows(&self) -> u16 {
        u16::try_from(self.active_len() + 1)
            .unwrap_or(u16::MAX)
            .max(EMPTY_TABLE_ROWS)
    }

    fn render_overview(&self, frame: &mut Frame, area: Rect) {
        let panel = self.overview_panel;
        self.render_with_panel(
            frame,
            area,
            if panel == OverviewPanel::None {
                PanelPlace::Closed
            } else {
                PanelPlace::BesideOrUnder
            },
            |width, _| match panel {
                OverviewPanel::None => 0,
                OverviewPanel::Detail => self.ap_detail_height(),
                OverviewPanel::Channels => self.channel_map_height(width),
            },
            |frame, area| self.render_ap_table(frame, area),
            |frame, area| match panel {
                OverviewPanel::Detail => self.render_ap_detail(frame, area),
                OverviewPanel::Channels => self.render_channel_map(frame, area),
                OverviewPanel::None => {}
            },
        );
    }

    fn render_ssids(&self, frame: &mut Frame, area: Rect) {
        self.render_with_panel(
            frame,
            area,
            if self.ssid_detail_open {
                PanelPlace::BesideOrUnder
            } else {
                PanelPlace::Closed
            },
            // Under its table the panel is wide and the table wants the
            // rows, so the record folds into two columns there.
            |width, beside| self.ssid_detail_height(width, !beside),
            |frame, area| self.render_ssid_table(frame, area),
            |frame, area| self.render_ssid_detail(frame, area),
        );
    }

    fn render_clients(&self, frame: &mut Frame, area: Rect) {
        self.render_with_panel(
            frame,
            area,
            if self.client_detail_open {
                PanelPlace::Under
            } else {
                PanelPlace::Closed
            },
            |_, _| self.client_detail_height(),
            |frame, area| self.render_client_table(frame, area),
            |frame, area| self.render_client_detail(frame, area),
        );
    }

    /// Draw a table and its panel where `place` puts it, so the table
    /// stays in view either way. The panel takes the rows `wanted` says
    /// its content needs at the width it is given, beside the table or
    /// not, and no more.
    fn render_with_panel(
        &self,
        frame: &mut Frame,
        area: Rect,
        place: PanelPlace,
        wanted: impl Fn(u16, bool) -> u16,
        table: impl FnOnce(&mut Frame, Rect),
        panel: impl FnOnce(&mut Frame, Rect),
    ) {
        if place == PanelPlace::Closed {
            table(frame, area);
            return;
        }
        let roomy = area.width >= SIDE_PANEL_MIN.0 && area.height >= SIDE_PANEL_MIN.1;
        if place == PanelPlace::BesideOrUnder && roomy {
            // A column of air stands in for the frame the table lacks.
            let [left, _, right] = Layout::horizontal([
                Constraint::Percentage(56),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .areas(area);
            table(frame, left);
            let height = right.height.min(wanted(right.width, true));
            panel(frame, Rect { height, ..right });
            return;
        }
        let wanted = wanted(area.width, false);
        let rows = rows_above_panel(area.height, self.table_rows(), wanted);
        let [above, below] =
            Layout::vertical([Constraint::Length(rows), Constraint::Min(0)]).areas(area);
        if !above.is_empty() {
            table(frame, above);
        }
        let height = below.height.min(wanted);
        panel(frame, Rect { height, ..below });
    }

    /// The screen title, with the search narrowing the active table. A
    /// click on the query clears it.
    fn title(&self) -> ClickLine<'static, Click> {
        let mut title = ClickLine::default();
        title.push_gap(Span::styled(" WiFi ", theme::title_style()));
        if let Some(query) = self.active_query().filter(|query| !query.is_empty()) {
            let clear = Click::Action(Action::CloseSearch);
            title.push(
                clear.clone(),
                Span::styled(
                    format!("[\"{query}\"]"),
                    Style::default().fg(theme::warning()),
                ),
            );
            title.push(
                clear,
                Span::styled(" \u{2715} ", Style::default().fg(theme::text_muted())),
            );
        }
        title
    }

    fn tab_strip(&self) -> ClickLine<'static, Click> {
        let labels = WifiSubTab::ALL.map(WifiSubTab::label);
        let active = WifiSubTab::ALL
            .iter()
            .position(|tab| *tab == self.sub_tab)
            .unwrap_or_default();
        sub_tabs::sub_tab_strip(&labels, active)
    }

    /// What describes the active table, for the right of the toolbar,
    /// least important first: on a narrow row the first are left out.
    /// What narrows the table sits at the edge and goes last.
    fn chips(&self) -> Vec<ClickLine<'static, Click>> {
        let narrowing = self.focus_shown();
        let mut chips = Vec::new();
        if !narrowing {
            chips.extend(self.focus_chip());
        }
        chips.extend(self.band_chip());
        chips.extend(self.stale_chip());
        if self.sub_tab == WifiSubTab::Roaming
            && let Some(target) = &self.roam_target
        {
            chips.push(toolbar::chip(None, "client", target.label.clone()));
        }
        if narrowing {
            chips.extend(self.focus_chip());
        }
        chips
    }

    /// The AP the Clients and Neighbors tables are narrowed to. It shows
    /// on every sub-tab, dimmed where it narrows nothing, so the focus is
    /// never forgotten. A click clears it.
    fn focus_chip(&self) -> Option<ClickLine<'static, Click>> {
        let focused = self.focused_ap.as_ref()?;
        let clear = Click::Action(Action::WifiFocusAp(None));
        let value = if self.focus_shown() {
            theme::accent_secondary()
        } else {
            theme::text_muted()
        };
        let muted = Style::default().fg(theme::text_muted());
        let mut chip = ClickLine::default();
        chip.push(
            clear.clone(),
            Span::styled("focus ", Style::default().fg(theme::text_secondary())),
        );
        chip.push(
            clear.clone(),
            Span::styled(focused.label.clone(), Style::default().fg(value)),
        );
        chip.push(clear, Span::styled(" \u{2715}", muted));
        Some(chip)
    }

    /// The band on show, with the key that changes it where one does.
    fn band_chip(&self) -> Option<ClickLine<'static, Click>> {
        let key = match self.sub_tab {
            WifiSubTab::Neighbors if !self.session_missing() => ("f", click::char_key('f')),
            WifiSubTab::Overview if self.channel_map_open() => ("[/]", click::char_key(']')),
            _ => return None,
        };
        Some(toolbar::chip(
            Some(key),
            "band",
            self.selected_band.label().to_owned(),
        ))
    }

    /// The note that the rows on screen outlived a failed refresh, so
    /// they are not read as fresh.
    fn stale_chip(&self) -> Option<ClickLine<'static, Click>> {
        let stale = match self.sub_tab {
            WifiSubTab::Neighbors => {
                self.neighbors_error.is_some() && self.neighbors_loaded_at.is_some()
            }
            WifiSubTab::Roaming => {
                !self.roam_rows.is_empty() && self.roam_history_error().is_some()
            }
            _ => false,
        };
        stale.then(|| {
            let mut chip = ClickLine::default();
            chip.push_gap(Span::styled(
                "refresh failed",
                Style::default().fg(theme::warning()),
            ));
            chip
        })
    }

    /// One line on the state of the site's access points. It claims
    /// nothing the data does not show: an AP with no clients has no score.
    fn health_banner(&self) -> Line<'static> {
        let (message, color) = self.health_summary();
        let style = match color {
            Some(color) => Style::default()
                .fg(theme::bg_base())
                .bg(color)
                .add_modifier(Modifier::BOLD),
            None => Style::default().fg(theme::text_muted()),
        };
        Line::from(Span::styled(format!(" {message} "), style))
    }

    pub(super) fn health_summary(&self) -> (String, Option<Color>) {
        if !self.loaded.devices {
            return ("Waiting for access point data".to_owned(), None);
        }
        let total = self.all_aps().count();
        if total == 0 {
            return ("No access points on this site".to_owned(), None);
        }

        let down = self
            .all_aps()
            .filter(|ap| ap.state != DeviceState::Online)
            .count();
        let scores: Vec<u8> = self
            .all_aps()
            .filter(|ap| ap.state == DeviceState::Online)
            .filter_map(|ap| self.ap_health(ap))
            .collect();
        let poor = scores.iter().filter(|score| **score < 50).count();
        let fair = scores
            .iter()
            .filter(|score| (50..80).contains(*score))
            .count();

        let mut parts = Vec::new();
        if down > 0 {
            parts.push(format!("{down} not online"));
        }
        if poor > 0 {
            parts.push(format!("{poor} with poor client health"));
        }
        match fair {
            0 => {}
            1 => parts.push("1 needs attention".to_owned()),
            fair => parts.push(format!("{fair} need attention")),
        }
        let aps = if total == 1 {
            "access point"
        } else {
            "access points"
        };
        if !parts.is_empty() {
            let color = if down > 0 || poor > 0 {
                theme::error()
            } else {
                theme::warning()
            };
            return (
                format!("\u{25CF} {total} {aps}: {}", parts.join(", ")),
                Some(color),
            );
        }
        if scores.is_empty() {
            return (
                format!("{total} {aps} online, no client health to report yet"),
                None,
            );
        }
        (
            format!("\u{25CF} {total} {aps} online, client health good"),
            Some(theme::success()),
        )
    }
}

/// One muted line, centered in `area`, for a panel with nothing to list.
/// It wraps when the panel is narrower than the sentence.
fn render_notice(frame: &mut Frame, area: Rect, text: &str) {
    if area.is_empty() {
        return;
    }
    let width = usize::from(area.width.saturating_sub(2)).max(1);
    let lines = wrap(text, width);
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .min(area.height);
    let centered = Rect {
        y: area.y + (area.height - height) / 2,
        height,
        ..area
    };
    let lines: Vec<Line> = lines.into_iter().map(Line::from).collect();
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme::text_muted())),
        centered,
    );
}

fn panel_block<'a>(title: impl Into<Line<'a>>, focused: bool) -> Block<'a> {
    Block::default()
        .title(title)
        .title_style(theme::title_style())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(if focused {
            theme::border_focused()
        } else {
            theme::border_default()
        })
}

fn health_color(value: Option<u8>) -> Color {
    match value {
        Some(score) if score >= 80 => theme::success(),
        Some(score) if score >= 50 => theme::warning(),
        Some(_) => theme::error(),
        None => theme::text_muted(),
    }
}

fn health_label(value: Option<u8>) -> String {
    value.map_or_else(|| "─".to_string(), |score| format!("{score}%"))
}

/// What a device that is not online is doing instead.
fn state_label(state: DeviceState) -> &'static str {
    match state {
        DeviceState::Online => "online",
        DeviceState::Offline => "offline",
        DeviceState::PendingAdoption => "pending",
        DeviceState::Updating => "updating",
        DeviceState::GettingReady => "starting",
        DeviceState::Adopting => "adopting",
        DeviceState::Deleting => "removing",
        DeviceState::ConnectionInterrupted => "interrupted",
        DeviceState::Isolated => "isolated",
        _ => "unknown",
    }
}

fn state_color(state: DeviceState) -> Color {
    match state {
        DeviceState::Online => theme::success(),
        DeviceState::Offline | DeviceState::ConnectionInterrupted | DeviceState::Isolated => {
            theme::error()
        }
        _ => theme::warning(),
    }
}

fn band_label(band: WifiBand) -> &'static str {
    band.label()
}

fn signal_text(signal: Option<i32>) -> String {
    match signal {
        Some(dbm) if dbm >= -50 => "▂▄▆█".to_string(),
        Some(dbm) if dbm >= -60 => "▂▄▆ ".to_string(),
        Some(dbm) if dbm >= -70 => "▂▄  ".to_string(),
        Some(dbm) if dbm >= -80 => "▂   ".to_string(),
        Some(_) => "·   ".to_string(),
        None => "····".to_string(),
    }
}

fn signal_color(signal: Option<i32>) -> Color {
    match signal {
        Some(dbm) if dbm >= -50 => theme::success(),
        Some(dbm) if dbm >= -60 => theme::accent_secondary(),
        Some(dbm) if dbm >= -70 => theme::warning(),
        Some(dbm) if dbm >= -80 => theme::accent_tertiary(),
        Some(_) => theme::error(),
        None => theme::text_muted(),
    }
}

fn kbps_to_mbps(value: Option<u64>) -> String {
    value.map_or_else(|| "─".to_string(), |kbps| ((kbps + 500) / 1000).to_string())
}
