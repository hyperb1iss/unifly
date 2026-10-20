use std::time::Instant;

use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use super::{App, ConnectionStatus};
use crate::tui::action::{Action, ConfirmAction, Notification, NotificationLevel};
use crate::tui::screen::ScreenId;
use crate::tui::theme;
use crate::tui::widgets::click::{self, Click, ClickLine, Hint, HitMap};

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
        self.hits.clear();

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

        // Search holds the input, as it does for keys: only its own
        // status-bar buttons stay live. A screen taking typed input holds
        // the chrome the same way, so a tap cannot switch screens or type
        // a status-bar key into a form field.
        if self.search_active {
            self.hits.push(content_area.union(tab_area), Click::Absorb);
        } else if self.active_screen_captures_input() {
            self.hits.push(tab_area.union(status_area), Click::Absorb);
        }

        if self.effects_enabled && self.effects.is_active() {
            self.effects.process(delta, frame.buffer_mut(), area);
        }

        if let Some((notification, _)) = &self.notification {
            Self::render_notification(frame, area, notification, &mut self.hits);
        }

        if let Some(confirm) = &self.pending_confirm {
            Self::render_confirm_dialog(frame, area, confirm, &mut self.hits);
        }

        if self.about_visible {
            Self::render_about_overlay(frame, area, &mut self.hits);
        }

        if self.help_visible {
            Self::render_help_overlay(frame, area, &mut self.hits);
        }
    }

    /// Register a modal's click targets: a tap outside it presses `Esc`
    /// (which closes it or answers no), and a tap inside that misses its
    /// buttons does nothing.
    fn register_modal(hits: &mut HitMap<Click>, screen: Rect, modal: Rect) {
        hits.push(screen, Click::Key(KeyCode::Esc.into()));
        hits.push(modal, Click::Absorb);
    }

    /// Render the bottom tab bar showing all primary screens, and keep
    /// where each tab landed so a click can switch to it.
    fn render_tab_bar(&mut self, frame: &mut Frame, area: Rect) {
        let line = self.tab_strip(area.width).draw(area, &mut self.hits);
        frame.render_widget(Paragraph::new(line), area);
    }

    /// The tab bar in the roomiest style that fits `width`: full labels
    /// before short ones, padded tabs before tight ones. A clipped tab
    /// could be neither read nor clicked.
    fn tab_strip(&self, width: u16) -> ClickLine<'static, Click> {
        [(false, true), (false, false), (true, true)]
            .into_iter()
            .map(|(compact, padded)| self.tab_strip_styled(compact, padded))
            .find(|strip| strip.width() <= usize::from(width))
            .unwrap_or_else(|| self.tab_strip_styled(true, false))
    }

    /// The tab bar's spans. Each tab owns the padding around its title, so
    /// only the one-column dividers fall between click targets.
    fn tab_strip_styled(&self, compact: bool, padded: bool) -> ClickLine<'static, Click> {
        let mut strip = ClickLine::default();
        for (index, &id) in ScreenId::ALL.iter().enumerate() {
            if index > 0 {
                strip.push_gap(Span::styled(" ", theme::key_hint()));
            }
            let style = if id == self.active_screen {
                theme::tab_active().add_modifier(Modifier::REVERSED)
            } else {
                theme::tab_inactive()
            };
            let label = if compact {
                id.label_short()
            } else {
                id.label()
            };
            let target = Click::Action(Action::SwitchScreen(id));
            if padded {
                strip.push(target.clone(), Span::raw(" "));
            }
            strip.push(
                target.clone(),
                Span::styled(format!(" {} {} ", id.number(), label), style),
            );
            if padded {
                strip.push(target, Span::raw(" "));
            }
        }
        strip
    }

    /// Render the bottom status bar with connection status and key hints.
    fn render_status_bar(&mut self, frame: &mut Frame, area: Rect) {
        if self.search_active {
            let mut line = ClickLine::default();
            line.push_gap(Span::styled(
                " / ",
                Style::default().fg(theme::accent_primary()),
            ));
            line.push_gap(Span::styled(
                self.search_query.clone(),
                Style::default().fg(theme::accent_secondary()),
            ));
            line.push_gap(Span::styled(
                "█",
                Style::default().fg(theme::accent_secondary()),
            ));
            line.push_gap(Span::raw("  "));
            line.push_hints(
                &[
                    Hint::button("Esc", "cancel", KeyCode::Esc),
                    Hint::button("Enter", "submit", KeyCode::Enter),
                ],
                theme::key_hint_key(),
                theme::key_hint(),
            );
            let line = line.draw(area, &mut self.hits);
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
            ConnectionStatus::Reconnecting { attempt } => {
                let label = if attempt > 1 {
                    format!("◐ reconnecting (attempt {attempt})")
                } else {
                    "◐ reconnecting".to_string()
                };
                Span::styled(label, Style::default().fg(theme::warning()))
            }
            ConnectionStatus::Connecting => {
                Span::styled("◐ connecting", Style::default().fg(theme::warning()))
            }
        };

        let mut hints = ClickLine::default();
        hints.push_gap(Span::styled(" │ ", theme::key_hint()));
        // App-wide buttons dispatch their action directly: as keys they
        // would reach the screen first, which may use the same letter.
        let retry = Hint::action("^r", "retry", Action::RetryConnect);
        let global = [
            Hint::action("?", "help", Action::ToggleHelp),
            Hint::action("a", "about", Action::ToggleAbout),
            Hint::action("/", "search", Action::OpenSearch),
            Hint::action(",", "settings", Action::OpenSettings),
            Hint::action("q", "quit", Action::Quit),
        ];
        let shown: Vec<Hint> = if self.connection_status == ConnectionStatus::Disconnected {
            std::iter::once(retry).chain(global).collect()
        } else {
            global.to_vec()
        };
        hints.push_hints(&shown, theme::key_hint_key(), theme::key_hint());
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
        let mut line = ClickLine::default();
        for span in spans {
            line.push_gap(span);
        }
        line.append(hints);
        let line = line.draw(area, &mut self.hits);
        frame.render_widget(Paragraph::new(line), area);

        if self.show_donate {
            Self::render_donate_button(frame, area, &mut self.hits);
        }
    }

    /// Render a clickable sponsor button at the right edge of the status bar.
    fn render_donate_button(frame: &mut Frame, area: Rect, hits: &mut HitMap<Click>) {
        let button_width = 12u16;
        let x = area.x + area.width.saturating_sub(button_width);
        let button_rect = Rect::new(x, area.y, button_width, 1).intersection(area);
        hits.push(button_rect, Click::Action(Action::OpenDonate));

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
    fn render_about_overlay(frame: &mut Frame, area: Rect, hits: &mut HitMap<Click>) {
        let width = 46u16.min(area.width.saturating_sub(4));
        let height = 16u16.min(area.height.saturating_sub(4));
        let x = (area.width.saturating_sub(width)) / 2;
        let y = (area.height.saturating_sub(height)) / 2;
        let overlay = Rect::new(area.x + x, area.y + y, width, height);
        Self::register_modal(hits, area, overlay);

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

        let mut content = about_header();
        let links = about_links();
        links.register_centered(click::paragraph_row(inner, content.len()), hits);
        content.push(links.into_line());
        content.extend([
            Line::from(""),
            Line::from(Span::styled(
                "Apache-2.0",
                Style::default().fg(theme::text_muted()),
            )),
            Line::from(""),
        ]);
        let close = close_button("Esc or a to close");
        close.register_centered(click::paragraph_row(inner, content.len()), hits);
        content.push(close.into_line());

        frame.render_widget(
            Paragraph::new(content).alignment(ratatui::layout::Alignment::Center),
            inner,
        );
    }

    /// Render the help overlay centered on screen. The underlying screen
    /// stays visible around the modal — we only clear the overlay's own
    /// rect so the rest of the TUI shows through.
    fn render_help_overlay(frame: &mut Frame, area: Rect, hits: &mut HitMap<Click>) {
        let help_width = 60u16.min(area.width.saturating_sub(4));
        let help_height = 23u16.min(area.height.saturating_sub(4));
        let x = (area.width.saturating_sub(help_width)) / 2;
        let y = (area.height.saturating_sub(help_height)) / 2;
        let help_area = Rect::new(area.x + x, area.y + y, help_width, help_height);
        Self::register_modal(hits, area, help_area);

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

        let mut help_text = help_lines();
        let mut close = ClickLine::default();
        close.push_gap(Span::raw("                         "));
        close.append(close_button("Esc or ? to close"));
        help_text.push(close.draw(click::paragraph_row(inner, help_text.len()), hits));

        frame.render_widget(Paragraph::new(help_text), inner);
    }

    /// Render a centered confirmation dialog. The underlying screen
    /// stays visible around the dialog — only the dialog's own rect is
    /// cleared so context behind the prompt remains legible.
    fn render_confirm_dialog(
        frame: &mut Frame,
        area: Rect,
        confirm: &ConfirmAction,
        hits: &mut HitMap<Click>,
    ) {
        let width = 50u16.min(area.width.saturating_sub(4));
        let height = 5u16;
        let x = (area.width.saturating_sub(width)) / 2;
        let y = (area.height.saturating_sub(height)) / 2;
        let dialog_area = Rect::new(area.x + x, area.y + y, width, height).intersection(area);
        Self::register_modal(hits, area, dialog_area);

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

        let mut answers = click::key_hints(&[
            Hint::button("y", "confirm", click::char_key('y')),
            Hint::button("n", "cancel", click::char_key('n')),
        ]);
        let answer_row = Rect {
            y: inner.y.saturating_add(2),
            height: 1,
            ..inner
        }
        .intersection(inner);
        let text = vec![
            Line::from(Span::styled(
                format!("  {confirm}"),
                Style::default().fg(theme::text_secondary()),
            )),
            Line::from(""),
            std::mem::take(&mut answers).draw(answer_row, hits),
        ];

        frame.render_widget(Paragraph::new(text), inner);
    }

    /// Render a notification toast in the bottom-right corner. A tap on it
    /// dismisses it.
    fn render_notification(
        frame: &mut Frame,
        area: Rect,
        notif: &Notification,
        hits: &mut HitMap<Click>,
    ) {
        let msg_len = u16::try_from(notif.message.len()).unwrap_or(u16::MAX);
        let width = (msg_len + 6).clamp(20, 60);
        let height = 3u16;
        let x = area.width.saturating_sub(width + 1);
        let y = area.height.saturating_sub(height + 2);
        let toast_area = Rect::new(area.x + x, area.y + y, width, height).intersection(area);
        hits.push(toast_area, Click::Action(Action::DismissNotification));

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

/// The About overlay's text above its links.
fn about_header() -> Vec<Line<'static>> {
    vec![
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
    ]
}

/// The About overlay's donate and GitHub links, which press `d` and `g`.
fn about_links() -> ClickLine<'static, Click> {
    let donate = Click::Key(click::char_key('d'));
    let github = Click::Key(click::char_key('g'));
    let mut links = ClickLine::default();
    links.push(
        donate.clone(),
        Span::styled(
            "\u{2665} ",
            Style::default()
                .fg(theme::accent_primary())
                .add_modifier(Modifier::BOLD),
        ),
    );
    links.push(donate.clone(), Span::styled("d", theme::key_hint_key()));
    links.push(
        donate,
        Span::styled("onate", Style::default().fg(theme::text_secondary())),
    );
    links.push_gap(Span::raw("     "));
    links.push(github.clone(), Span::styled("g", theme::key_hint_key()));
    links.push(
        github,
        Span::styled("ithub", Style::default().fg(theme::text_secondary())),
    );
    links
}

/// An overlay's close hint, which presses `Esc`.
fn close_button(text: &'static str) -> ClickLine<'static, Click> {
    let mut close = ClickLine::default();
    close.push(
        Click::Key(KeyCode::Esc.into()),
        Span::styled(text, theme::key_hint()),
    );
    close
}

/// The help overlay's shortcut table, above its close hint.
fn help_lines() -> Vec<Line<'static>> {
    vec![
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
            Span::styled("Quit               ", theme::key_hint()),
            Span::styled("^r ", theme::key_hint_key()),
            Span::styled("Retry connect", theme::key_hint()),
        ]),
        Line::from(vec![
            Span::styled("  Mouse       ", theme::key_hint_key()),
            Span::styled(
                "Tap tabs, rows, and key hints; wheel scrolls",
                theme::key_hint(),
            ),
        ]),
        Line::from(""),
    ]
}
