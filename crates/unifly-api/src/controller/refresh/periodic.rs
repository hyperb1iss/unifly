use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::core_error::CoreError;

use super::super::{ConnectionState, Controller};

/// Periodically refresh data from the controller.
///
/// A refresh that fails means the controller stopped answering, so the
/// connection state says so: `Reconnecting` counts the failed refreshes in
/// a row, and the first one that succeeds puts it back to `Connected`.
/// Without this a link lost after a good connect leaves frozen data under
/// a status that still reads connected.
pub(in crate::controller) async fn refresh_task(
    controller: Controller,
    interval_secs: u64,
    cancel: CancellationToken,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval.tick().await;
    let mut failures: u32 = 0;

    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => break,
            _ = interval.tick() => {
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => break,
                    result = controller.full_refresh() => {
                        failures = controller.note_refresh(&result, failures);
                    }
                }
            }
        }
    }
}

impl Controller {
    /// Record how a periodic refresh went in the connection state, and
    /// return the new count of failures in a row. One failed refresh is a
    /// slow answer or a hiccup; the state says `Reconnecting` from the
    /// second, and any refresh that succeeds says `Connected` again.
    fn note_refresh(&self, result: &Result<(), CoreError>, failures: u32) -> u32 {
        match result {
            Ok(()) => 0,
            Err(error) => {
                let failures = failures.saturating_add(1);
                warn!(error = %error, failures, "periodic refresh failed");
                if failures >= FAILURES_BEFORE_RECONNECTING {
                    self.inner
                        .connection_state
                        .send_if_modified(|state| match state {
                            ConnectionState::Connected | ConnectionState::Reconnecting { .. } => {
                                *state = ConnectionState::Reconnecting { attempt: failures };
                                true
                            }
                            // A connect or disconnect in progress owns the
                            // state; a late refresh result must not undo it.
                            _ => false,
                        });
                }
                failures
            }
        }
    }

    /// A refresh fetched everything: the controller is answering, whatever
    /// earlier refreshes reported.
    pub(in crate::controller) fn note_refresh_succeeded(&self) {
        self.inner.connection_state.send_if_modified(|state| {
            let recovering = matches!(state, ConnectionState::Reconnecting { .. });
            if recovering {
                *state = ConnectionState::Connected;
            }
            recovering
        });
    }
}

/// Failed periodic refreshes in a row before the state reads
/// `Reconnecting`.
const FAILURES_BEFORE_RECONNECTING: u32 = 2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_failed_refreshes_read_as_reconnecting_until_any_refresh_succeeds() {
        let controller = Controller::new(crate::ControllerConfig::default());
        let state = controller.connection_state();
        controller
            .inner
            .connection_state
            .send_replace(ConnectionState::Connected);
        let failed = Err(CoreError::ControllerDisconnected);

        let failures = controller.note_refresh(&failed, 0);
        assert_eq!(failures, 1);
        assert_eq!(
            *state.borrow(),
            ConnectionState::Connected,
            "one failed refresh is not a lost connection"
        );
        let failures = controller.note_refresh(&failed, failures);
        assert_eq!(
            *state.borrow(),
            ConnectionState::Reconnecting { attempt: 2 }
        );
        let failures = controller.note_refresh(&failed, failures);
        assert_eq!(
            *state.borrow(),
            ConnectionState::Reconnecting { attempt: 3 }
        );

        // Any refresh that succeeds recovers it, not only the periodic one.
        controller.note_refresh_succeeded();
        assert_eq!(*state.borrow(), ConnectionState::Connected);
        assert_eq!(controller.note_refresh(&Ok(()), failures), 0);
    }

    #[test]
    fn a_late_refresh_result_never_overrides_a_disconnect() {
        let controller = Controller::new(crate::ControllerConfig::default());
        let state = controller.connection_state();
        let failed = Err(CoreError::ControllerDisconnected);
        controller.note_refresh(&failed, 5);
        assert_eq!(*state.borrow(), ConnectionState::Disconnected);
        controller.note_refresh_succeeded();
        assert_eq!(*state.borrow(), ConnectionState::Disconnected);
    }
}
