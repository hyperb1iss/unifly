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
use crate::tui::action::Action;
use crate::tui::screen::ScreenId;
use crate::tui::theme;
use crate::tui::widgets::click::{self, Click, ClickLine, Hint, HitMap};
use crate::tui::widgets::wrap::wrap;

/// Columns the sponsor button takes: ` ♥ Sponsor `.
const DONATE_WIDTH: u16 = 11;

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

        // Setup and Settings draw the whole frame, with no tab or status
        // bar. Toasts and dialogs still go over them: what they report (a
        // keyring fallback, a failed save) has to be seen, and what they
        // ask has to be answered.
        if self.fills_frame() {
            if let Some(screen) = self.screens.get(&self.active_screen) {
                screen.render(frame, area);
            }
            if self.effects_enabled && self.effects.is_active() {
                self.effects.process(delta, frame.buffer_mut(), area);
            }
            self.render_toasts(frame, area);
            self.render_overlays(frame, area);
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

        self.render_toasts(frame, content_area);

        self.render_overlays(frame, area);
    }

    /// Whether the active screen takes the whole frame, chrome and all.
    pub(super) fn fills_frame(&self) -> bool {
        self.active_screen == ScreenId::Setup || self.active_screen == ScreenId::Settings
    }

    /// Draw the dialogs and overlays that sit on top of any screen.
    fn render_overlays(&mut self, frame: &mut Frame, area: Rect) {
        if let Some(confirm) = &self.pending_confirm {
            let prompt = self.shown_text(&confirm.prompt);
            let detail = confirm.detail.as_deref().map(|text| self.shown_text(text));
            Self::render_confirm_dialog(frame, area, &prompt, detail.as_deref(), &mut self.hits);
        }

        if let Some((request, input)) = &self.prompt {
            let title = self.shown_text(&request.title);
            Self::render_prompt(
                frame,
                area,
                &title,
                (request, input, self.prompt_revealed),
                &mut self.hits,
            );
        }

        if self.notices_visible {
            self.render_notices_overlay(frame, area);
        }

        if self.about_visible {
            Self::render_about_overlay(frame, area, &mut self.hits);
        }

        if self.help_visible {
            let screen_keys = self
                .screens
                .get(&self.active_screen)
                .map(|screen| screen.help())
                .unwrap_or_default();
            let searches = self.active_screen_searches();
            let global: Vec<(&'static str, &'static str)> =
                if self.fills_frame() || self.active_screen_captures_input() {
                    FORM_KEYS.to_vec()
                } else {
                    GLOBAL_KEYS
                        .iter()
                        .filter(|(key, _)| searches || *key != "/")
                        .copied()
                        .collect()
                };
            Self::render_help_overlay(
                frame,
                area,
                self.active_screen,
                &screen_keys,
                &global,
                &mut self.hits,
            );
        }
    }

    /// Register a modal's click targets: a tap outside it presses `Esc`
    /// (which closes it or answers no), and a tap inside that misses its
    /// buttons does nothing.
    pub(super) fn register_modal(hits: &mut HitMap<Click>, screen: Rect, modal: Rect) {
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
            // The query takes what the hints leave, scrolling under the
            // cursor when it is longer.
            let room = usize::from(area.width).saturating_sub(3 + 2 + 24).max(8);
            for span in self.search_input.spans(
                room,
                true,
                None,
                Style::default().fg(theme::accent_secondary()),
            ) {
                line.push_gap(span);
            }
            line.push_gap(Span::raw("  "));
            line.push_hints(
                &[
                    Hint::button("Esc", "clear", KeyCode::Esc),
                    Hint::button("Enter", "done", KeyCode::Enter),
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
        let shown = self.status_hints();
        hints.push_hints(&shown, theme::key_hint_key(), theme::key_hint());
        let mut spans = vec![Span::raw(" "), connection_indicator];
        if matches!(self.connection_status, ConnectionStatus::Disconnected)
            && let Some(reason) = &self.connection_error
        {
            // The reason is best-effort context: it must never displace the
            // key hints or run under the sponsor button. Budget from the
            // actual fixed-span widths and drop it when it cannot fit.
            let donate_width = if self.show_donate {
                usize::from(DONATE_WIDTH)
            } else {
                0
            };
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
        // The sponsor button only takes room the hints leave it, so it
        // can never sit on top of one.
        let hints_end = line.width();
        let line = line.draw(area, &mut self.hits);
        frame.render_widget(Paragraph::new(line), area);

        if self.show_donate && hints_end + usize::from(DONATE_WIDTH) < usize::from(area.width) {
            Self::render_donate_button(frame, area, &mut self.hits);
        }
    }

    /// The status bar's buttons. App-wide ones dispatch their action
    /// directly: as keys they would reach the screen first, which may use
    /// the same letter.
    fn status_hints(&self) -> Vec<Hint> {
        let refresh = if self.connection_status == ConnectionStatus::Disconnected {
            Hint::action("^r", "retry", Action::RetryConnect)
        } else {
            Hint::action("^r", "refresh", Action::Refresh)
        };
        let search = self
            .active_screen_searches()
            .then(|| Hint::action("/", "search", Action::OpenSearch));
        // A form has the keyboard: `?`, `,`, and `q` type into its field,
        // so the bar offers only what still works.
        if self.active_screen_captures_input() {
            vec![Hint::info("F1", "help"), Hint::info("^c", "quit")]
        } else {
            [
                Some(Hint::action("?", "help", Action::ToggleHelp)),
                search,
                Some(refresh),
                Some(Hint::action(",", "settings", Action::OpenSettings)),
                Some(Hint::action("q", "quit", Action::Quit)),
            ]
            .into_iter()
            .flatten()
            .collect()
        }
    }

    /// Render a clickable sponsor button at the right edge of the status bar.
    fn render_donate_button(frame: &mut Frame, area: Rect, hits: &mut HitMap<Click>) {
        let x = area.x + area.width.saturating_sub(DONATE_WIDTH);
        let button_rect = Rect::new(x, area.y, DONATE_WIDTH, 1).intersection(area);
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

    /// Render the help overlay centered on screen: the active screen's own
    /// keys first, then the keys that work everywhere, side by side so
    /// the whole thing fits a 24-row terminal.
    fn render_help_overlay(
        frame: &mut Frame,
        area: Rect,
        screen: ScreenId,
        screen_keys: &[(&'static str, &'static str)],
        global: &[(&'static str, &'static str)],
        hits: &mut HitMap<Click>,
    ) {
        let help_width = 78u16.min(area.width.saturating_sub(2));
        let column = usize::from(help_width.saturating_sub(2) / 2);
        let left = help_column(&format!("{screen} screen"), screen_keys, column);
        let right = help_column("Everywhere", global, column);
        let rows = left.len().max(right.len());

        let help_height = u16::try_from(rows + 4)
            .unwrap_or(u16::MAX)
            .min(area.height.saturating_sub(2));
        let x = (area.width.saturating_sub(help_width)) / 2;
        let y = (area.height.saturating_sub(help_height)) / 2;
        let help_area = Rect::new(area.x + x, area.y + y, help_width, help_height);
        Self::register_modal(hits, area, help_area);

        frame.render_widget(Clear, help_area);
        let close = close_button(" Esc to close ");
        let close_row = Rect {
            x: help_area.x.saturating_add(2),
            y: help_area.bottom().saturating_sub(1),
            width: help_area.width.saturating_sub(4),
            height: 1,
        }
        .intersection(help_area);
        close.register(close_row, hits);
        let block = Block::default()
            .title(" Keyboard Shortcuts ")
            .title_style(theme::title_style())
            .title_bottom(close.into_line())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_focused())
            .style(Style::default().bg(theme::bg_base()));

        let inner = block.inner(help_area);
        frame.render_widget(block, help_area);

        let columns = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(Rect {
                y: inner.y.saturating_add(1),
                height: inner.height.saturating_sub(1),
                ..inner
            });
        frame.render_widget(Paragraph::new(left), columns[0]);
        frame.render_widget(Paragraph::new(right), columns[1]);
    }

    /// Render the one-line prompt: what is asked, the field, guidance, and
    /// the two ways out.
    fn render_prompt(
        frame: &mut Frame,
        area: Rect,
        title: &str,
        (request, input, revealed): (
            &crate::tui::action::PromptRequest,
            &crate::tui::widgets::line_input::LineInput,
            bool,
        ),
        hits: &mut HitMap<Click>,
    ) {
        let width = 60u16.min(area.width.saturating_sub(4));
        let text_width = usize::from(width.saturating_sub(6));
        let hint_lines = request
            .hint
            .as_deref()
            .map(|hint| wrap(hint, text_width))
            .unwrap_or_default();
        let height = u16::try_from(hint_lines.len() + 6)
            .unwrap_or(u16::MAX)
            .min(area.height);
        let x = (area.width.saturating_sub(width)) / 2;
        let y = (area.height.saturating_sub(height)) / 2;
        let dialog = Rect::new(area.x + x, area.y + y, width, height).intersection(area);
        Self::register_modal(hits, area, dialog);

        frame.render_widget(Clear, dialog);
        let block = Block::default()
            .title(format!(" {title} "))
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme::border_focused())
            .style(Style::default().bg(theme::bg_base()));
        let inner = block.inner(dialog);
        frame.render_widget(block, dialog);

        let mut field = vec![Span::styled("  \u{203A} ", theme::key_hint_key())];
        field.extend(input.spans(
            text_width.saturating_sub(2),
            true,
            (request.secret && !revealed).then_some('\u{25CF}'),
            Style::default().fg(theme::accent_secondary()),
        ));
        let mut text = vec![
            Line::from(Span::styled(
                format!("  {}", request.label),
                Style::default().fg(theme::text_secondary()),
            )),
            Line::from(field),
        ];
        text.extend(
            hint_lines
                .into_iter()
                .map(|line| Line::from(Span::styled(format!("  {line}"), theme::key_hint()))),
        );
        text.push(Line::from(""));
        let mut keys = vec![
            Hint::button("Enter", "save", KeyCode::Enter),
            Hint::button("Esc", "cancel", KeyCode::Esc),
        ];
        if request.secret {
            let reveal = crossterm::event::KeyEvent::new(
                KeyCode::Char('t'),
                crossterm::event::KeyModifiers::CONTROL,
            );
            let label = if revealed { "hide" } else { "show" };
            keys.push(Hint::button("^t", label, reveal));
        }
        let mut buttons = click::key_hints(&keys);
        let button_row = click::paragraph_row(inner, text.len());
        text.push(std::mem::take(&mut buttons).draw(button_row, hits));
        frame.render_widget(Paragraph::new(text), inner);
    }

    /// Render a centered confirmation dialog, sized to its text: the
    /// question, what saying yes costs, and the two answers. The screen
    /// stays visible around it.
    fn render_confirm_dialog(
        frame: &mut Frame,
        area: Rect,
        prompt: &str,
        detail: Option<&str>,
        hits: &mut HitMap<Click>,
    ) {
        let width = 56u16.min(area.width.saturating_sub(4));
        let text_width = usize::from(width.saturating_sub(6));
        let prompt_lines = wrap(prompt, text_width);
        let detail_lines = detail
            .map(|text| wrap(text, text_width))
            .unwrap_or_default();
        let text_rows = prompt_lines.len() + detail_lines.len();
        let height = u16::try_from(text_rows + 4)
            .unwrap_or(u16::MAX)
            .min(area.height);
        let x = (area.width.saturating_sub(width)) / 2;
        let y = (area.height.saturating_sub(height)) / 2;
        let dialog_area = Rect::new(area.x + x, area.y + y, width, height).intersection(area);
        Self::register_modal(hits, area, dialog_area);

        frame.render_widget(Clear, dialog_area);
        let block = Block::default()
            .title(" Confirm ")
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::warning()))
            .style(Style::default().bg(theme::bg_base()));

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let mut text: Vec<Line> = prompt_lines
            .into_iter()
            .map(|line| {
                Line::from(Span::styled(
                    format!("  {line}"),
                    Style::default()
                        .fg(theme::text_secondary())
                        .add_modifier(Modifier::BOLD),
                ))
            })
            .collect();
        text.extend(
            detail_lines
                .into_iter()
                .map(|line| Line::from(Span::styled(format!("  {line}"), theme::key_hint()))),
        );
        text.push(Line::from(""));
        let mut answers = click::key_hints(&[
            Hint::button("y", "confirm", click::char_key('y')),
            Hint::button("n", "cancel", click::char_key('n')),
        ]);
        let answer_row = click::paragraph_row(inner, text.len());
        text.push(std::mem::take(&mut answers).draw(answer_row, hits));

        frame.render_widget(Paragraph::new(text), inner);
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

/// Keys that work on every screen.
const GLOBAL_KEYS: &[(&str, &str)] = &[
    ("1-9", "Jump to screen"),
    ("Tab", "Next screen"),
    ("Shift+Tab", "Previous screen"),
    ("Esc", "Close, unfilter, or back"),
    ("/", "Search the list"),
    ("^r", "Refresh now, or retry"),
    ("n", "Notifications"),
    (",", "Settings"),
    ("a", "About"),
    ("?  F1", "This help"),
    ("q  ^c", "Quit"),
    ("^l", "Redraw"),
    ("Mouse", "Tap tabs, rows, hints"),
    ("Shift+drag", "Select text"),
];

/// Keys that still work while a full-frame form has the keyboard.
const FORM_KEYS: &[(&str, &str)] = &[
    ("F1", "This help"),
    ("^c", "Quit"),
    ("^l", "Redraw"),
    ("Mouse", "Tap fields and hints"),
    ("Shift+drag", "Select text"),
];

/// The rows [`help_column`] draws for `keys`, without its heading, as
/// plain text.
#[cfg(test)]
pub(super) fn help_column_text(keys: &[(&'static str, &'static str)], width: usize) -> Vec<String> {
    help_column("", keys, width)
        .into_iter()
        .skip(1)
        .map(|line| {
            let text: String = line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect();
            text.trim_end().to_owned()
        })
        .collect()
}

/// Columns a key takes in the help overlay, with its gap.
const HELP_KEY_WIDTH: usize = 14;

/// One column of the help overlay, `width` columns wide: a heading over
/// its keys. A description longer than the column continues on the next
/// row under itself, so none is cut.
fn help_column(
    heading: &str,
    keys: &[(&'static str, &'static str)],
    width: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(
        format!("  {heading}"),
        Style::default()
            .fg(theme::accent_secondary())
            .add_modifier(Modifier::BOLD),
    ))];
    let room = width.saturating_sub(HELP_KEY_WIDTH).max(8);
    for (key, label) in keys {
        let key_width = Span::raw(*key).width();
        // A key wider than its column gets a row to itself.
        let mut key_cell = Some(format!("  {key:<12}"));
        if key_width > 12 {
            lines.push(Line::from(Span::styled(
                format!("  {key}"),
                theme::key_hint_key(),
            )));
            key_cell = Some(" ".repeat(HELP_KEY_WIDTH));
        }
        for part in wrap(label, room) {
            let lead = key_cell
                .take()
                .unwrap_or_else(|| " ".repeat(HELP_KEY_WIDTH));
            lines.push(Line::from(vec![
                Span::styled(lead, theme::key_hint_key()),
                Span::styled(part, theme::key_hint()),
            ]));
        }
    }
    if keys.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Tap a panel to open its screen",
            theme::key_hint(),
        )));
    }
    lines
}
