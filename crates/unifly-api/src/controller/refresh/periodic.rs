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
    /// return the new count of failures in a row.
    fn note_refresh(&self, result: &Result<(), CoreError>, failures: u32) -> u32 {
        match result {
            Ok(()) => {
                if failures > 0 {
                    self.inner
                        .connection_state
                        .send_replace(ConnectionState::Connected);
                }
                0
            }
            Err(error) => {
                let failures = failures.saturating_add(1);
                warn!(error = %error, failures, "periodic refresh failed");
                self.inner
                    .connection_state
                    .send_replace(ConnectionState::Reconnecting { attempt: failures });
                failures
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_refresh_shows_in_the_connection_state_until_one_succeeds() {
        let controller = Controller::new(crate::ControllerConfig::default());
        let state = controller.connection_state();
        let failed = Err(CoreError::ControllerDisconnected);

        let failures = controller.note_refresh(&failed, 0);
        assert_eq!(failures, 1);
        assert_eq!(
            *state.borrow(),
            ConnectionState::Reconnecting { attempt: 1 }
        );
        let failures = controller.note_refresh(&failed, failures);
        assert_eq!(
            *state.borrow(),
            ConnectionState::Reconnecting { attempt: 2 }
        );

        assert_eq!(controller.note_refresh(&Ok(()), failures), 0);
        assert_eq!(*state.borrow(), ConnectionState::Connected);
    }
}
