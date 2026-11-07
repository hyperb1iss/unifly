use crate::model::Alarm;
use crate::session::system_log::SystemLogEntry;
use crate::{Error, SessionClient};

use super::event_history::endpoint_missing;

/// An unsupported collection selects the legacy API. An empty collection is
/// still a supported modern endpoint, and a failed mutation is never replayed.
async fn critical_alerts(session: &SessionClient) -> Result<Option<Vec<SystemLogEntry>>, Error> {
    match session.list_critical_alerts().await {
        Ok(alerts) => Ok(Some(alerts)),
        Err(error) if endpoint_missing(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) async fn list(session: &SessionClient) -> Result<Vec<Alarm>, Error> {
    match critical_alerts(session).await? {
        Some(alerts) => alerts.into_iter().map(Alarm::try_from).collect(),
        None => Ok(session
            .list_alarms()
            .await?
            .into_iter()
            .map(Alarm::from)
            .collect()),
    }
}

pub(super) async fn archive(session: &SessionClient, id: &str) -> Result<(), Error> {
    if critical_alerts(session).await?.is_some() {
        session.acknowledge_critical_alert(id).await
    } else {
        session.archive_alarm(id).await
    }
}

pub(super) async fn archive_all(session: &SessionClient) -> Result<(), Error> {
    if critical_alerts(session).await?.is_some() {
        session.acknowledge_all_critical_alerts().await
    } else {
        session.archive_all_alarms().await
    }
}

#[cfg(test)]
mod tests;
