//! App-side work for the monitoring screens: Dashboard, Topology, Events,
//! and Stats.

use std::sync::Arc;
use std::time::Instant;

use color_eyre::eyre::Result;
use tokio::task::AbortHandle;

use unifly_api::EventHistoryQuery;

use super::App;
use crate::tui::action::{Access, Action};
use crate::tui::screen::ScreenId;

/// The screens that draw connection state themselves.
const MONITOR_SCREENS: [ScreenId; 4] = [
    ScreenId::Dashboard,
    ScreenId::Topology,
    ScreenId::Events,
    ScreenId::Stats,
];

/// What the demo fleet can reach: everything, since it answers every query
/// from its simulation.
const DEMO_ACCESS: Access = Access {
    integration: true,
    session: true,
    live_events: true,
};

/// In-flight work and caches for the monitoring screens.
#[derive(Default)]
pub(super) struct MonitorState {
    /// The stats fetch in flight, aborted when a newer one replaces it.
    pub(super) stats_task: Option<AbortHandle>,
    /// The last DPI totals fetched, and when.
    pub(super) dpi: Option<(Instant, super::stats::DpiSnapshot)>,
}

impl App {
    /// Handle the monitoring screens' actions. Returns whether `action`
    /// was consumed.
    pub(super) fn handle_monitor_action(&mut self, action: &Action) -> Result<bool> {
        match action {
            // The controller reports access by which API clients it holds,
            // and the demo fleet holds none, so its report would switch off
            // everything the simulation serves.
            Action::AccessUpdated(access) if self.demo_fleet_active() && *access != DEMO_ACCESS => {
                self.action_tx.send(Action::AccessUpdated(DEMO_ACCESS))?;
            }
            Action::Connecting
            | Action::Connected
            | Action::Disconnected(_)
            | Action::Reconnecting { .. } => {
                for screen in MONITOR_SCREENS {
                    self.forward_to_screen(screen, action)?;
                }
                // A link that came back reloads the statistics on screen.
                if matches!(action, Action::Connected) && self.access.session {
                    self.reload_visible_stats()?;
                }
                return Ok(false);
            }
            // Statistics wait for session access, which is known only now.
            Action::AccessUpdated(access) => {
                if access.session {
                    self.reload_visible_stats()?;
                }
                return Ok(false);
            }
            Action::ControllerChanged => {
                if let Some(task) = self.monitor.stats_task.take() {
                    task.abort();
                }
                self.monitor.dpi = None;
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

    #[tokio::test]
    async fn the_demo_fleet_reports_everything_it_simulates_as_reachable() {
        let mut app = demo_app().await;
        let bare = Access::default();

        app.process_action(&Action::AccessUpdated(bare))
            .expect("access");
        assert_eq!(app.access, bare, "the clientless report is held back");

        pump(&mut app, |action| {
            matches!(action, Action::AccessUpdated(_))
        })
        .await;
        assert_eq!(app.access, DEMO_ACCESS);
    }

    #[test]
    fn a_real_controllers_access_report_is_left_alone() {
        let mut app = App::new(None, None, false, true);
        let cloud = Access {
            integration: true,
            ..Access::default()
        };

        app.process_action(&Action::AccessUpdated(cloud))
            .expect("access");

        assert_eq!(app.access, cloud);
    }

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
