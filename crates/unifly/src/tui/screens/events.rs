//! Events screen — live event stream with pause/filter (spec §2.7).

mod punchcard;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use tokio::sync::mpsc::UnboundedSender;

use unifly_api::Event;
use unifly_api::model::EventSeverity;

use crate::tui::action::Action;
use crate::tui::component::Component;
use crate::tui::theme;
use crate::tui::widgets::click::{self, Click, Hint, HitMap};

use punchcard::{PUNCHCARD_HEIGHT, render_punchcard};

pub struct EventsScreen {
    focused: bool,
    events: VecDeque<Arc<Event>>,
    paused: bool,
    /// How many events below the newest the view ends, so 0 shows the
    /// newest event at the bottom.
    scroll_offset: usize,
    /// Max events to keep in memory.
    capacity: usize,
    /// What the last frame drew where, for clicks.
    hits: RefCell<HitMap<Click>>,
}

impl Default for EventsScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl EventsScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            events: VecDeque::new(),
            paused: false,
            scroll_offset: 0,
            capacity: 10_000,
            hits: RefCell::default(),
        }
    }

    #[allow(dead_code, clippy::unused_self)]
    fn visible_count(&self, area_height: u16) -> usize {
        usize::from(area_height.saturating_sub(1))
    }
}

impl Component for EventsScreen {
    fn init(&mut self, _action_tx: UnboundedSender<Action>) -> Result<()> {
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match key.code {
            KeyCode::Char(' ') => {
                self.paused = !self.paused;
                if !self.paused {
                    // Resume: snap to bottom
                    self.scroll_offset = 0;
                }
                Ok(Some(Action::ToggleEventPause))
            }
            KeyCode::Char('j') | KeyCode::Down if self.paused => {
                if self.scroll_offset > 0 {
                    self.scroll_offset -= 1;
                }
                Ok(None)
            }
            KeyCode::Char('k') | KeyCode::Up if self.paused => {
                self.scroll_offset =
                    (self.scroll_offset + 1).min(self.events.len().saturating_sub(1));
                Ok(None)
            }
            KeyCode::Char('g') if self.paused => {
                self.scroll_offset = self.events.len().saturating_sub(1);
                Ok(Some(Action::ScrollToTop))
            }
            KeyCode::Char('G') if self.paused => {
                self.scroll_offset = 0;
                Ok(Some(Action::ScrollToBottom))
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) && self.paused => {
                self.scroll_offset = self.scroll_offset.saturating_sub(10);
                Ok(Some(Action::PageDown))
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) && self.paused => {
                self.scroll_offset =
                    (self.scroll_offset + 10).min(self.events.len().saturating_sub(1));
                Ok(Some(Action::PageUp))
            }
            _ => Ok(None),
        }
    }

    /// Key hints press their key, and the wheel scrolls the feed.
    /// Scrolling back through a live feed pauses it first, so the rows
    /// hold still under the wheel.
    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        let Some(delta) = click::wheel_delta(mouse) else {
            let target = self.hits.borrow().clicked(mouse);
            return Ok(match target {
                Some(Click::Key(key)) => Some(Action::PressKey(key)),
                _ => None,
            });
        };
        let paused = (delta < 0 && !self.paused && !self.events.is_empty()).then(|| {
            self.paused = true;
            Action::ToggleEventPause
        });
        let key = if delta < 0 {
            KeyCode::Up
        } else {
            KeyCode::Down
        };
        let scrolled = self.handle_key_event(key.into())?;
        Ok(paused.or(scrolled))
    }

    fn update(&mut self, action: &Action) -> Result<Option<Action>> {
        if let Action::EventReceived(event) = action {
            self.events.push_back(Arc::clone(event));
            // The view ends `scroll_offset` events above the newest, so a
            // paused view steps back over each arrival to hold its rows.
            if self.paused {
                self.scroll_offset += 1;
            }
            if self.events.len() > self.capacity {
                self.events.pop_front();
            }
            self.scroll_offset = self.scroll_offset.min(self.events.len().saturating_sub(1));
        }
        Ok(None)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        self.hits.borrow_mut().clear();
        let count = self.events.len();
        let title = format!(" Events ({count}) ");
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

        let show_punchcard = inner.height >= PUNCHCARD_HEIGHT + 7 && inner.width >= 48;
        let layout = if show_punchcard {
            Layout::vertical([
                Constraint::Length(1),                // status line
                Constraint::Length(PUNCHCARD_HEIGHT), // punchcard
                Constraint::Min(1),                   // events
                Constraint::Length(1),                // hints
            ])
            .split(inner)
        } else {
            Layout::vertical([
                Constraint::Length(1), // status line
                Constraint::Min(1),    // events
                Constraint::Length(1), // hints
            ])
            .split(inner)
        };

        frame.render_widget(Paragraph::new(self.status_line()), layout[0]);

        let (events_area, hints_area) = if show_punchcard {
            render_punchcard(&self.events, frame, layout[1]);
            (layout[2], layout[3])
        } else {
            (layout[1], layout[2])
        };

        self.render_event_list(frame, events_area);
        self.render_hints(frame, hints_area);
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn id(&self) -> &'static str {
        "Events"
    }
}

impl EventsScreen {
    fn status_line(&self) -> Line<'static> {
        let live_indicator = if self.paused {
            Span::styled("PAUSED", Style::default().fg(theme::warning()))
        } else {
            Span::styled("● LIVE", Style::default().fg(theme::success()))
        };

        Line::from(vec![
            Span::styled("  Filter: ", Style::default().fg(theme::text_secondary())),
            Span::styled("[all]", Style::default().fg(theme::accent_secondary())),
            Span::styled("  Type: ", Style::default().fg(theme::text_secondary())),
            Span::styled("[all]", Style::default().fg(theme::accent_secondary())),
            Span::raw("  "),
            live_indicator,
        ])
    }

    fn render_event_list(&self, frame: &mut Frame, area: Rect) {
        let auto_scrolling = !self.paused && !self.events.is_empty();
        // The header row and the auto-scroll marker share the area with the
        // events, so the newest event stays on screen above the marker.
        let chrome = 1 + usize::from(auto_scrolling);
        let visible_height = usize::from(area.height).saturating_sub(chrome);
        let end = self.events.len().saturating_sub(self.scroll_offset);
        let start = end.saturating_sub(visible_height);

        let mut lines = vec![Line::from(vec![
            Span::styled(format!("  {:<12}", "Time"), theme::table_header()),
            Span::styled(format!("{:<11}", "Category"), theme::table_header()),
            Span::styled("Message", theme::table_header()),
        ])];

        let meta_cols: u16 = 2 + 12 + 11; // indent + time + category
        let msg_width = usize::from(area.width.saturating_sub(meta_cols).max(10));

        for event in self.events.iter().skip(start).take(end - start) {
            let time_str = event.timestamp.format("%H:%M:%S").to_string();
            let severity_color = match event.severity {
                EventSeverity::Error | EventSeverity::Critical => theme::error(),
                EventSeverity::Warning => theme::warning(),
                EventSeverity::Info => theme::accent_secondary(),
                _ => theme::text_secondary(),
            };
            let category = format!("{:?}", event.category);
            let msg: String = event.message.chars().take(msg_width).collect();

            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {time_str:<12}"),
                    Style::default().fg(theme::warning()),
                ),
                Span::styled(
                    format!("{category:<11}"),
                    Style::default().fg(theme::text_secondary()),
                ),
                Span::styled(msg, Style::default().fg(severity_color)),
            ]));
        }

        if self.events.is_empty() {
            lines.push(Line::from(Span::styled(
                "  Waiting for events...",
                Style::default().fg(theme::border_unfocused()),
            )));
        }

        if auto_scrolling {
            lines.push(Line::from(Span::styled(
                "  ↓ auto-scrolling",
                Style::default().fg(theme::border_unfocused()),
            )));
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_hints(&self, frame: &mut Frame, area: Rect) {
        let bar = click::key_hints(&[
            Hint::button("Space", "pause/resume", click::char_key(' ')),
            Hint::info("j/k", "scroll (paused)"),
        ]);
        let line = bar.draw(area, &mut self.hits.borrow_mut());
        frame.render_widget(Paragraph::new(line), area);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use unifly_api::Event;

    use super::EventsScreen;
    use crate::tui::action::Action;
    use crate::tui::component::Component;
    use crate::tui::table::render_lines;

    fn event(minute: u32) -> Arc<Event> {
        Arc::new(
            serde_json::from_value(json!({
                "id": format!("evt-{minute}"),
                "timestamp": format!("2026-10-12T09:{minute:02}:00Z"),
                "category": "Client",
                "severity": "Info",
                "event_type": "EVT_WU_Connected",
                "message": format!("client-{minute:02} connected"),
                "device_mac": null,
                "client_mac": null,
                "site_id": null,
                "raw_key": null
            }))
            .expect("event fixture should deserialize"),
        )
    }

    #[test]
    fn the_wheel_leaves_an_empty_live_feed_running() {
        use crossterm::event::MouseEventKind;

        use crate::tui::widgets::click::mouse;

        let mut screen = EventsScreen::new();
        let action = screen
            .handle_mouse_event(mouse(MouseEventKind::ScrollUp, 5, 5))
            .expect("wheel handling");

        assert!(action.is_none(), "{action:?}");
        assert!(!screen.paused);
    }

    #[test]
    fn scrolling_back_through_a_live_feed_pauses_it() {
        use crossterm::event::MouseEventKind;

        use crate::tui::widgets::click::mouse;

        let mut screen = EventsScreen::new();
        for index in 0..5 {
            screen.events.push_back(event(index));
        }

        let action = screen
            .handle_mouse_event(mouse(MouseEventKind::ScrollUp, 5, 5))
            .expect("wheel handling");
        assert!(matches!(action, Some(Action::ToggleEventPause)));
        assert!(screen.paused);
        assert_eq!(screen.scroll_offset, 1);

        screen
            .handle_mouse_event(mouse(MouseEventKind::ScrollDown, 5, 5))
            .expect("wheel handling");
        assert!(screen.paused, "scrolling forward keeps the pause");
        assert_eq!(screen.scroll_offset, 0);
    }

    #[test]
    fn live_list_keeps_the_newest_event_above_the_scroll_marker() {
        let mut screen = EventsScreen::new();
        for minute in 0..40 {
            screen
                .update(&Action::EventReceived(event(minute)))
                .expect("update");
        }

        for (width, height) in [(80, 24), (100, 30), (183, 47)] {
            let text = render_lines(width, height, |frame, area| screen.render(frame, area));
            let newest = text
                .iter()
                .position(|row| row.contains("client-39 connected"))
                .unwrap_or_else(|| panic!("newest event missing:\n{}", text.join("\n")));
            assert!(
                text[newest + 1].contains("auto-scrolling"),
                "newest event not last:\n{}",
                text.join("\n")
            );
        }
    }

    /// The event rows a 100x30 frame shows, newest last.
    fn shown_events(screen: &EventsScreen) -> Vec<String> {
        render_lines(100, 30, |frame, area| screen.render(frame, area))
            .into_iter()
            .filter(|row| row.contains(" connected"))
            .map(|row| row.trim_matches(['│', ' ']).to_owned())
            .collect()
    }

    fn receive(screen: &mut EventsScreen, minutes: std::ops::Range<u32>) {
        for minute in minutes {
            screen
                .update(&Action::EventReceived(event(minute)))
                .expect("update");
        }
    }

    #[test]
    fn a_paused_feed_holds_still_as_events_arrive() {
        use crossterm::event::{KeyCode, KeyEvent};

        let press = |screen: &mut EventsScreen, code: KeyCode| {
            screen.handle_key_event(KeyEvent::from(code)).expect("key");
        };
        let mut screen = EventsScreen::new();
        receive(&mut screen, 0..40);

        press(&mut screen, KeyCode::Char(' '));
        let paused = shown_events(&screen);
        assert!(paused.last().is_some_and(|row| row.contains("client-39")));
        receive(&mut screen, 40..45);
        assert_eq!(shown_events(&screen), paused, "new events moved the view");

        // Scrolled back, the view holds its place as well.
        for _ in 0..3 {
            press(&mut screen, KeyCode::Char('k'));
        }
        let scrolled = shown_events(&screen);
        assert!(scrolled.last().is_some_and(|row| row.contains("client-36")));
        receive(&mut screen, 45..50);
        assert_eq!(shown_events(&screen), scrolled, "new events moved the view");

        // Scrolling forward reaches the events that arrived while paused,
        // and resuming jumps to the newest.
        press(&mut screen, KeyCode::Char('G'));
        assert!(
            shown_events(&screen)
                .last()
                .is_some_and(|row| row.contains("client-49"))
        );
        press(&mut screen, KeyCode::Char('k'));
        press(&mut screen, KeyCode::Char(' '));
        assert!(
            shown_events(&screen)
                .last()
                .is_some_and(|row| row.contains("client-49"))
        );
    }

    #[test]
    fn a_full_feed_drops_its_oldest_events() {
        let mut screen = EventsScreen::new();
        screen.capacity = 5;
        receive(&mut screen, 0..8);

        let kept: Vec<_> = screen
            .events
            .iter()
            .map(|event| event.message.as_str())
            .collect();
        assert_eq!(
            kept,
            [
                "client-03 connected",
                "client-04 connected",
                "client-05 connected",
                "client-06 connected",
                "client-07 connected",
            ]
        );
    }

    #[test]
    fn a_paused_view_of_the_oldest_events_survives_their_eviction() {
        use crossterm::event::{KeyCode, KeyEvent};

        let mut screen = EventsScreen::new();
        screen.capacity = 5;
        receive(&mut screen, 0..5);
        screen
            .handle_key_event(KeyEvent::from(KeyCode::Char(' ')))
            .expect("pause");
        screen
            .handle_key_event(KeyEvent::from(KeyCode::Char('g')))
            .expect("top");

        receive(&mut screen, 5..8);

        let shown = shown_events(&screen);
        assert_eq!(
            shown.first().map(String::as_str),
            Some("09:03:00    Client     client-03 connected"),
            "{shown:?}"
        );
    }

    #[test]
    fn list_header_lines_up_with_the_rows() {
        let mut screen = EventsScreen::new();
        screen
            .update(&Action::EventReceived(event(5)))
            .expect("update");

        let text = render_lines(100, 30, |frame, area| screen.render(frame, area));
        let header = text
            .iter()
            .find(|row| row.contains("Category"))
            .expect("list header");
        let row = text
            .iter()
            .find(|row| row.contains("client-05"))
            .expect("event row");
        assert_eq!(header.find("Category"), row.find("Client"));
        assert_eq!(header.find("Message"), row.find("client-05"));
    }
}
