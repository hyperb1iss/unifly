// Session API event and alarm endpoints
//
// Events (stat/event) and alarms (stat/alarm) with archive support
// via cmd/evtmgr.

use serde_json::json;
use tracing::debug;

use crate::error::Error;
use crate::session::client::SessionClient;
use crate::session::models::{SessionAlarm, SessionEvent};

impl SessionClient {
    /// Read one page of legacy events, newest first, within the specified number of hours.
    ///
    /// `POST /api/s/{site}/stat/event`; `start` is the raw record offset.
    pub async fn list_events_page(
        &self,
        within_hours: u32,
        start: u32,
        limit: u32,
    ) -> Result<Vec<SessionEvent>, Error> {
        self.post_read(
            self.site_url("stat/event"),
            &json!({
                "_sort": "-time",
                "within": within_hours,
                "type": null,
                "_start": start,
                "_limit": limit,
            }),
        )
        .await
    }

    /// List recent events.
    ///
    /// `GET /api/s/{site}/stat/event`
    ///
    /// If `count` is provided, limits the number of events returned
    /// by appending `?_limit={count}` to the request.
    pub async fn list_events(&self, count: Option<u32>) -> Result<Vec<SessionEvent>, Error> {
        let path = match count {
            Some(n) => format!("stat/event?_limit={n}"),
            None => "stat/event".to_string(),
        };
        let url = self.site_url(&path);
        debug!(?count, "listing events");
        self.get(url).await
    }

    /// List active alarms.
    ///
    /// `GET /api/s/{site}/stat/alarm`
    pub async fn list_alarms(&self) -> Result<Vec<SessionAlarm>, Error> {
        let url = self.site_url("stat/alarm");
        debug!("listing alarms");
        self.get(url).await
    }

    /// Archive (acknowledge) a specific alarm by its ID.
    ///
    /// `POST /api/s/{site}/cmd/evtmgr` with `{"cmd": "archive-alarm", "_id": "..."}`
    pub async fn archive_alarm(&self, id: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/evtmgr");
        debug!(id, "archiving alarm");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "archive-alarm",
                    "_id": id,
                }),
            )
            .await?;
        Ok(())
    }

    /// Archive all alarms for the active site.
    ///
    /// `POST /api/s/{site}/cmd/evtmgr` with `{"cmd": "archive-all-alarms"}`
    pub async fn archive_all_alarms(&self) -> Result<(), Error> {
        let url = self.site_url("cmd/evtmgr");
        debug!("archiving all alarms");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "archive-all-alarms",
                }),
            )
            .await?;
        Ok(())
    }
}
