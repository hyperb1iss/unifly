//! App-side work for the monitoring screens: Dashboard, Topology, Events,
//! and Stats.

use std::sync::Arc;
use std::time::Instant;

use color_eyre::eyre::Result;
use tokio::task::AbortHandle;

use unifly_api::{Command, EventHistoryQuery};

use super::{App, ConnectionStatus};
use crate::tui::action::{Action, Notice, Notification};
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
    /// Counts the controllers this session has had. A fetch remembers the
    /// count it started under, and its result is dropped when it differs.
    generation: u64,
    /// The stats fetch in flight, aborted when a newer one replaces it.
    pub(super) stats_task: Option<AbortHandle>,
    /// The last DPI totals fetched, and when.
    pub(super) dpi: Option<(Instant, super::stats::DpiSnapshot)>,
    /// When alarms were last asked for.
    alarms_fetched_at: Option<Instant>,
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
                // Whatever is still in flight was asked of the controller
                // that just went.
                self.monitor = MonitorState {
                    generation: self.monitor.generation.wrapping_add(1),
                    ..MonitorState::default()
                };
                return Ok(false);
            }
            Action::MonitorFetched { generation, result } => {
                if *generation == self.monitor.generation {
                    self.process_action(result)?;
                }
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
            // Alarms are not in the controller's store, so the refresh an
            // ordinary command ends with would not show an archive.
            Action::Run { command, notice }
                if matches!(
                    **command,
                    Command::ArchiveAlarm { .. } | Command::ArchiveAllAlarms
                ) =>
            {
                self.archive_alarms((**command).clone(), notice);
            }
            Action::Tick => {
                let due = self
                    .monitor
                    .alarms_fetched_at
                    .is_none_or(|at| at.elapsed() >= ALARM_REFRESH);
                if due && self.shows_alarms() && self.can_read_alarms() {
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

    /// Whether the screen in front draws alarms. The timed read runs only
    /// there: on a legacy controller each one is a probe and a full
    /// listing, archived alarms included.
    fn shows_alarms(&self) -> bool {
        matches!(self.active_screen, ScreenId::Dashboard | ScreenId::Events)
    }

    /// Wrap a fetch's result so it is dropped if the controller changes
    /// before it is handled.
    pub(super) fn fenced(&self) -> impl Fn(Action) -> Action + Send + Sync + 'static {
        let generation = self.monitor.generation;
        move |result| Action::MonitorFetched {
            generation,
            result: Box::new(result),
        }
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
        let fenced = self.fenced();
        tokio::spawn(async move {
            let _ = tx.send(fenced(read_alarms(&controller, sanitizer.as_deref()).await));
        });
    }

    /// Archive one alarm or all of them, then read the alarms again
    /// before reporting, so each archive's own re-read shows its result
    /// however many are running. An alarm the controller still lists as
    /// active afterwards is reported, not toasted as archived.
    fn archive_alarms(&mut self, command: Command, notice: &Notice) {
        let Some(controller) = self.controller.clone() else {
            self.show_notification(Notification::error("Not connected"));
            return;
        };
        self.monitor.alarms_fetched_at = Some(Instant::now());
        let tx = self.action_tx.clone();
        let sanitizer = self.sanitizer.clone();
        let fenced = self.fenced();
        let done = notice.done.clone();
        let target = match &command {
            Command::ArchiveAlarm { id } => Some(id.clone()),
            _ => None,
        };
        self.spawn_op(&notice.pending, async move {
            if let Err(error) = controller.execute(command).await {
                tracing::warn!(%error, "archiving alarms failed");
                return Notification::error(format!("{error}"));
            }
            let read = read_alarms(&controller, sanitizer.as_deref()).await;
            let still_active = match (&read, &target) {
                (Action::AlarmsLoaded(alarms), Some(id)) => alarms
                    .iter()
                    .any(|alarm| &alarm.id == id && !alarm.archived),
                (Action::AlarmsLoaded(alarms), None) => alarms.iter().any(|alarm| !alarm.archived),
                _ => false,
            };
            let _ = tx.send(fenced(read));
            if still_active {
                Notification::warning(if target.is_some() {
                    "The controller still lists that alarm as active"
                } else {
                    "The controller still lists active alarms"
                })
            } else {
                Notification::success(done)
            }
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
        let fenced = self.fenced();
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
            let _ = tx.send(fenced(action));
        });
    }
}

/// Read the controller's alarms as the action that reports them, with
/// names and addresses replaced when a sanitizer is on.
async fn read_alarms(
    controller: &unifly_api::Controller,
    sanitizer: Option<&crate::sanitizer::Sanitizer>,
) -> Action {
    match (controller.list_alarms().await, sanitizer) {
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

    /// Process queued actions until `wanted` picks one, and return it. A
    /// fetch's result is judged by what it carries.
    async fn pump(app: &mut App, wanted: impl Fn(&Action) -> bool) -> Action {
        loop {
            let action =
                tokio::time::timeout(std::time::Duration::from_secs(5), app.action_rx.recv())
                    .await
                    .expect("an action in time")
                    .expect("the channel stays open");
            app.process_action(&action).expect("process");
            let action = match action {
                Action::MonitorFetched { result, .. } => *result,
                other => other,
            };
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
    async fn archives_started_back_to_back_both_leave_the_list() {
        let mut app = demo_app().await;
        app.process_action(&Action::Connected).expect("connected");
        app.process_action(&Action::AccessUpdated(FULL_ACCESS))
            .expect("access");
        app.process_action(&Action::Tick).expect("tick");
        let first = pump(&mut app, |action| matches!(action, Action::AlarmsLoaded(_))).await;
        let Action::AlarmsLoaded(alarms) = first else {
            unreachable!()
        };
        let active: Vec<_> = alarms.iter().filter(|alarm| !alarm.archived).collect();
        assert!(active.len() >= 2, "the demo fleet has two standing alarms");

        // The second archive starts before the first has finished.
        for alarm in &active[..2] {
            app.process_action(&Action::Run {
                command: Box::new(Command::ArchiveAlarm {
                    id: alarm.id.clone(),
                }),
                notice: Notice::new("Archiving", "Archived"),
            })
            .expect("run");
        }

        let mut finished = 0;
        let mut listed = None;
        while finished < 2 {
            let action = pump(&mut app, |action| {
                matches!(action, Action::AlarmsLoaded(_) | Action::OpFinished { .. })
            })
            .await;
            match action {
                Action::AlarmsLoaded(alarms) => {
                    listed = Some(alarms.iter().filter(|alarm| !alarm.archived).count());
                }
                Action::OpFinished { notification, .. } => {
                    assert_eq!(notification.message, "Archived");
                    finished += 1;
                }
                _ => unreachable!(),
            }
        }
        assert_eq!(listed, Some(active.len() - 2), "each archive re-read");
    }

    #[tokio::test]
    async fn a_result_that_outlives_its_controller_is_dropped() {
        let mut app = demo_app().await;
        app.process_action(&Action::RequestEventHistory {
            within_hours: 24,
            limit: 50,
            include_firewall: false,
        })
        .expect("request");
        let asked_under = app.monitor.generation;
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), app.action_rx.recv())
            .await
            .expect("the fetch reports")
            .expect("the channel stays open");
        assert!(
            matches!(result, Action::MonitorFetched { generation, .. } if generation == asked_under),
            "{result:?}"
        );

        // `Quit` stands in for a result: it shows whether it was acted on.
        let stale = app.fenced()(Action::Quit);
        app.process_action(&Action::ControllerChanged)
            .expect("switch");
        assert_ne!(app.monitor.generation, asked_under);
        app.process_action(&stale).expect("stale result");
        app.process_action(&result).expect("stale history");
        assert!(app.running, "a result for the old controller is dropped");

        let current = app.fenced()(Action::Quit);
        app.process_action(&current).expect("current result");
        assert!(!app.running, "a result for this controller is acted on");
    }

    #[tokio::test]
    async fn alarms_are_only_polled_where_they_are_drawn() {
        let mut app = demo_app().await;
        app.process_action(&Action::Connected).expect("connected");
        app.access = FULL_ACCESS;

        app.active_screen = ScreenId::Devices;
        app.process_action(&Action::Tick).expect("tick");
        assert!(app.monitor.alarms_fetched_at.is_none());

        app.active_screen = ScreenId::Events;
        app.process_action(&Action::Tick).expect("tick");
        assert!(app.monitor.alarms_fetched_at.is_some());
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
