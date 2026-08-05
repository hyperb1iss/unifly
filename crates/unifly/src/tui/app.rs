//! Application core — event loop, screen management, action dispatch.

mod commands;
mod dispatch;
mod lifecycle;
mod navigation;
mod render;
mod screens;
mod stats;
mod wifi;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use color_eyre::eyre::Result;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::info;

use unifly_api::Controller;

use crate::sanitizer::Sanitizer;
use crate::tui::action::{Action, ConfirmAction, Notification, StatsPeriod};
use crate::tui::component::Component;
use crate::tui::effects::EffectStack;
use crate::tui::event::{Event, EventReader};
use crate::tui::screen::ScreenId;
use crate::tui::screens::create_screens;
use crate::tui::terminal::Tui;

/// Connection status as seen by the TUI.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ConnectionStatus {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Reconnecting {
        attempt: u32,
    },
}

/// Top-level application state and event loop.
#[allow(clippy::struct_excessive_bools)]
pub struct App {
    /// Current active screen.
    active_screen: ScreenId,
    /// Previous screen for GoBack.
    previous_screen: Option<ScreenId>,
    /// All screen components, keyed by ScreenId.
    screens: HashMap<ScreenId, Box<dyn Component>>,
    /// Whether the app should keep running.
    running: bool,
    /// Connection status indicator.
    connection_status: ConnectionStatus,
    /// Why the last connect attempt failed, shown next to the indicator.
    connection_error: Option<String>,
    /// Help overlay visibility.
    help_visible: bool,
    /// About overlay visibility.
    about_visible: bool,
    /// Search overlay visibility.
    search_active: bool,
    /// Current search query.
    search_query: String,
    /// Terminal size for responsive layout.
    terminal_size: (u16, u16),
    /// Action sender — components can dispatch actions through this.
    action_tx: mpsc::UnboundedSender<Action>,
    /// Action receiver — main loop drains this.
    action_rx: mpsc::UnboundedReceiver<Action>,
    /// Optional controller for live data.
    controller: Option<Controller>,
    /// PII sanitizer for demo mode (None when demo mode is off).
    sanitizer: Option<Arc<Sanitizer>>,
    /// Cancellation token for the data bridge task.
    data_cancel: CancellationToken,
    /// Handle of the newest data bridge task — successors chain on it so
    /// bridge lifetimes never overlap on the same controller.
    bridge_handle: Option<tokio::task::JoinHandle<()>>,
    /// Pending confirmation dialog (blocks other input while active).
    pending_confirm: Option<ConfirmAction>,
    /// Active notification toast with display timestamp.
    notification: Option<(Notification, Instant)>,
    /// Generation counter for stats requests — prevents stale responses from
    /// overwriting fresh data when the user rapidly switches periods.
    stats_generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// Timestamp of the last stats fetch — drives auto-refresh.
    last_stats_fetch: Option<std::time::Instant>,
    /// Currently selected stats period — preserved for auto-refresh.
    stats_period: StatsPeriod,
    /// Whether to show the donate button in the status bar.
    show_donate: bool,
    /// tachyonfx effect stack applied as buffer post-processing.
    effects: EffectStack,
    /// Whether effects are enabled this session (flag + env + config).
    effects_enabled: bool,
    /// Timestamp of the previous rendered frame — used to compute per-frame
    /// delta time for the effect stack.
    last_frame: Instant,
    /// Whether the next render tick should draw a frame.
    needs_redraw: bool,
}

impl App {
    /// Create a new App with all screens. Optionally accepts a [`Controller`]
    /// for live data — if `None`, the TUI shows the onboarding wizard.
    pub fn new(
        controller: Option<Controller>,
        sanitizer: Option<Arc<Sanitizer>>,
        effects_enabled: bool,
    ) -> Self {
        let show_donate = crate::config::load_config().map_or(true, |c| c.defaults.show_donate);

        let (action_tx, action_rx) = mpsc::unbounded_channel();

        let mut screens: HashMap<ScreenId, Box<dyn Component>> =
            create_screens().into_iter().collect();

        // If no controller, show the onboarding wizard instead of the dashboard
        let active_screen = if controller.is_none() {
            screens.insert(
                ScreenId::Setup,
                Box::new(crate::tui::screens::onboarding::OnboardingScreen::new()),
            );
            ScreenId::Setup
        } else {
            ScreenId::Dashboard
        };

        Self {
            active_screen,
            previous_screen: None,
            screens,
            running: true,
            connection_status: ConnectionStatus::default(),
            connection_error: None,
            help_visible: false,
            about_visible: false,
            search_active: false,
            search_query: String::new(),
            terminal_size: (0, 0),
            action_tx,
            action_rx,
            controller,
            sanitizer,
            data_cancel: CancellationToken::new(),
            bridge_handle: None,
            pending_confirm: None,
            notification: None,
            stats_generation: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            last_stats_fetch: None,
            stats_period: StatsPeriod::default(),
            show_donate,
            effects: EffectStack::new(),
            effects_enabled,
            last_frame: Instant::now(),
            needs_redraw: true,
        }
    }

    /// Initialize all screen components with the action sender.
    fn init_screens(&mut self) -> Result<()> {
        for screen in self.screens.values_mut() {
            screen.init(self.action_tx.clone())?;
        }

        if let Some(screen) = self.screens.get_mut(&self.active_screen) {
            screen.set_focused(true);
        }

        Ok(())
    }

    /// Run the main event loop. This is the heart of the TUI.
    pub async fn run(&mut self) -> Result<()> {
        let mut tui = Tui::new()?;
        tui.enter()?;
        #[cfg(feature = "tui-graphics")]
        {
            let protocol = crate::tui::graphics::probe_stdio();
            crate::tui::render_caps::set_graphics_protocol(protocol);
            info!(?protocol, "graphics chart protocol probe complete");
        }
        self.terminal_size = tui.size().unwrap_or((80, 24));
        self.init_screens()?;

        if let Some(controller) = self.controller.clone() {
            self.spawn_data_bridge(controller);
        }

        // Reset the frame clock so the first draw gets a sane delta, then
        // queue the launch intro effect if effects are enabled this session.
        self.last_frame = Instant::now();
        if self.effects_enabled {
            self.effects.start_intro();
        }

        let mut events = EventReader::new(Duration::from_millis(250), Duration::from_millis(33));

        info!("TUI event loop started");

        while self.running {
            let Some(event) = events.next().await else {
                break;
            };

            match event {
                Event::Key(key) => {
                    if let Some(action) = self.handle_key_event(key)? {
                        self.action_tx.send(action)?;
                    }
                }
                Event::Mouse(mouse) => {
                    if let Some(action) = self.handle_mouse_event(mouse)? {
                        self.action_tx.send(action)?;
                    }
                }
                Event::Resize(w, h) => {
                    self.action_tx.send(Action::Resize(w, h))?;
                }
                Event::Tick => {
                    self.action_tx.send(Action::Tick)?;
                }
                Event::Render => {
                    #[cfg(feature = "tui-graphics")]
                    if crate::tui::graphics::poll_ready_charts() {
                        self.needs_redraw = true;
                    }
                    self.action_tx.send(Action::Render)?;
                }
            }

            while let Ok(action) = self.action_rx.try_recv() {
                self.process_action(&action)?;

                if let Action::Render = action
                    && self.should_draw()
                {
                    tui.draw(|frame| self.render(frame))?;
                    self.needs_redraw = false;
                }
            }
        }

        self.data_cancel.cancel();
        events.stop();
        info!("TUI event loop ended");
        Ok(())
    }

    pub(super) fn should_draw(&self) -> bool {
        crate::tui::render_scheduler::should_draw(
            self.needs_redraw,
            self.effects_enabled && self.effects.is_active(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_gate_skips_clean_static_frames() {
        let mut app = App::new(None, None, false);

        assert!(app.should_draw());

        app.needs_redraw = false;
        app.process_action(&Action::Render)
            .expect("render action should be handled");

        assert!(!app.should_draw());
    }

    #[test]
    fn render_gate_reopens_after_state_change() {
        let mut app = App::new(None, None, false);
        app.needs_redraw = false;

        app.process_action(&Action::Resize(120, 40))
            .expect("resize should be handled");

        assert!(app.should_draw());
    }

    #[test]
    fn chart_peak_starts_effect_when_enabled() {
        let mut app = App::new(None, None, true);

        app.process_action(&Action::ChartPeak)
            .expect("chart peak should be handled");

        assert!(app.effects.is_active());
        assert!(app.should_draw());
    }

    #[test]
    fn chart_peak_respects_disabled_effects() {
        let mut app = App::new(None, None, false);

        app.process_action(&Action::ChartPeak)
            .expect("chart peak should be handled");

        assert!(!app.effects.is_active());
        assert!(app.should_draw());
    }
}
