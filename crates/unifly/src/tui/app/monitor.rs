//! App-side work for the monitoring screens: Dashboard, Topology, Events,
//! and Stats.

use std::sync::Arc;
use std::time::Instant;

use color_eyre::eyre::Result;
use tokio::task::AbortHandle;

use unifly_api::{Command, EventHistoryQuery};

use super::{App, ConnectionStatus};
use crate::tui::action::Action;
use crate::tui::screen::ScreenId;

/// The screens that draw connection state themselves.
const MONITOR_SCREENS: [ScreenId; 4] = [
    ScreenId::Dashboard,
    ScreenId::Topology,
    ScreenId::Events,
    ScreenId::Stats,
];

/// How often alarms are read again while connected.
const ALARM_REFRESH: std::time::Duration = std::time::Duration::from_mins(1);

/// In-flight work and caches for the monitoring screens.
#[derive(Default)]
pub(super) struct MonitorState {
    /// The stats fetch in flight, aborted when a newer one replaces it.
    pub(super) stats_task: Option<AbortHandle>,
    /// The last DPI totals fetched, and when.
    pub(super) dpi: Option<(Instant, super::stats::DpiSnapshot)>,
    /// When alarms were last asked for.
    alarms_fetched_at: Option<Instant>,
    /// An archive is running: alarms are read again when it finishes.
    alarms_stale: bool,
    /// The controller connected and has not been lost since. A failed
    /// refresh leaves this set: the clients are still up.
    connected: bool,
}

impl App {
    /// Handle the monitoring screens' actions. Returns whether `action`
    /// was consumed.
    pub(super) fn handle_monitor_action(&mut self, action: &Action) -> Result<bool> {
        match action {
            Action::Connecting
            | Action::Connected
            | Action::Disconnected(_)
            | Action::Reconnecting { .. } => {
                for screen in MONITOR_SCREENS {
                    self.forward_to_screen(screen, action)?;
                }
                // A link that came back reloads the statistics on screen.
                // The status still holds what it was before this action.
                let came_back = matches!(action, Action::Connected)
                    && self.connection_status != ConnectionStatus::Connected;
                match action {
                    Action::Connected => self.monitor.connected = true,
                    Action::Connecting | Action::Disconnected(_) => self.monitor.connected = false,
                    _ => {}
                }
                if came_back && self.access.session {
                    self.reload_visible_stats()?;
                }
                return Ok(false);
            }
            // Statistics wait for session access, which is known only now.
            Action::AccessUpdated(access) => {
                if access.session {
                    self.reload_visible_stats()?;
                    // Read on the next tick, once this access is recorded.
                    self.monitor.alarms_fetched_at = None;
                }
                return Ok(false);
            }
            Action::ControllerChanged => {
                if let Some(task) = self.monitor.stats_task.take() {
                    task.abort();
                }
                self.monitor = MonitorState::default();
                return Ok(false);
            }
            Action::StatsUpdated(data) => {
                self.remember_dpi(data);
                return Ok(false);
            }
            Action::StatsLoading(_) | Action::StatsFailed { .. } => {
                self.forward_to_screen(ScreenId::Stats, action)?;
            }
            Action::RequestEventHistory {
                within_hours,
                limit,
                include_firewall,
            } => self.fetch_event_history(EventHistoryQuery {
                within_hours: *within_hours,
                limit: *limit,
                include_firewall: *include_firewall,
            }),
            Action::EventHistoryLoaded { .. } | Action::EventHistoryFailed(_) => {
                self.forward_to_screen(ScreenId::Events, action)?;
            }
            Action::RequestAlarms => self.fetch_alarms(),
            Action::AlarmsLoaded(_) | Action::AlarmsFailed(_) => {
                self.forward_to_screen(ScreenId::Events, action)?;
                self.forward_to_screen(ScreenId::Dashboard, action)?;
            }
            // Alarms are not in the controller's store, so the refresh a
            // command ends with does not pick up an archive.
            Action::Run { command, .. }
                if matches!(
                    **command,
                    Command::ArchiveAlarm { .. } | Command::ArchiveAllAlarms
                ) =>
            {
                self.monitor.alarms_stale = true;
                return Ok(false);
            }
            Action::OpFinished { .. } if self.monitor.alarms_stale => {
                self.monitor.alarms_stale = false;
                self.fetch_alarms();
                return Ok(false);
            }
            Action::Tick => {
                let due = self
                    .monitor
                    .alarms_fetched_at
                    .is_none_or(|at| at.elapsed() >= ALARM_REFRESH);
                if due && self.can_read_alarms() {
                    self.fetch_alarms();
                }
                return Ok(false);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Queue a reload of the Stats screen's period when it is on screen.
    /// Queued, so it runs once the action being handled has settled.
    fn reload_visible_stats(&self) -> Result<()> {
        if self.active_screen == ScreenId::Stats {
            self.action_tx
                .send(Action::RequestStats(self.stats_period))?;
        }
        Ok(())
    }

    /// Whether the controller can be asked for data. `Reconnecting` after
    /// a connect reports a refresh that failed while the clients are still
    /// up, so requests go on; after a failed connect it is the next
    /// attempt, and there is nothing to ask.
    pub(super) fn controller_reachable(&self) -> bool {
        match self.connection_status {
            ConnectionStatus::Connected => true,
            ConnectionStatus::Reconnecting { .. } => self.monitor.connected,
            _ => false,
        }
    }

    /// Whether an alarm read can succeed: alarms come over session access.
    fn can_read_alarms(&self) -> bool {
        self.controller_reachable() && self.access.session
    }

    /// Fetch the controller's alarms for the Events screen and the
    /// Dashboard's count. A failure is reported inline, never toasted,
    /// since the read repeats on a timer.
    fn fetch_alarms(&mut self) {
        self.monitor.alarms_fetched_at = Some(Instant::now());
        let tx = self.action_tx.clone();
        let Some(controller) = self.controller.clone() else {
            let _ = tx.send(Action::AlarmsFailed("not connected".into()));
            return;
        };
        let sanitizer = self.sanitizer.clone();
        tokio::spawn(async move {
            let action = match (controller.list_alarms().await, &sanitizer) {
                (Ok(alarms), Some(sanitizer)) => Action::AlarmsLoaded(Arc::new(
                    alarms
                        .iter()
                        .map(|alarm| sanitizer.sanitize_alarm(alarm))
                        .collect(),
                )),
                (Ok(alarms), None) => Action::AlarmsLoaded(Arc::new(alarms)),
                (Err(error), Some(sanitizer)) => {
                    Action::AlarmsFailed(sanitizer.sanitize_text(&error.to_string()))
                }
                (Err(error), None) => Action::AlarmsFailed(error.to_string()),
            };
            let _ = tx.send(action);
        });
    }

    /// Fetch a window of event history and hand it to the Events screen.
    fn fetch_event_history(&self, query: EventHistoryQuery) {
        let tx = self.action_tx.clone();
        let Some(controller) = self.controller.clone() else {
            let _ = tx.send(Action::EventHistoryFailed("not connected".into()));
            return;
        };
        let sanitizer = self.sanitizer.clone();
        tokio::spawn(async move {
            let action = match controller.list_events(query).await {
                Ok(events) => {
                    let events: Vec<_> = events.into_iter().map(Arc::new).collect();
                    let events = match &sanitizer {
                        Some(sanitizer) => sanitizer.sanitize_events_vec(&events),
                        None => events,
                    };
                    Action::EventHistoryLoaded {
                        within_hours: query.within_hours,
                        include_firewall: query.include_firewall,
                        events: Arc::new(events),
                    }
                }
                Err(error) => Action::EventHistoryFailed(match &sanitizer {
                    Some(sanitizer) => sanitizer.sanitize_text(&error.to_string()),
                    None => error.to_string(),
                }),
            };
            let _ = tx.send(action);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::action::Access;

    async fn demo_app() -> App {
        let demo = unifly_api::Controller::demo(unifly_api::DemoFleetOptions::default());
        demo.connect().await.expect("demo fleet connects");
        App::new(Some(demo), None, false, true)
    }

    /// Process queued actions until `wanted` picks one, and return it.
    async fn pump(app: &mut App, wanted: impl Fn(&Action) -> bool) -> Action {
        loop {
            let action =
                tokio::time::timeout(std::time::Duration::from_secs(5), app.action_rx.recv())
                    .await
                    .expect("an action in time")
                    .expect("the channel stays open");
            app.process_action(&action).expect("process");
            if wanted(&action) {
                return action;
            }
        }
    }

    /// Everything reachable, as a controller with session access reports.
    const FULL_ACCESS: Access = Access {
        integration: true,
        session: true,
        live_events: true,
    };

    #[tokio::test]
    async fn a_history_request_comes_back_as_events_for_the_events_screen() {
        let mut app = demo_app().await;
        app.process_action(&Action::RequestEventHistory {
            within_hours: 24 * 30,
            limit: 50,
            include_firewall: false,
        })
        .expect("request");

        let loaded = pump(&mut app, |action| {
            matches!(action, Action::EventHistoryLoaded { .. })
        })
        .await;
        let Action::EventHistoryLoaded { events, .. } = loaded else {
            unreachable!()
        };
        assert!(!events.is_empty() && events.len() <= 50, "{}", events.len());
    }

    #[tokio::test]
    async fn alarms_are_read_once_connected_and_again_after_an_archive() {
        let mut app = demo_app().await;
        app.process_action(&Action::Connected).expect("connected");
        app.process_action(&Action::AccessUpdated(FULL_ACCESS))
            .expect("access");
        app.process_action(&Action::Tick).expect("tick");

        let active = |action: &Action| match action {
            Action::AlarmsLoaded(alarms) => {
                Some(alarms.iter().filter(|alarm| !alarm.archived).count())
            }
            _ => None,
        };
        let first = pump(&mut app, |action| active(action).is_some()).await;
        let before = active(&first).expect("alarms");
        assert!(before > 0, "the demo fleet has standing alarms");
        // The next tick does not read again: the last read is fresh.
        app.process_action(&Action::Tick).expect("tick");

        let Action::AlarmsLoaded(alarms) = first else {
            unreachable!()
        };
        let id = alarms
            .iter()
            .find(|alarm| !alarm.archived)
            .expect("an active alarm")
            .id
            .clone();
        app.process_action(&Action::Run {
            command: Box::new(Command::ArchiveAlarm { id }),
            notice: crate::tui::action::Notice::new("Archiving", "Archived"),
        })
        .expect("run");

        let after = pump(&mut app, |action| active(action).is_some()).await;
        assert_eq!(active(&after), Some(before - 1));
    }

    #[tokio::test]
    async fn alarms_are_still_read_after_one_failed_refresh() {
        let mut app = demo_app().await;
        app.process_action(&Action::Connected).expect("connected");
        app.access = FULL_ACCESS;
        app.process_action(&Action::Reconnecting { attempt: 1 })
            .expect("a refresh failed");

        app.process_action(&Action::Tick).expect("tick");
        assert!(app.monitor.alarms_fetched_at.is_some());
    }

    #[tokio::test]
    async fn alarms_are_not_read_while_a_failed_connect_is_retried() {
        let mut app = demo_app().await;
        app.access = FULL_ACCESS;
        app.process_action(&Action::Disconnected("refused".into()))
            .expect("disconnected");
        app.process_action(&Action::Reconnecting { attempt: 2 })
            .expect("retrying");

        app.process_action(&Action::Tick).expect("tick");
        assert!(app.monitor.alarms_fetched_at.is_none());
    }

    #[tokio::test]
    async fn alarms_are_not_read_without_session_access() {
        let mut app = demo_app().await;
        app.process_action(&Action::Connected).expect("connected");
        app.access = Access {
            integration: true,
            ..Access::default()
        };

        app.process_action(&Action::Tick).expect("tick");

        assert!(app.monitor.alarms_fetched_at.is_none());
    }

    #[test]
    fn a_history_request_without_a_controller_fails_inline() {
        let mut app = App::new(None, None, false, true);
        app.process_action(&Action::RequestEventHistory {
            within_hours: 24,
            limit: 50,
            include_firewall: false,
        })
        .expect("request");

        let failed = app.action_rx.try_recv().expect("a failure is reported");
        assert!(
            matches!(failed, Action::EventHistoryFailed(_)),
            "{failed:?}"
        );
    }
}
