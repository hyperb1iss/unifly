//! Toasts and the notification history behind them.
//!
//! Toasts stack in the top-right corner, wrap instead of clipping, and
//! never replace one another, so an error cannot be wiped by the success
//! that lands a moment later. A command in flight shows a progress toast
//! that its result takes over. Everything shown is kept in a short history
//! the `n` key opens.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use super::App;
use crate::tui::action::{Action, Notification, NotificationLevel};
use crate::tui::theme;
use crate::tui::widgets::click::Click;
use crate::tui::widgets::wrap::{wrap, wrap_clamped};

/// Toasts drawn at once; older ones wait in the history.
const MAX_VISIBLE: usize = 4;
/// Notifications the history keeps.
const HISTORY: usize = 50;
/// Text rows one toast may wrap to.
const MAX_ROWS: u16 = 5;
/// Rows the stack starts below the top of its area: the screen's border
/// and its toolbar row.
const TOAST_TOP: u16 = 2;
/// A progress toast nobody finished is dropped after this long.
const PROGRESS_LIMIT: Duration = Duration::from_secs(90);

fn ttl(level: NotificationLevel) -> Duration {
    match level {
        NotificationLevel::Info | NotificationLevel::Success => Duration::from_secs(3),
        NotificationLevel::Warning => Duration::from_secs(8),
        NotificationLevel::Error => Duration::from_secs(10),
    }
}

#[derive(Debug)]
struct Toast {
    id: u64,
    notification: Notification,
    shown: Instant,
    /// Whether this is a command still running: it stays until finished.
    progress: bool,
}

impl Toast {
    fn expired(&self) -> bool {
        let limit = if self.progress {
            PROGRESS_LIMIT
        } else {
            ttl(self.notification.level)
        };
        self.shown.elapsed() > limit
    }

    fn sticky(&self) -> bool {
        !self.progress
            && matches!(
                self.notification.level,
                NotificationLevel::Warning | NotificationLevel::Error
            )
    }
}

/// One line of the notification history.
#[derive(Debug)]
struct Past {
    notification: Notification,
    at: chrono::DateTime<chrono::Local>,
}

/// The toasts on screen and the history of everything shown.
#[derive(Debug, Default)]
pub(super) struct Notices {
    toasts: VecDeque<Toast>,
    history: VecDeque<Past>,
    next_id: u64,
}

impl Notices {
    fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn remember(&mut self, notification: &Notification) {
        if matches!(
            notification.level,
            NotificationLevel::Warning | NotificationLevel::Error
        ) {
            tracing::warn!(message = %notification.message, "notification");
        }
        self.history.push_front(Past {
            notification: notification.clone(),
            at: chrono::Local::now(),
        });
        self.history.truncate(HISTORY);
    }

    /// Show a toast. One that repeats a toast still on screen restarts its
    /// timer instead of stacking a copy.
    pub(super) fn show(&mut self, notification: Notification) {
        if let Some(same) = self.toasts.iter_mut().find(|toast| {
            !toast.progress
                && toast.notification.message == notification.message
                && toast.notification.level == notification.level
        }) {
            same.shown = Instant::now();
            return;
        }
        self.remember(&notification);
        let id = self.next_id();
        self.push(Toast {
            id,
            notification,
            shown: Instant::now(),
            progress: false,
        });
    }

    fn push(&mut self, toast: Toast) {
        self.toasts.push_back(toast);
        while self.toasts.len() > MAX_VISIBLE {
            // Drop the oldest toast that is not an unread warning or error,
            // and the oldest of all when every one is. The toast that just
            // arrived is never the one to go.
            let older = self.toasts.len() - 1;
            let drop = self
                .toasts
                .iter()
                .take(older)
                .position(|toast| !toast.sticky())
                .unwrap_or(0);
            self.toasts.remove(drop);
        }
    }

    /// Show a progress toast for work that just started, and return the id
    /// its result is delivered under.
    pub(super) fn begin(&mut self, message: String) -> u64 {
        let id = self.next_id();
        self.push(Toast {
            id,
            notification: Notification {
                message,
                level: NotificationLevel::Info,
            },
            shown: Instant::now(),
            progress: true,
        });
        id
    }

    /// Replace progress toast `id` with its result, in place. A result
    /// whose progress toast is already gone shows as a new toast.
    pub(super) fn finish(&mut self, id: Option<u64>, notification: Notification) {
        let slot = id.and_then(|id| self.toasts.iter_mut().find(|toast| toast.id == id));
        match slot {
            Some(toast) => {
                toast.notification = notification.clone();
                toast.shown = Instant::now();
                toast.progress = false;
                self.remember(&notification);
            }
            None => self.show(notification),
        }
    }

    pub(super) fn dismiss(&mut self, id: u64) {
        self.toasts.retain(|toast| toast.id != id);
    }

    /// Dismiss every warning and error toast.
    pub(super) fn dismiss_sticky(&mut self) {
        self.toasts.retain(|toast| !toast.sticky());
    }

    pub(super) fn has_sticky(&self) -> bool {
        self.toasts.iter().any(Toast::sticky)
    }

    /// Drop toasts whose time is up. Returns whether any went.
    pub(super) fn expire(&mut self) -> bool {
        let before = self.toasts.len();
        self.toasts.retain(|toast| !toast.expired());
        self.toasts.len() != before
    }

    pub(super) fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    /// The newest toast's text, for tests.
    #[cfg(test)]
    pub(super) fn latest(&self) -> Option<&Notification> {
        self.toasts.back().map(|toast| &toast.notification)
    }

    #[cfg(test)]
    pub(super) fn messages(&self) -> Vec<&str> {
        self.toasts
            .iter()
            .map(|toast| toast.notification.message.as_str())
            .collect()
    }
}

fn level_style(level: NotificationLevel, progress: bool) -> (ratatui::style::Color, &'static str) {
    if progress {
        return (theme::accent_secondary(), "\u{25D0}");
    }
    match level {
        NotificationLevel::Success => (theme::success(), "\u{2713}"),
        NotificationLevel::Error => (theme::error(), "\u{2717}"),
        NotificationLevel::Warning => (theme::warning(), "!"),
        NotificationLevel::Info => (theme::accent_secondary(), "\u{00B7}"),
    }
}

fn text_lines(lines: Vec<String>) -> Vec<Line<'static>> {
    lines.into_iter().map(Line::from).collect()
}

impl App {
    /// Show a toast. Every toast passes through here, so its text is
    /// sanitized once for all of them.
    pub(super) fn show_notification(&mut self, notification: Notification) {
        let Notification { message, level } = notification;
        let message = self.shown_text(&message);
        self.notices.show(Notification { message, level });
    }

    /// Start a progress toast and return the id to finish it with.
    pub(super) fn begin_progress(&mut self, message: &str) -> u64 {
        let message = self.shown_text(message);
        self.notices.begin(message)
    }

    pub(super) fn finish_progress(&mut self, id: Option<u64>, notification: &Notification) {
        let message = self.shown_text(&notification.message);
        self.notices.finish(
            id,
            Notification {
                message,
                level: notification.level,
            },
        );
    }

    pub(super) fn has_sticky_notification(&self) -> bool {
        self.notices.has_sticky()
    }

    /// Draw the toast stack down the top-right corner, newest at the
    /// bottom, under the screen's toolbar row, whose right end says what
    /// the view is showing, and clear of the bars below. A tap on a toast
    /// dismisses it.
    pub(super) fn render_toasts(&mut self, frame: &mut Frame, area: Rect) {
        if self.notices.is_empty() || area.width < 12 || area.height < 4 {
            return;
        }
        let width = area.width.saturating_sub(4).min(64);
        let text_width = width.saturating_sub(6);
        let x = area.right().saturating_sub(width.saturating_add(2));
        let mut y = area.y.saturating_add(TOAST_TOP);
        let floor = area.bottom().saturating_sub(1);

        // When the stack is taller than the screen the newest toasts are
        // the ones drawn: a result must not hide under older errors.
        let wrapped: Vec<Vec<String>> = self
            .notices
            .toasts
            .iter()
            .map(|toast| {
                wrap_clamped(
                    &toast.notification.message,
                    usize::from(text_width),
                    usize::from(MAX_ROWS),
                )
            })
            .collect();
        let mut room = floor.saturating_sub(y);
        let mut first = wrapped.len();
        for lines in wrapped.iter().rev() {
            let height = u16::try_from(lines.len())
                .unwrap_or(MAX_ROWS)
                .saturating_add(2);
            if height > room {
                break;
            }
            room -= height;
            first -= 1;
        }

        for (toast, lines) in self.notices.toasts.iter().zip(wrapped).skip(first) {
            let rows = u16::try_from(lines.len()).unwrap_or(MAX_ROWS);
            let height = rows.saturating_add(2);
            // A short message gets a toast as wide as it needs.
            let longest = lines.iter().map(|line| Line::from(line.as_str()).width());
            let needed = u16::try_from(longest.max().unwrap_or(0))
                .unwrap_or(u16::MAX)
                .saturating_add(6);
            let toast_width = needed.clamp(18.min(width), width);
            let rect = Rect::new(
                x.saturating_add(width - toast_width),
                y,
                toast_width,
                height,
            )
            .intersection(area);
            self.hits
                .push(rect, Click::Action(Action::DismissToast(toast.id)));

            let (color, icon) = level_style(toast.notification.level, toast.progress);
            frame.render_widget(Clear, rect);
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(color))
                .style(Style::default().bg(theme::bg_base()));
            let inner = block.inner(rect);
            frame.render_widget(block, rect);

            let icon_area = Rect {
                width: 3.min(inner.width),
                ..inner
            };
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!(" {icon} "),
                    Style::default().fg(color),
                )),
                icon_area,
            );
            let text_area = Rect {
                x: inner.x.saturating_add(3),
                width: inner.width.saturating_sub(4),
                ..inner
            };
            frame.render_widget(
                Paragraph::new(text_lines(lines))
                    .style(Style::default().fg(theme::text_secondary())),
                text_area,
            );
            y = y.saturating_add(height);
        }
    }

    /// Draw the notification history: everything shown this session,
    /// newest first, in full.
    pub(super) fn render_notices_overlay(&mut self, frame: &mut Frame, area: Rect) {
        let width = 76u16.min(area.width.saturating_sub(4));
        let text_width = usize::from(width.saturating_sub(16));
        let wanted: usize = self
            .notices
            .history
            .iter()
            .map(|past| wrap(&past.notification.message, text_width).len())
            .sum();
        let height = u16::try_from(wanted.max(1) + 2)
            .unwrap_or(u16::MAX)
            .min(area.height.saturating_sub(4));
        let overlay = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        Self::register_modal(&mut self.hits, area, overlay);

        frame.render_widget(Clear, overlay);
        let block = Block::default()
            .title(" Notifications ")
            .title_style(theme::title_style())
            .title_bottom(Line::from(Span::styled(
                " Esc or n to close ",
                theme::key_hint(),
            )))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_focused())
            .style(Style::default().bg(theme::bg_base()));
        let inner = block.inner(overlay);
        frame.render_widget(block, overlay);

        if self.notices.history.is_empty() {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "  Nothing yet. Results and errors from this session land here.",
                    theme::key_hint(),
                )),
                inner,
            );
            return;
        }

        let text_width = inner.width.saturating_sub(14);
        let mut y = inner.y;
        for past in &self.notices.history {
            let lines = wrap(&past.notification.message, usize::from(text_width));
            let rows = u16::try_from(lines.len()).unwrap_or(u16::MAX);
            if y.saturating_add(rows) > inner.bottom() {
                break;
            }
            let (color, icon) = level_style(past.notification.level, false);
            let stamp = Line::from(vec![
                Span::styled(
                    format!(" {} ", past.at.format("%H:%M:%S")),
                    theme::key_hint(),
                ),
                Span::styled(format!("{icon} "), Style::default().fg(color)),
            ]);
            frame.render_widget(Paragraph::new(stamp), Rect::new(inner.x, y, 12, 1));
            frame.render_widget(
                Paragraph::new(text_lines(lines))
                    .style(Style::default().fg(theme::text_secondary())),
                Rect::new(inner.x.saturating_add(12), y, text_width, rows),
            );
            y = y.saturating_add(rows);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::screen::ScreenId;
    use crate::tui::table::render_lines;

    fn app() -> App {
        let mut app = App::new(None, None, false, true);
        app.active_screen = ScreenId::Dashboard;
        app
    }

    #[test]
    fn a_long_error_wraps_instead_of_losing_its_reason() {
        let mut app = app();
        let reason = "Validation failed: IPv4 and DHCP server settings require a \
                      gateway- or switch-managed network";
        app.show_notification(Notification::error(reason));
        let text = render_lines(80, 24, |frame, _| app.render(frame)).join("\n");
        for word in ["Validation", "gateway-", "switch-managed", "network"] {
            assert!(text.contains(word), "{word} missing:\n{text}");
        }
    }

    #[test]
    fn an_error_survives_the_success_that_lands_after_it() {
        let mut app = app();
        app.show_notification(Notification::error("stats failed"));
        app.show_notification(Notification::success("Restart sent to sw-core"));
        assert_eq!(
            app.notices.messages(),
            ["stats failed", "Restart sent to sw-core"]
        );

        // Past the cap, routine toasts go before an unread error does.
        for index in 0..MAX_VISIBLE {
            app.show_notification(Notification::info(format!("info {index}")));
        }
        assert_eq!(app.notices.toasts.len(), MAX_VISIBLE);
        assert!(app.notices.messages().contains(&"stats failed"));
    }

    #[test]
    fn the_newest_toast_is_never_the_one_dropped_or_hidden() {
        let mut app = app();
        for index in 0..MAX_VISIBLE {
            app.show_notification(Notification::error(format!(
                "error {index}: a long reason that wraps over several rows of the toast so \
                 that four of them cannot all fit on a short terminal at the same time"
            )));
        }
        app.show_notification(Notification::success("Restart sent to sw-core"));
        assert_eq!(
            app.notices.messages().last().copied(),
            Some("Restart sent to sw-core"),
            "the arriving toast was evicted"
        );

        let text = render_lines(80, 16, |frame, _| app.render(frame)).join("\n");
        assert!(text.contains("Restart sent to sw-core"), "{text}");
    }

    #[test]
    fn a_repeated_toast_restarts_instead_of_stacking() {
        let mut app = app();
        for _ in 0..3 {
            app.show_notification(Notification::error("WiFi neighbors: no session"));
        }
        assert_eq!(app.notices.toasts.len(), 1);
        assert_eq!(app.notices.history.len(), 1);
    }

    #[test]
    fn a_result_takes_over_its_progress_toast() {
        let mut app = app();
        let id = app.begin_progress("Restarting sw-core\u{2026}");
        assert!(!app.has_sticky_notification());
        app.finish_progress(Some(id), &Notification::error("sw-core did not answer"));
        assert_eq!(app.notices.messages(), ["sw-core did not answer"]);
        assert!(app.has_sticky_notification());

        // A result with no progress toast left still shows.
        app.finish_progress(Some(999), &Notification::success("done"));
        assert_eq!(app.notices.toasts.len(), 2);
    }

    #[test]
    fn the_history_keeps_what_the_toasts_dropped() {
        let mut app = app();
        app.show_notification(Notification::error("first failure, with its whole reason"));
        app.notices.dismiss_sticky();
        assert!(app.notices.is_empty());

        app.notices_visible = true;
        let text = render_lines(100, 30, |frame, _| app.render(frame)).join("\n");
        assert!(text.contains("Notifications"), "{text}");
        assert!(
            text.contains("first failure, with its whole reason"),
            "{text}"
        );
    }

    #[test]
    fn toasts_stay_clear_of_the_status_bar() {
        let mut app = app();
        app.active_screen = ScreenId::Devices;
        app.show_notification(Notification::error("something went wrong"));
        let rows = render_lines(80, 24, |frame, _| app.render(frame));
        let toast_row = rows
            .iter()
            .position(|row| row.contains("something went wrong"))
            .expect("toast drawn");
        assert!(
            toast_row < 6,
            "toast at row {toast_row}:\n{}",
            rows.join("\n")
        );
        let status = rows.last().expect("status bar drawn");
        assert!(status.contains("? help"), "{status}");
    }
}
