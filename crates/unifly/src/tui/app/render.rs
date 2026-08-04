use std::time::Instant;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Tabs},
};

use super::{App, ConnectionStatus};
use crate::tui::action::{ConfirmAction, Notification, NotificationLevel};
use crate::tui::screen::ScreenId;
use crate::tui::theme;

impl App {
    /// Render the full application frame.
    ///
    /// Takes `&mut self` so we can tick the tachyonfx [`EffectStack`] once
    /// per frame. The effect stack is applied as buffer post-processing
    /// after screens + tabs + status bar render, but before overlay chrome
    /// (notifications, dialogs, help, about) — matching chromacat's layering
    /// and ensuring effects animate primary content without obscuring
    /// transient UI.
    pub(super) fn render(&mut self, frame: &mut Frame) {
        let now = Instant::now();
        let delta = now.duration_since(self.last_frame);
        self.last_frame = now;

        let area = frame.area();

        if self.active_screen == ScreenId::Setup || self.active_screen == ScreenId::Settings {
            if let Some(screen) = self.screens.get(&self.active_screen) {
                screen.render(frame, area);
            }
            if self.effects_enabled && self.effects.is_active() {
                self.effects.process(delta, frame.buffer_mut(), area);
            }
            return;
        }

        let layout = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

        let content_area = layout[0];
        let tab_area = layout[1];
        let status_area = layout[2];

        if let Some(screen) = self.screens.get(&self.active_screen) {
            screen.render(frame, content_area);
        }

        self.render_tab_bar(frame, tab_area);
        self.render_status_bar(frame, status_area);

        if self.effects_enabled && self.effects.is_active() {
            self.effects.process(delta, frame.buffer_mut(), area);
        }

        if let Some((notification, _)) = &self.notification {
            self.render_notification(frame, area, notification);
        }

        if let Some(confirm) = &self.pending_confirm {
            self.render_confirm_dialog(frame, area, confirm);
        }

        if self.about_visible {
            self.render_about_overlay(frame, area);
        }

        if self.help_visible {
            self.render_help_overlay(frame, area);
        }
    }

    /// Render the bottom tab bar showing all primary screens.
    fn render_tab_bar(&self, frame: &mut Frame, area: Rect) {
        let compact = area.width < 100;
        let titles: Vec<Line> = ScreenId::ALL
            .iter()
            .map(|&id| {
                let style = if id == self.active_screen {
                    theme::tab_active()
                } else {
                    theme::tab_inactive()
                };
                let label = if compact {
                    id.label_short()
                } else {
                    id.label()
                };
                Line::from(Span::styled(format!(" {} {} ", id.number(), label), style))
            })
            .collect();

        let tabs = Tabs::new(titles)
            .divider(Span::styled(" ", theme::key_hint()))
            .select(
                ScreenId::ALL
                    .iter()
                    .position(|&screen| screen == self.active_screen)
                    .unwrap_or(0),
            );

        frame.render_widget(tabs, area);
    }

    /// Render the bottom status bar with connection status and key hints.
    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        if self.search_active {
            let line = Line::from(vec![
                Span::styled(" / ", Style::default().fg(theme::accent_primary())),
                Span::styled(
                    &self.search_query,
                    Style::default().fg(theme::accent_secondary()),
                ),
                Span::styled("█", Style::default().fg(theme::accent_secondary())),
                Span::styled("  Esc cancel  Enter submit", theme::key_hint()),
            ]);
            frame.render_widget(Paragraph::new(line), area);
            return;
        }

        let connection_indicator = match self.connection_status {
            ConnectionStatus::Connected => {
                Span::styled("● connected", Style::default().fg(theme::success()))
            }
            ConnectionStatus::Disconnected => {
                Span::styled("○ disconnected", Style::default().fg(theme::error()))
            }
            ConnectionStatus::Reconnecting => {
                Span::styled("◐ reconnecting", Style::default().fg(theme::warning()))
            }
            ConnectionStatus::Connecting => {
                Span::styled("◐ connecting", Style::default().fg(theme::warning()))
            }
        };

        let hints = Span::styled(
            " │ ? help  a about  / search  , settings  q quit",
            theme::key_hint(),
        );
        let mut spans = vec![Span::raw(" "), connection_indicator];
        if matches!(self.connection_status, ConnectionStatus::Disconnected)
            && let Some(reason) = &self.connection_error
        {
            // The reason is best-effort context: it must never displace the
            // key hints or run under the sponsor button. Budget from the
            // actual fixed-span widths and drop it when it cannot fit.
            let donate_width = if self.show_donate { 12 } else { 0 };
            let fixed: usize =
                spans.iter().map(Span::width).sum::<usize>() + hints.width() + 3 + donate_width;
            let budget = usize::from(area.width).saturating_sub(fixed);
            if budget >= 12 {
                let short: String = reason.chars().take(budget).collect();
                spans.push(Span::styled(
                    format!(" ({short})"),
                    Style::default().fg(theme::error()),
                ));
            }
        }
        spans.push(hints);
        let line = Line::from(spans);

        frame.render_widget(Paragraph::new(line), area);

        if self.show_donate {
            self.render_donate_button(frame, area);
        }
    }

    /// Render a clickable sponsor button at the right edge of the status bar.
    #[allow(clippy::unused_self)]
    fn render_donate_button(&self, frame: &mut Frame, area: Rect) {
        let button_width = 12u16;
        let x = area.x + area.width.saturating_sub(button_width);
        let button_rect = Rect::new(x, area.y, button_width, 1);

        let line = Line::from(vec![
            Span::styled(
                " ♥ ",
                Style::default()
                    .fg(theme::accent_primary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Sponsor ", Style::default().fg(theme::text_muted())),
        ]);
        frame.render_widget(Paragraph::new(line), button_rect);
    }

    /// Render the About overlay centered on screen.
    #[allow(clippy::unused_self)]
    fn render_about_overlay(&self, frame: &mut Frame, area: Rect) {
        let width = 46u16.min(area.width.saturating_sub(4));
        let height = 16u16.min(area.height.saturating_sub(4));
        let x = (area.width.saturating_sub(width)) / 2;
        let y = (area.height.saturating_sub(height)) / 2;
        let overlay = Rect::new(area.x + x, area.y + y, width, height);

        frame.render_widget(Clear, overlay);
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::bg_base())),
            overlay,
        );

        let block = Block::default()
            .title(" About ")
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_focused());

        let inner = block.inner(overlay);
        frame.render_widget(block, overlay);

        let content = vec![
            Line::from(""),
            Line::from(Span::styled(
                "unifly",
                Style::default()
                    .fg(theme::accent_primary())
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!("v{}", env!("CARGO_PKG_VERSION")),
                Style::default().fg(theme::text_muted()),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "CLI and TUI for managing",
                Style::default().fg(theme::text_secondary()),
            )),
            Line::from(Span::styled(
                "UniFi network controllers",
                Style::default().fg(theme::text_secondary()),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("by ", Style::default().fg(theme::text_muted())),
                Span::styled(
                    "Stefanie Jane",
                    Style::default()
                        .fg(theme::accent_secondary())
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "\u{2665} ",
                    Style::default()
                        .fg(theme::accent_primary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("d", theme::key_hint_key()),
                Span::styled("onate", Style::default().fg(theme::text_secondary())),
                Span::styled("     ", Style::default()),
                Span::styled("g", theme::key_hint_key()),
                Span::styled("ithub", Style::default().fg(theme::text_secondary())),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Apache-2.0",
                Style::default().fg(theme::text_muted()),
            )),
            Line::from(""),
            Line::from(Span::styled("Esc or a to close", theme::key_hint())),
        ];

        frame.render_widget(
            Paragraph::new(content).alignment(ratatui::layout::Alignment::Center),
            inner,
        );
    }

    /// Render the help overlay centered on screen. The underlying screen
    /// stays visible around the modal — we only clear the overlay's own
    /// rect so the rest of the TUI shows through.
    #[allow(clippy::unused_self)]
    fn render_help_overlay(&self, frame: &mut Frame, area: Rect) {
        let help_width = 60u16.min(area.width.saturating_sub(4));
        let help_height = 22u16.min(area.height.saturating_sub(4));
        let x = (area.width.saturating_sub(help_width)) / 2;
        let y = (area.height.saturating_sub(help_height)) / 2;
        let help_area = Rect::new(area.x + x, area.y + y, help_width, help_height);

        frame.render_widget(Clear, help_area);
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::bg_base())),
            help_area,
        );

        let block = Block::default()
            .title(" Keyboard Shortcuts ")
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_focused());

        let inner = block.inner(help_area);
        frame.render_widget(block, help_area);

        let help_text = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "  Navigation",
                Style::default().fg(theme::accent_secondary()),
            )]),
            Line::from(Span::styled("  ─────────────", theme::key_hint())),
            Line::from(vec![
                Span::styled("  1-9         ", theme::key_hint_key()),
                Span::styled("Jump to screen", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  Tab         ", theme::key_hint_key()),
                Span::styled("Next screen", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  Shift+Tab   ", theme::key_hint_key()),
                Span::styled("Previous screen", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  j/k ↑/↓     ", theme::key_hint_key()),
                Span::styled("Move up/down", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  Enter       ", theme::key_hint_key()),
                Span::styled("Select / expand", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  Esc         ", theme::key_hint_key()),
                Span::styled("Back / close", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  g/G         ", theme::key_hint_key()),
                Span::styled("Top / bottom", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  Ctrl+d/u    ", theme::key_hint_key()),
                Span::styled("Page down / up", theme::key_hint()),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "  Global",
                Style::default().fg(theme::accent_secondary()),
            )]),
            Line::from(Span::styled("  ──────────────", theme::key_hint())),
            Line::from(vec![
                Span::styled("  /           ", theme::key_hint_key()),
                Span::styled("Search            ", theme::key_hint()),
                Span::styled("?  ", theme::key_hint_key()),
                Span::styled("This help", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  s           ", theme::key_hint_key()),
                Span::styled("Sort column        ", theme::key_hint()),
                Span::styled("f  ", theme::key_hint_key()),
                Span::styled("Filter", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  ,           ", theme::key_hint_key()),
                Span::styled("Settings           ", theme::key_hint()),
                Span::styled("a  ", theme::key_hint_key()),
                Span::styled("About", theme::key_hint()),
            ]),
            Line::from(vec![
                Span::styled("  q           ", theme::key_hint_key()),
                Span::styled("Quit", theme::key_hint()),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "                         Esc or ? to close",
                theme::key_hint(),
            )),
        ];

        frame.render_widget(Paragraph::new(help_text), inner);
    }

    /// Render a centered confirmation dialog. The underlying screen
    /// stays visible around the dialog — only the dialog's own rect is
    /// cleared so context behind the prompt remains legible.
    #[allow(clippy::unused_self)]
    fn render_confirm_dialog(&self, frame: &mut Frame, area: Rect, confirm: &ConfirmAction) {
        let width = 50u16.min(area.width.saturating_sub(4));
        let height = 5u16;
        let x = (area.width.saturating_sub(width)) / 2;
        let y = (area.height.saturating_sub(height)) / 2;
        let dialog_area = Rect::new(area.x + x, area.y + y, width, height);

        frame.render_widget(Clear, dialog_area);
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::bg_base())),
            dialog_area,
        );

        let block = Block::default()
            .title(" Confirm ")
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::warning()));

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let text = vec![
            Line::from(Span::styled(
                format!("  {confirm}"),
                Style::default().fg(theme::text_secondary()),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("  y ", theme::key_hint_key()),
                Span::styled("confirm    ", theme::key_hint()),
                Span::styled("n ", theme::key_hint_key()),
                Span::styled("cancel", theme::key_hint()),
            ]),
        ];

        frame.render_widget(Paragraph::new(text), inner);
    }

    /// Render a notification toast in the bottom-right corner.
    #[allow(clippy::unused_self)]
    fn render_notification(&self, frame: &mut Frame, area: Rect, notif: &Notification) {
        let msg_len = u16::try_from(notif.message.len()).unwrap_or(u16::MAX);
        let width = (msg_len + 6).clamp(20, 60);
        let height = 3u16;
        let x = area.width.saturating_sub(width + 1);
        let y = area.height.saturating_sub(height + 2);
        let toast_area = Rect::new(area.x + x, area.y + y, width, height);

        let (border_color, icon) = match notif.level {
            NotificationLevel::Success => (theme::success(), "✓"),
            NotificationLevel::Error => (theme::error(), "✗"),
            NotificationLevel::Warning => (theme::warning(), "!"),
            NotificationLevel::Info => (theme::accent_secondary(), "·"),
        };

        frame.render_widget(
            Block::default().style(Style::default().bg(theme::bg_base())),
            toast_area,
        );

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color));

        let inner = block.inner(toast_area);
        frame.render_widget(block, toast_area);

        let line = Line::from(vec![
            Span::styled(format!(" {icon} "), Style::default().fg(border_color)),
            Span::styled(&notif.message, Style::default().fg(theme::text_secondary())),
        ]);
        frame.render_widget(Paragraph::new(line), inner);
    }
}
