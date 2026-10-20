use super::super::{SettingsField, SettingsScreen, SettingsState};

use crate::tui::theme;
use crate::tui::widgets::click::{self, ClickLine, Hint};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

pub(super) fn panel_rect(area: Rect) -> Rect {
    let width = 62u16.min(area.width.saturating_sub(4));
    let height = 32u16.min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    Rect::new(area.x + x, area.y + y, width, height)
}

pub(super) fn centered_rect(area: Rect, cols: u16, rows: u16) -> Rect {
    let width = cols.min(area.width.saturating_sub(2));
    let height = rows.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

impl SettingsScreen {
    #[allow(clippy::unused_self)]
    pub(super) fn render_centered_panel(&self, frame: &mut Frame, area: Rect) -> Rect {
        let panel = panel_rect(area);

        frame.render_widget(
            Block::default().style(Style::default().bg(theme::bg_base())),
            panel,
        );

        let block = Block::default()
            .title(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    "Settings",
                    Style::default()
                        .fg(theme::accent_secondary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
            ]))
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::accent_primary()));

        let inner = block.inner(panel);
        frame.render_widget(block, panel);
        inner
    }

    pub(super) fn render_key_hints(&self, frame: &mut Frame, area: Rect) {
        let next = Hint::button("Tab", "next", KeyCode::Tab);
        let save = Hint::button("Enter", "test & save", KeyCode::Enter);
        let close = Hint::button("Esc", "close", KeyCode::Esc);
        let hints = match self.state {
            SettingsState::Testing => vec![Hint::button("Esc", "cancel", KeyCode::Esc)],
            SettingsState::Editing => match self.active_field {
                SettingsField::AuthMode => {
                    vec![Hint::info("\u{25C2}/\u{25B8}", "select"), next, save, close]
                }
                SettingsField::Insecure | SettingsField::ShowDonate => vec![
                    Hint::button("Space", "toggle", click::char_key(' ')),
                    next,
                    save,
                    close,
                ],
                SettingsField::Password => vec![
                    Hint::button(
                        "Ctrl+U",
                        "reveal",
                        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
                    ),
                    next,
                    save,
                    close,
                ],
                SettingsField::Theme => vec![
                    Hint::button("Enter", "choose theme", KeyCode::Enter),
                    next,
                    close,
                ],
                _ => vec![
                    next,
                    Hint::button("Shift+Tab", "prev", KeyCode::BackTab),
                    save,
                    close,
                ],
            },
        };

        let mut line = ClickLine::default();
        line.push_hints(&hints, theme::key_hint_key(), theme::key_hint());
        line.register_centered(area, &mut self.hits.borrow_mut());
        frame.render_widget(
            Paragraph::new(line.into_line()).alignment(Alignment::Center),
            area,
        );
    }
}
