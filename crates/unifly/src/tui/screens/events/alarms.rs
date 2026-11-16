//! The Alarms view: the controller's unarchived alarms, with archiving.

use std::sync::Arc;

use chrono::Local;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use unifly_api::Command;
use unifly_api::model::Alarm;

use super::render::{category_label, clip, severity_color, severity_label};
use super::{EventsScreen, View};
use crate::tui::action::{Action, ConfirmRequest, Notice, Notification};
use crate::tui::theme;
use crate::tui::widgets::click::{self, Click, Hint};

/// Columns of the list's time, severity, and category cells.
const TIME_COLUMNS: usize = 16;
const SEVERITY_COLUMNS: usize = 10;
const CATEGORY_COLUMNS: usize = 10;

/// The list's columns for one frame.
struct Columns {
    /// Zero on a screen too narrow for the category beside every message
    /// it shows whole: an alarm cut short says less than one without its
    /// category.
    category: usize,
    message: usize,
}

impl Columns {
    fn plan(shown: &[&Alarm], width: u16) -> Self {
        let longest = shown
            .iter()
            .map(|alarm| Span::raw(alarm.message.as_str()).width())
            .max()
            .unwrap_or(0);
        let room = usize::from(width).saturating_sub(2 + TIME_COLUMNS + SEVERITY_COLUMNS);
        let category = if longest + CATEGORY_COLUMNS <= room {
            CATEGORY_COLUMNS
        } else {
            0
        };
        Self {
            category,
            message: room - category,
        }
    }

    /// One row: the cursor marker, then the time, severity, category,
    /// and message cells in their styles. With `fill` the message cell
    /// runs to the edge, which a selected row's highlight needs.
    fn line(&self, cells: [&str; 5], styles: [Style; 4], fill: bool) -> Line<'static> {
        let [marker, time, severity, category, message] = cells;
        let category_columns = self.category;
        let category = if category_columns == 0 { "" } else { category };
        let message = clip(message, self.message);
        let message_columns = if fill { self.message } else { 0 };
        Line::from(vec![
            Span::styled(format!("{marker}{time:<TIME_COLUMNS$}"), styles[0]),
            Span::styled(format!("{severity:<SEVERITY_COLUMNS$}"), styles[1]),
            Span::styled(format!("{category:<category_columns$}"), styles[2]),
            Span::styled(format!("{message:<message_columns$}"), styles[3]),
        ])
    }
}

/// The alarms the controller last reported, and the cursor over the
/// unarchived ones.
#[derive(Debug, Default)]
pub(super) struct AlarmList {
    alarms: Arc<Vec<Alarm>>,
    /// Row of the active list the cursor is on.
    cursor: usize,
    /// A read is in flight.
    loading: bool,
    /// Whether any read has come back.
    loaded: bool,
    error: Option<String>,
}

impl AlarmList {
    /// Unarchived alarms, newest first.
    pub(super) fn active(&self) -> Vec<&Alarm> {
        let mut active: Vec<&Alarm> = self.alarms.iter().filter(|alarm| !alarm.archived).collect();
        active.sort_by_key(|alarm| std::cmp::Reverse(alarm.timestamp));
        active
    }

    fn archived(&self) -> usize {
        self.alarms.iter().filter(|alarm| alarm.archived).count()
    }

    pub(super) fn loaded(&mut self, alarms: &Arc<Vec<Alarm>>) {
        // The cursor stays on its alarm when the list changes under it.
        let held = self.active().get(self.cursor).map(|alarm| alarm.id.clone());
        self.alarms = Arc::clone(alarms);
        self.loading = false;
        self.loaded = true;
        self.error = None;
        let active = self.active();
        self.cursor = held
            .and_then(|id| active.iter().position(|alarm| alarm.id == id))
            .unwrap_or(self.cursor)
            .min(active.len().saturating_sub(1));
    }

    pub(super) fn failed(&mut self, message: &str) {
        self.loading = false;
        self.error = Some(message.to_owned());
    }

    fn step(&mut self, delta: isize) {
        let last = self.active().len().saturating_sub(1);
        self.cursor = self.cursor.saturating_add_signed(delta).min(last);
    }
}

impl EventsScreen {
    /// Switch between the event feed and the Alarms view. Opening the
    /// view reads the alarms again.
    pub(super) fn toggle_alarms(&mut self) -> Action {
        if self.view == View::Alarms {
            self.view = View::Feed;
            return Action::Invalidate;
        }
        self.view = View::Alarms;
        self.reload_alarms()
    }

    fn reload_alarms(&mut self) -> Action {
        if !self.access.is_some_and(|access| access.session) {
            return Action::Invalidate;
        }
        self.alarms.loading = true;
        Action::RequestAlarms
    }

    pub(super) fn handle_alarm_key(&mut self, key: KeyEvent) -> Option<Action> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let page = isize::try_from(self.page.get().max(1)).unwrap_or(10);
        match key.code {
            KeyCode::Char('v') | KeyCode::Esc => return Some(self.toggle_alarms()),
            KeyCode::Char('j') | KeyCode::Down => self.alarms.step(1),
            KeyCode::Char('k') | KeyCode::Up => self.alarms.step(-1),
            KeyCode::Char('d') if ctrl => self.alarms.step(page),
            KeyCode::Char('u') if ctrl => self.alarms.step(-page),
            KeyCode::Char('g') => self.alarms.cursor = 0,
            KeyCode::Char('G') => self.alarms.step(isize::MAX),
            KeyCode::Char('r') => return Some(self.reload_alarms()),
            KeyCode::Char('x') => return self.archive_selected(),
            KeyCode::Char('X') => return self.archive_all(),
            KeyCode::Enter => return self.open_alarm_device(),
            _ => return None,
        }
        Some(Action::Invalidate)
    }

    /// Ask before archiving the alarm under the cursor.
    fn archive_selected(&self) -> Option<Action> {
        let active = self.alarms.active();
        let alarm = active.get(self.alarms.cursor)?;
        Some(Action::ShowConfirm(
            ConfirmRequest::new(
                "Archive this alarm?",
                Action::Run {
                    command: Box::new(Command::ArchiveAlarm {
                        id: alarm.id.clone(),
                    }),
                    notice: Notice::new("Archiving the alarm\u{2026}", "Alarm archived"),
                },
            )
            // A newer controller lists alerts by group and archives the
            // group, so the row may stand for more than one alert.
            .detail(format!(
                "{} (alerts the controller groups with it are archived too)",
                alarm.message
            )),
        ))
    }

    /// Ask before archiving every alarm.
    fn archive_all(&self) -> Option<Action> {
        let count = self.alarms.active().len();
        if count == 0 {
            return None;
        }
        let prompt = if count == 1 {
            "Archive the 1 active alarm?".to_owned()
        } else {
            format!("Archive all {count} active alarms?")
        };
        Some(Action::ShowConfirm(
            ConfirmRequest::new(
                prompt,
                Action::Run {
                    command: Box::new(Command::ArchiveAllAlarms),
                    notice: Notice::new("Archiving every alarm\u{2026}", "All alarms archived"),
                },
            )
            .detail("They leave this list. The controller keeps them as archived."),
        ))
    }

    /// Open the device the alarm under the cursor names.
    fn open_alarm_device(&self) -> Option<Action> {
        // With no alarm under the cursor there is nothing to say.
        self.alarms.active().get(self.alarms.cursor)?;
        Some(match self.alarm_device() {
            Some(device) => Action::ShowDevice(device.id.clone()),
            None => Action::Notify(Notification::info("This alarm names no known device")),
        })
    }

    /// A tap takes the cursor to a row; a tap on the cursor's row opens
    /// that alarm's device, like `Enter`.
    pub(super) fn click_alarm_row(&mut self, row: usize) -> Option<Action> {
        if self.alarms.cursor == row {
            return self.open_alarm_device();
        }
        self.alarms.cursor = row.min(self.alarms.active().len().saturating_sub(1));
        Some(Action::Invalidate)
    }

    /// How many alarms are unarchived, for the feed's alarm chip.
    pub(super) fn active_alarms(&self) -> usize {
        self.alarms.active().len()
    }

    pub(super) fn alarm_title(&self) -> String {
        format!(" Alarms ({}) ", self.alarms.active().len())
    }

    /// How many alarms the controller holds as archived.
    pub(super) fn archived_alarms(&self) -> usize {
        self.alarms.archived()
    }

    /// A read under way, or the last one's failure cut to `room` columns.
    pub(super) fn alarm_read_status(&self, room: usize) -> Option<Span<'static>> {
        if self.alarms.loading {
            return Some(Span::styled(
                "loading\u{2026}",
                Style::default().fg(theme::warning()),
            ));
        }
        let error = self.alarms.error.as_ref()?;
        Some(Span::styled(
            clip(&format!("could not read alarms: {error}"), room),
            Style::default().fg(theme::error()),
        ))
    }

    /// The device the alarm under the cursor names, when it is still on
    /// the controller.
    fn alarm_device(&self) -> Option<&Arc<unifly_api::Device>> {
        let active = self.alarms.active();
        let mac = active.get(self.alarms.cursor)?.device_mac.as_ref()?;
        self.devices.iter().find(|device| &device.mac == mac)
    }

    /// One centered line saying why no alarm is listed.
    fn render_no_alarms(&self, frame: &mut Frame, area: Rect) {
        let message = if let Some(waiting) = self.link.waiting() {
            waiting
        } else if !self.access.is_some_and(|access| access.session) {
            "Alarms need a direct controller session; cloud profiles cannot read them"
        } else if self.alarms.error.is_some() {
            "Alarms could not be read. Press r to try again."
        } else if self.alarms.loaded {
            "No active alarms"
        } else {
            "Loading alarms\u{2026}"
        };
        let row = Rect {
            y: area.y + area.height / 2,
            height: area.height.min(1),
            ..area
        };
        frame.render_widget(
            Paragraph::new(Span::styled(
                message,
                Style::default().fg(theme::text_muted()),
            ))
            .alignment(Alignment::Center),
            row,
        );
    }

    pub(super) fn render_alarm_list(&self, frame: &mut Frame, area: Rect) {
        let active = self.alarms.active();
        if active.is_empty() {
            self.render_no_alarms(frame, area);
            return;
        }

        let rows = usize::from(area.height.saturating_sub(1)).max(1);
        self.page.set(rows);
        let cursor = self.alarms.cursor.min(active.len() - 1);
        let first = self
            .top
            .get()
            .clamp((cursor + 1).saturating_sub(rows), cursor)
            .min(active.len().saturating_sub(rows));
        self.top.set(first);
        let shown = &active[first..active.len().min(first + rows)];
        let columns = Columns::plan(shown, area.width);

        let mut lines = vec![columns.line(
            ["  ", "Time", "Severity", "Category", "Message"],
            [theme::table_header(); 4],
            false,
        )];
        let mut hits = self.hits.borrow_mut();
        for (row, alarm) in (first..).zip(shown) {
            let selected = row == cursor;
            let time = alarm
                .timestamp
                .with_timezone(&Local)
                .format("%m-%d %H:%M:%S")
                .to_string();
            let severity = Style::default().fg(severity_color(alarm.severity));
            let line = columns.line(
                [
                    if selected { "\u{25B8} " } else { "  " },
                    &time,
                    severity_label(alarm.severity),
                    category_label(alarm.category),
                    &alarm.message,
                ],
                [
                    Style::default().fg(theme::warning()),
                    severity,
                    Style::default().fg(theme::text_secondary()),
                    severity,
                ],
                selected,
            );
            lines.push(if selected {
                line.style(theme::table_selected())
            } else {
                line
            });
            hits.push(click::paragraph_row(area, lines.len() - 1), Click::Row(row));
        }
        drop(hits);
        frame.render_widget(Paragraph::new(lines), area);
    }

    /// What the status bar offers: with no active alarm there is nothing
    /// to archive or open, and `Enter` is offered only for an alarm whose
    /// device can be opened.
    pub(super) fn alarm_hints(&self) -> Vec<(u8, Hint)> {
        let mut hints = Vec::new();
        if self.alarm_device().is_some() {
            hints.push((0, Hint::button("Enter", "device", KeyCode::Enter)));
        }
        if self.active_alarms() > 0 {
            hints.extend([
                (1, Hint::button("x", "archive", click::char_key('x'))),
                (2, Hint::button("X", "archive all", click::char_key('X'))),
            ]);
        }
        hints.extend([
            (3, Hint::button("r", "reload", click::char_key('r'))),
            (0, Hint::button("v", "events", click::char_key('v'))),
        ]);
        hints
    }

    pub(super) fn alarm_help(&self) -> Vec<(&'static str, &'static str)> {
        let mut keys = vec![("v  Esc", "Back to the event feed")];
        if self.active_alarms() > 0 {
            keys.extend_from_slice(crate::tui::component::LIST_HELP);
            keys.extend_from_slice(&[
                ("x", "Archive the selected alarm"),
                ("X", "Archive every alarm"),
                ("Enter", "Open the alarm's device"),
            ]);
        }
        keys.push(("r", "Read the alarms again"));
        keys
    }
}
