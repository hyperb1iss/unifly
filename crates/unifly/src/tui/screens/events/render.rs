use chrono::{DateTime, Local, Utc};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use unifly_api::Event;
use unifly_api::model::{EventCategory, EventSeverity};

use super::punchcard::{PUNCHCARD_HEIGHT, render_punchcard};
use super::{EventsScreen, View};
use crate::tui::theme;
use crate::tui::widgets::click::{self, Click};

/// List rows the heatmap must leave for it to show unasked.
const ROOMY_LIST_ROWS: u16 = 12;
/// List rows the heatmap must leave for it to show at all.
const MIN_LIST_ROWS: u16 = 4;
/// Columns of the category cell, sized to "Firewall" and a gap.
const CATEGORY_COLUMNS: usize = 10;

/// A category as the filter strip and the rows name it.
pub(super) fn category_label(category: EventCategory) -> &'static str {
    match category {
        EventCategory::Device => "Device",
        EventCategory::Client => "Client",
        EventCategory::Network => "Network",
        EventCategory::System => "System",
        EventCategory::Admin => "Admin",
        EventCategory::Firewall => "Firewall",
        EventCategory::Vpn => "VPN",
        _ => "Other",
    }
}

pub(super) fn severity_label(severity: EventSeverity) -> &'static str {
    match severity {
        EventSeverity::Info => "Info",
        EventSeverity::Warning => "Warning",
        EventSeverity::Error => "Error",
        EventSeverity::Critical => "Critical",
        _ => "Unknown",
    }
}

pub(super) fn severity_color(severity: EventSeverity) -> Color {
    match severity {
        EventSeverity::Error | EventSeverity::Critical => theme::error(),
        EventSeverity::Warning => theme::warning(),
        EventSeverity::Info => theme::accent_secondary(),
        _ => theme::text_secondary(),
    }
}

/// `text` cut to `width` columns, with an ellipsis when it was cut.
pub(in crate::tui::screens) fn clip(text: &str, width: usize) -> String {
    if Span::raw(text).width() <= width {
        return text.to_owned();
    }
    let mut clipped = String::new();
    let mut used = 0;
    let mut buffer = [0_u8; 4];
    for ch in text.chars() {
        let columns = Span::raw(&*ch.encode_utf8(&mut buffer)).width();
        if used + columns + 1 > width {
            break;
        }
        used += columns;
        clipped.push(ch);
    }
    if width > 0 {
        clipped.push('\u{2026}');
    }
    clipped
}

fn local_date(timestamp: DateTime<Utc>) -> chrono::NaiveDate {
    timestamp.with_timezone(&Local).date_naive()
}

impl EventsScreen {
    pub(super) fn render_screen(&self, frame: &mut Frame, area: Rect) {
        self.hits.borrow_mut().clear();
        let block = Block::default()
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(if self.focused {
                theme::border_focused()
            } else {
                theme::border_default()
            });
        let inner = block.inner(area);
        // Room on a border between its corners, for what rides it.
        let border = usize::from(inner.width);

        if self.view == View::Alarms {
            let title = Line::from(self.alarm_title());
            let status = self.alarm_status_line(border.saturating_sub(title.width() + 2));
            frame.render_widget(block.title(title).title(status), area);
            self.render_alarm_list(frame, inner);
            return;
        }

        let view = self.view();
        self.place_chips(inner.width);
        let title = self.feed_title(view.len());
        let history = self.history_line(border.saturating_sub(title.width() + 2));
        let block = block
            .title(title)
            .title(history)
            .title_bottom(self.position_line(&view));
        frame.render_widget(block, area);

        let detail = self
            .detail_open
            .then(|| self.detail_lines(inner.width))
            .flatten();
        // The toolbar is always drawn; the heatmap and the detail pane
        // share what is left with the list.
        let free = inner.height.saturating_sub(1);
        let detail_height = detail.as_ref().map_or(0, |lines| {
            u16::try_from(lines.len() + 2)
                .unwrap_or(u16::MAX)
                .min(free.saturating_sub(MIN_LIST_ROWS))
        });
        let heatmap_height = self.heatmap_height(free, detail_height, inner.width);
        self.heatmap_shown.set(heatmap_height > 0);

        let layout = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(heatmap_height),
            Constraint::Min(1),
            Constraint::Length(detail_height),
        ])
        .split(inner);

        self.render_toolbar(frame, layout[0]);
        if heatmap_height > 0 {
            render_punchcard(self.feed.events(), frame, layout[1]);
        }
        self.render_event_list(frame, layout[2], &view);
        if let Some(lines) = detail
            && detail_height > 2
        {
            Self::render_detail(frame, layout[3], lines);
        }
    }

    /// Rows the heatmap takes: all of them when it is wanted and the list
    /// keeps enough room beside it, else none.
    fn heatmap_height(&self, free: u16, detail_height: u16, width: u16) -> u16 {
        let left = free.saturating_sub(detail_height);
        let list_rows = left.saturating_sub(PUNCHCARD_HEIGHT);
        let fits = left > PUNCHCARD_HEIGHT && list_rows >= MIN_LIST_ROWS && width >= 48;
        self.heatmap_fits.set(fits);
        let wanted = self
            .heatmap
            .unwrap_or(list_rows >= ROOMY_LIST_ROWS && detail_height == 0);
        if fits && wanted { PUNCHCARD_HEIGHT } else { 0 }
    }

    /// Whether every event in the feed happened today, so the time of day
    /// alone places it.
    fn all_today(&self) -> bool {
        let today = Local::now().date_naive();
        let events = self.feed.events();
        events
            .front()
            .zip(events.back())
            .is_none_or(|(first, last)| {
                local_date(first.timestamp) == today && local_date(last.timestamp) == today
            })
    }

    fn render_event_list(&self, frame: &mut Frame, area: Rect, view: &[usize]) {
        if view.is_empty() {
            self.render_empty(frame, area);
            return;
        }
        let (time_format, time_columns) = if self.all_today() {
            ("%H:%M:%S", 10)
        } else {
            ("%m-%d %H:%M:%S", 16)
        };
        // A header row above shares the area.
        let rows = usize::from(area.height.saturating_sub(1)).max(1);
        self.page.set(rows);
        let first = self.first_row(view, rows);
        let cursor_row = self.paused.then(|| self.cursor_row(view));
        let width = usize::from(area.width);
        let message_columns = width.saturating_sub(2 + time_columns + CATEGORY_COLUMNS);

        let mut lines = vec![Line::from(vec![
            Span::styled(
                format!("  {:<time_columns$}", "Time"),
                theme::table_header(),
            ),
            Span::styled(
                format!("{:<CATEGORY_COLUMNS$}", "Category"),
                theme::table_header(),
            ),
            Span::styled("Message", theme::table_header()),
        ])];

        let mut hits = self.hits.borrow_mut();
        for (row, index) in view.iter().enumerate().skip(first).take(rows) {
            let Some(event) = self.feed.get(*index) else {
                continue;
            };
            let selected = cursor_row == Some(row);
            lines.push(event_row(
                event,
                selected,
                (time_format, time_columns),
                message_columns,
            ));
            hits.push(click::paragraph_row(area, lines.len() - 1), Click::Row(row));
        }
        drop(hits);
        frame.render_widget(Paragraph::new(lines), area);
    }

    /// The first row of `view` to draw. A live feed ends on its newest
    /// event; a paused one keeps its place and only scrolls to keep the
    /// cursor on screen, never past either end.
    fn first_row(&self, view: &[usize], rows: usize) -> usize {
        let last_first = view.len().saturating_sub(rows);
        let first = if self.paused {
            // The row the anchored event is on now, wherever arrivals and
            // evictions have moved it.
            let cursor = self.cursor_row(view);
            view.partition_point(|index| *index < self.anchor.get())
                .clamp((cursor + 1).saturating_sub(rows), cursor)
                .min(last_first)
        } else {
            last_first
        };
        self.top.set(first);
        self.anchor.set(view.get(first).copied().unwrap_or(0));
        first
    }

    /// One centered line saying why the list is empty.
    fn render_empty(&self, frame: &mut Frame, area: Rect) {
        let message = if !self.feed.is_empty() {
            "No events match the filter"
        } else if let Some(waiting) = self.link.waiting() {
            waiting
        } else if self.access.is_none() {
            "Connecting\u{2026}"
        } else if !self.access.is_some_and(|access| access.session) {
            "Events need a direct controller session; cloud profiles cannot read them"
        } else {
            "No events yet. New ones appear here as they happen."
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
}

fn event_row(
    event: &Event,
    selected: bool,
    (time_format, time_columns): (&str, usize),
    message_columns: usize,
) -> Line<'static> {
    let time = event
        .timestamp
        .with_timezone(&Local)
        .format(time_format)
        .to_string();
    let marker = if selected { "\u{25B8} " } else { "  " };
    let message = clip(&event.message, message_columns);
    let line = Line::from(vec![
        Span::styled(
            format!("{marker}{time:<time_columns$}"),
            Style::default().fg(theme::warning()),
        ),
        Span::styled(
            format!("{:<CATEGORY_COLUMNS$}", category_label(event.category)),
            Style::default().fg(theme::text_secondary()),
        ),
        Span::styled(
            format!("{message:<message_columns$}"),
            Style::default().fg(severity_color(event.severity)),
        ),
    ]);
    if selected {
        line.style(theme::table_selected())
    } else {
        line
    }
}
