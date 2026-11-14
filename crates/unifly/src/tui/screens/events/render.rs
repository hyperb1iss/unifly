use chrono::{DateTime, Local, Utc};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use unifly_api::Event;
use unifly_api::model::{EventCategory, EventSeverity};

use super::punchcard::{PUNCHCARD_HEIGHT, render_punchcard};
use super::state::window_label;
use super::{CATEGORIES, EventsScreen, View};
use crate::tui::screens::link::Link;
use crate::tui::theme;
use crate::tui::widgets::click::{self, Click, Hint};
use crate::tui::widgets::sub_tabs;

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
        let view = self.view();
        let title = if self.view == View::Alarms {
            self.alarm_title()
        } else if self.filtered() {
            format!(" Events ({} of {}) ", view.len(), self.feed.len())
        } else {
            format!(" Events ({}) ", self.feed.len())
        };
        let block = Block::default()
            .title(title)
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(if self.focused {
                theme::border_focused()
            } else {
                theme::border_default()
            });

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.view == View::Alarms {
            let layout = Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(inner);
            frame.render_widget(Paragraph::new(self.alarm_status_line()), layout[0]);
            self.render_alarm_list(frame, layout[1]);
            let line =
                click::key_hints(&self.alarm_hints()).draw(layout[2], &mut self.hits.borrow_mut());
            frame.render_widget(Paragraph::new(line), layout[2]);
            return;
        }

        let detail = self
            .detail_open
            .then(|| self.detail_lines(inner.width))
            .flatten();
        // Status, filters, and hints are always drawn; the heatmap and the
        // detail pane share what is left with the list.
        let free = inner.height.saturating_sub(3);
        let detail_height = detail.as_ref().map_or(0, |lines| {
            u16::try_from(lines.len() + 2)
                .unwrap_or(u16::MAX)
                .min(free.saturating_sub(MIN_LIST_ROWS))
        });
        let heatmap_height = self.heatmap_height(free, detail_height, inner.width);
        self.heatmap_shown.set(heatmap_height > 0);

        let layout = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(heatmap_height),
            Constraint::Min(1),
            Constraint::Length(detail_height),
            Constraint::Length(1),
        ])
        .split(inner);

        frame.render_widget(Paragraph::new(self.status_line()), layout[0]);
        self.render_filters(frame, layout[1]);
        if heatmap_height > 0 {
            render_punchcard(self.feed.events(), frame, layout[2]);
        }
        self.render_event_list(frame, layout[3], &view);
        if let Some(lines) = detail
            && detail_height > 2
        {
            Self::render_detail(frame, layout[4], lines);
        }
        self.render_hints(frame, layout[5]);
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

    /// Whether the feed is live, polling, or cannot arrive at all, and how
    /// far back it reaches.
    fn status_line(&self) -> Line<'static> {
        let muted = Style::default().fg(theme::text_muted());
        let (state, color) = self.feed_state();
        let mut spans = vec![
            Span::raw("  "),
            Span::styled(state, Style::default().fg(color)),
        ];
        if self.paused && self.unseen > 0 {
            spans.push(Span::styled(
                format!("  \u{25BC} {} new", self.unseen),
                Style::default().fg(theme::accent_secondary()),
            ));
        }
        match self.active_alarms() {
            0 => {}
            1 => spans.push(Span::styled(
                "  \u{00B7}  \u{26A0} 1 alarm",
                Style::default().fg(theme::warning()),
            )),
            count => spans.push(Span::styled(
                format!("  \u{00B7}  \u{26A0} {count} alarms"),
                Style::default().fg(theme::warning()),
            )),
        }
        // A load and its outcome come before the steady facts: a narrow
        // terminal cuts the end of the line.
        if let Some(pending) = self.history.loading {
            let what = match pending.load {
                super::HistoryLoad::Older => {
                    format!("loading {} of older events", window_label(pending.hours))
                }
                super::HistoryLoad::Firewall => "loading firewall blocks".to_owned(),
            };
            spans.push(Span::styled(
                format!("  \u{00B7}  {what}\u{2026} Esc cancels"),
                Style::default().fg(theme::warning()),
            ));
        } else if let Some(note) = &self.history.note {
            spans.push(Span::styled(format!("  \u{00B7}  {note}"), muted));
        }
        // How far back the feed really reaches, not the window asked for.
        let reach = self.history_reach().map(|oldest| {
            format!(
                "back to {}",
                oldest.with_timezone(&Local).format("%m-%d %H:%M")
            )
        });
        let blocks = self.history.include_firewall.then_some("blocks");
        let covers: Vec<&str> = reach.as_deref().into_iter().chain(blocks).collect();
        if !covers.is_empty() {
            spans.push(Span::styled(
                format!("  \u{00B7}  {}", covers.join(" + ")),
                muted,
            ));
        }
        if !self.search.is_empty() {
            spans.push(Span::styled("  \u{00B7}  /", muted));
            spans.push(Span::styled(
                self.search.clone(),
                Style::default().fg(theme::accent_secondary()),
            ));
        }
        Line::from(spans)
    }

    pub(super) fn feed_state(&self) -> (&'static str, Color) {
        let Some(access) = self.access.filter(|_| self.link.reachable()) else {
            return if self.link == Link::Down {
                ("\u{25CB} not connected", theme::error())
            } else {
                ("\u{25D0} connecting", theme::warning())
            };
        };
        if !access.session {
            ("\u{25CB} no event access", theme::text_muted())
        } else if self.paused {
            ("\u{2016} paused", theme::warning())
        } else if access.live_events {
            ("\u{25CF} live", theme::success())
        } else {
            ("\u{25D0} polling (no live stream)", theme::info())
        }
    }

    /// The category strip: "All" and one chip per category.
    fn render_filters(&self, frame: &mut Frame, area: Rect) {
        let mut labels = vec!["All"];
        labels.extend(CATEGORIES.iter().map(|category| category_label(*category)));
        let active = self
            .category
            .and_then(|category| CATEGORIES.iter().position(|each| *each == category))
            .map_or(0, |index| index + 1);
        let mut strip = click::ClickLine::default();
        strip.push_gap(Span::raw("  "));
        strip.append(sub_tabs::sub_tab_strip(&labels, active));
        let line = strip.draw(area, &mut self.hits.borrow_mut());
        frame.render_widget(Paragraph::new(line), area);
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
        // A header row above and a position row below share the area.
        let rows = usize::from(area.height.saturating_sub(2)).max(1);
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

        while lines.len() < usize::from(area.height.saturating_sub(1)) {
            lines.push(Line::default());
        }
        lines.push(self.position_line(view, cursor_row));
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

    /// The row under the list: where the view is in the feed.
    fn position_line(&self, view: &[usize], cursor_row: Option<usize>) -> Line<'static> {
        let muted = Style::default().fg(theme::border_unfocused());
        let Some(row) = cursor_row else {
            return Line::from(Span::styled("  \u{2193} auto-scrolling", muted));
        };
        let mut spans = vec![Span::styled(
            format!("  event {} of {}", row + 1, view.len()),
            muted,
        )];
        if self.unseen > 0 {
            spans.push(Span::styled(
                format!("  \u{25BC} {} new below", self.unseen),
                Style::default().fg(theme::accent_secondary()),
            ));
        }
        Line::from(spans)
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

    fn render_hints(&self, frame: &mut Frame, area: Rect) {
        let pause = Hint::button(
            "Space",
            if self.paused { "resume" } else { "pause" },
            click::char_key(' '),
        );
        let hints = if self.history.loading.is_some() && !self.detail_open {
            vec![
                Hint::button("Esc", "cancel load", crossterm::event::KeyCode::Esc),
                pause,
                Hint::info("j/k", if self.paused { "move" } else { "scroll" }),
                Hint::button("Enter", "details", crossterm::event::KeyCode::Enter),
            ]
        } else if self.detail_open {
            let mut hints = vec![
                Hint::button("Esc", "close", crossterm::event::KeyCode::Esc),
                Hint::info("j/k", "move"),
            ];
            if self.selected_device_id().is_some() {
                hints.push(Hint::button("d", "device", click::char_key('d')));
            }
            if self.selected_client_id().is_some() {
                hints.push(Hint::button("c", "client", click::char_key('c')));
            }
            hints.push(pause);
            hints
        } else {
            vec![
                pause,
                Hint::info("j/k", if self.paused { "move" } else { "scroll" }),
                Hint::button("Enter", "details", crossterm::event::KeyCode::Enter),
                Hint::info("h/l", "category"),
                Hint::button("o", "older", click::char_key('o')),
                Hint::button("v", "alarms", click::char_key('v')),
            ]
        };
        let line = click::key_hints(&hints).draw(area, &mut self.hits.borrow_mut());
        frame.render_widget(Paragraph::new(line), area);
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
