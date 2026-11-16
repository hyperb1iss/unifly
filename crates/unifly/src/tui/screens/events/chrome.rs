//! What frames the feed: the title, the toolbar, the readouts on the
//! borders, and the actions the status bar offers.

use chrono::Local;
use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use super::render::category_label;
use super::state::window_label;
use super::{CATEGORIES, EventsScreen, HistoryLoad};
use crate::tui::screens::link::Link;
use crate::tui::theme;
use crate::tui::widgets::click::{self, Click, ClickLine, Hint};
use crate::tui::widgets::{sub_tabs, toolbar};

/// Columns the feed chip takes whatever it says, so nothing on the toolbar
/// moves when the feed pauses. Sized to its widest state, `Space ◐ polling`.
const FEED_CHIP_COLUMNS: usize = 15;
/// Columns between the two chips.
const CHIP_GAP: usize = 2;

/// How events are reaching the feed.
pub(super) struct FeedState {
    pub(super) text: &'static str,
    pub(super) color: Color,
    /// Whether there is a feed for `Space` to pause or resume.
    pub(super) pausable: bool,
}

fn separator() -> Span<'static> {
    Span::styled(
        " \u{00B7}  ",
        Style::default().fg(theme::border_unfocused()),
    )
}

/// The first of `parts`, joined, that fits `room` columns: all of them,
/// else the leading ones alone. A part that does not fit is left out
/// whole.
fn fitted(parts: Vec<Vec<Span<'static>>>, room: usize) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for part in parts {
        let lead = usize::from(spans.len() > 1) * (1 + separator().width());
        let width: usize = part.iter().map(Span::width).sum();
        let used: usize = spans.iter().map(Span::width).sum();
        if used + lead + width + 1 > room {
            continue;
        }
        if spans.len() > 1 {
            spans.push(Span::raw(" "));
            spans.push(separator());
        }
        spans.extend(part);
    }
    if spans.len() == 1 {
        return Line::default();
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

impl EventsScreen {
    /// Whether the feed is live, polling, paused, or cannot arrive at all.
    pub(super) fn feed_state(&self) -> FeedState {
        let state = |text, color, pausable| FeedState {
            text,
            color,
            pausable,
        };
        let Some(access) = self.access.filter(|_| self.link.reachable()) else {
            return if self.link == Link::Down {
                state("\u{25CB} not connected", theme::error(), false)
            } else {
                state("\u{25D0} connecting", theme::warning(), false)
            };
        };
        if !access.session {
            state("\u{25CB} no access", theme::text_muted(), false)
        } else if self.paused {
            state("\u{2016} paused", theme::warning(), true)
        } else if access.live_events {
            state("\u{25CF} live", theme::success(), true)
        } else {
            state("\u{25D0} polling", theme::info(), true)
        }
    }

    fn category_labels() -> Vec<&'static str> {
        let mut labels = vec!["All"];
        labels.extend(CATEGORIES.iter().map(|category| category_label(*category)));
        labels
    }

    /// The feed state as a button that pauses or resumes it.
    fn feed_chip(&self) -> ClickLine<'static, Click> {
        let state = self.feed_state();
        let value = Span::styled(state.text, Style::default().fg(state.color));
        let mut chip = ClickLine::default();
        if state.pausable {
            let press = Click::Key(click::char_key(' '));
            chip.push(press.clone(), Span::styled("Space ", theme::key_hint_key()));
            chip.push(press, value);
        } else {
            chip.push_gap(value);
        }
        let pad = FEED_CHIP_COLUMNS.saturating_sub(chip.width());
        chip.push_gap(Span::raw(" ".repeat(pad)));
        chip
    }

    /// The count of active alarms as a button that opens them.
    fn alarm_chip(&self) -> ClickLine<'static, Click> {
        let count = self.active_alarms();
        let color = if count > 0 {
            theme::warning()
        } else {
            theme::text_muted()
        };
        let press = Click::Key(click::char_key('v'));
        let mut chip = ClickLine::default();
        chip.push(press.clone(), Span::styled("v ", theme::key_hint_key()));
        chip.push(
            press.clone(),
            Span::styled("alarms ", Style::default().fg(theme::text_secondary())),
        );
        chip.push(
            press,
            Span::styled(count.to_string(), Style::default().fg(color)),
        );
        chip
    }

    /// Decide, for a screen `width` columns wide inside its border,
    /// whether the two chips fit beside the category strip. When they do
    /// not, the title and the status bar say what they would have.
    pub(super) fn place_chips(&self, width: u16) {
        let strip = sub_tabs::sub_tab_strip(&Self::category_labels(), 0).width();
        let chips = FEED_CHIP_COLUMNS + CHIP_GAP + self.alarm_chip().width();
        self.chips_shown
            .set(chips <= toolbar::right_room(width, strip));
    }

    /// The category strip, with the feed state and the alarm count against
    /// the right edge.
    pub(super) fn render_toolbar(&self, frame: &mut Frame, area: Rect) {
        let labels = Self::category_labels();
        let active = self
            .category
            .and_then(|category| CATEGORIES.iter().position(|each| *each == category))
            .map_or(0, |index| index + 1);
        let strip = sub_tabs::sub_tab_strip(&labels, active);
        let mut chips = ClickLine::default();
        if self.chips_shown.get() {
            chips.append(self.feed_chip());
            chips.push_gap(Span::raw(" ".repeat(CHIP_GAP)));
            chips.append(self.alarm_chip());
        }
        toolbar::render(frame, area, strip, chips, &mut self.hits.borrow_mut());
    }

    /// The title: how many events show, of how many, and the search
    /// narrowing them. On a screen too narrow for the chips it also says
    /// how events arrive and how many alarms are active.
    pub(super) fn feed_title(&self, shown: usize) -> Line<'static> {
        let count = if self.filtered() {
            format!(" Events ({shown} of {}) ", self.feed.len())
        } else {
            format!(" Events ({}) ", self.feed.len())
        };
        let mut spans = vec![Span::styled(count, theme::title_style())];
        if !self.search.is_empty() {
            spans.push(Span::styled(
                format!("[\"{}\"] ", self.search),
                Style::default().fg(theme::warning()),
            ));
        }
        if !self.chips_shown.get() {
            let state = self.feed_state();
            spans.push(separator());
            spans.push(Span::styled(state.text, Style::default().fg(state.color)));
            let alarms = match self.active_alarms() {
                0 => None,
                1 => Some("1 alarm".to_owned()),
                count => Some(format!("{count} alarms")),
            };
            if let Some(alarms) = alarms {
                spans.push(Span::raw(" "));
                spans.push(separator());
                spans.push(Span::styled(alarms, Style::default().fg(theme::warning())));
            }
            spans.push(Span::raw(" "));
        }
        Line::from(spans)
    }

    /// What the top border says against its right corner, in `room`
    /// columns: a history load or its outcome, then how far back the feed
    /// reaches, the oldest end of the list being its top.
    pub(super) fn history_line(&self, room: usize) -> Line<'static> {
        let muted = Style::default().fg(theme::text_secondary());
        let mut parts = Vec::new();
        if let Some(pending) = self.history.loading {
            let what = match pending.load {
                HistoryLoad::Older => {
                    format!(
                        "loading {} of older events\u{2026}",
                        window_label(pending.hours)
                    )
                }
                HistoryLoad::Firewall => "loading firewall blocks\u{2026}".to_owned(),
            };
            parts.push(vec![Span::styled(
                what,
                Style::default().fg(theme::warning()),
            )]);
        } else if let Some(note) = &self.history.note {
            parts.push(vec![Span::styled(note.clone(), muted)]);
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
            parts.push(vec![Span::styled(covers.join(" + "), muted)]);
        }
        fitted(parts, room).right_aligned()
    }

    /// What the bottom border says against its right corner while the
    /// feed is paused: where the cursor is, and what has arrived below it.
    /// A live feed needs neither: it ends on its newest event.
    pub(super) fn position_line(&self, view: &[usize]) -> Line<'static> {
        if !self.paused || view.is_empty() {
            return Line::default();
        }
        let mut spans = vec![Span::styled(
            format!(" event {} of {} ", self.cursor_row(view) + 1, view.len()),
            Style::default().fg(theme::text_secondary()),
        )];
        if self.unseen > 0 {
            spans.push(Span::styled(
                format!("\u{25BC} {} new ", self.unseen),
                Style::default().fg(theme::accent_secondary()),
            ));
        }
        Line::from(spans).right_aligned()
    }

    /// What the alarms view's top border says against its right corner:
    /// how many alarms are archived, and a read under way or failed.
    pub(super) fn alarm_status_line(&self, room: usize) -> Line<'static> {
        let mut parts = Vec::new();
        if let Some(status) = self.alarm_read_status(room.saturating_sub(2)) {
            parts.push(vec![status]);
        }
        let archived = self.archived_alarms();
        if archived > 0 {
            parts.push(vec![Span::styled(
                format!("{archived} archived"),
                Style::default().fg(theme::text_secondary()),
            )]);
        }
        fitted(parts, room).right_aligned()
    }

    /// What the status bar offers: what `Enter` opens and the loads the
    /// feed takes, by mode. `Space` and `v` join them only on a screen too
    /// narrow for the chips that carry those keys.
    pub(super) fn feed_hints(&self) -> Vec<(u8, Hint)> {
        let chips = self.chips_shown.get();
        let pause = (!chips && self.feed_state().pausable).then(|| {
            let label = if self.paused { "resume" } else { "pause" };
            Hint::button("Space", label, click::char_key(' '))
        });
        let alarms = (!chips).then(|| Hint::button("v", "alarms", click::char_key('v')));
        let mut hints = Vec::new();
        if self.detail_open {
            if self.selected_device_id().is_some() {
                hints.push((1, Hint::button("d", "device", click::char_key('d'))));
            }
            if self.selected_client_id().is_some() {
                hints.push((1, Hint::button("c", "client", click::char_key('c'))));
            }
            hints.extend(pause.map(|hint| (2, hint)));
            hints.push((0, Hint::button("Esc", "close", KeyCode::Esc)));
            return hints;
        }
        if self.any_listed() {
            hints.push((0, Hint::button("Enter", "details", KeyCode::Enter)));
        }
        hints.extend(pause.map(|hint| (0, hint)));
        if self.history.loading.is_some() {
            hints.extend(alarms.map(|hint| (2, hint)));
            hints.push((0, Hint::button("Esc", "cancel", KeyCode::Esc)));
            return hints;
        }
        let session = self.access.is_some_and(|access| access.session);
        if session {
            hints.push((1, Hint::button("o", "older", click::char_key('o'))));
        }
        if self.history.include_firewall {
            hints.push((3, Hint::button("f", "hide blocks", click::char_key('f'))));
        } else if session {
            hints.push((
                3,
                Hint::button("f", "firewall blocks", click::char_key('f')),
            ));
        }
        if self.heatmap_shown.get() || self.heatmap_fits.get() {
            hints.push((4, Hint::button("p", "heatmap", click::char_key('p'))));
        }
        hints.extend(alarms.map(|hint| (2, hint)));
        hints
    }
}

#[cfg(test)]
mod tests {
    use super::{EventsScreen, FEED_CHIP_COLUMNS};
    use crate::tui::action::{Access, Action};

    #[test]
    fn every_feed_state_fits_the_room_kept_for_its_chip() {
        let with = |actions: &[Action]| {
            let mut screen = EventsScreen::new();
            for action in actions {
                screen.apply_action(action);
            }
            screen
        };
        let access = |session, live_events| {
            Action::AccessUpdated(Access {
                integration: true,
                session,
                live_events,
            })
        };
        let screens = [
            with(&[]),
            with(&[Action::Connected, access(true, true)]),
            with(&[Action::Connected, access(true, false)]),
            with(&[Action::Connected, access(false, false)]),
            with(&[
                Action::Connected,
                access(true, true),
                Action::ToggleEventPause,
            ]),
            with(&[
                Action::Connected,
                access(true, true),
                Action::Disconnected("gone".into()),
            ]),
        ];
        let mut seen = std::collections::HashSet::new();
        for screen in &screens {
            let state = screen.feed_state();
            seen.insert(state.text);
            assert_eq!(
                screen.feed_chip().width(),
                FEED_CHIP_COLUMNS,
                "{} overflows its chip",
                state.text
            );
        }
        assert_eq!(seen.len(), screens.len(), "every state is covered");
    }
}
