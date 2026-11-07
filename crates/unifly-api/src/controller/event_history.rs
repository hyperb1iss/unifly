use std::cmp::Reverse;
use std::collections::HashSet;

use chrono::Utc;

use crate::convert::system_log::{invalid_history, legacy_history_event};
use crate::model::{EntityId, Event};
use crate::session::system_log::{SystemLogPage, SystemLogQuery};
use crate::{CoreError, Error, SessionClient};

use super::Controller;
use super::support::require_session;

const PAGE_SIZE: u32 = 1_000;

mod progress;

use progress::RawProgress;

/// Bounds and visibility for a newest-first event history query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventHistoryQuery {
    pub within_hours: u32,
    pub limit: u32,
    /// Include firewall-block events in addition to the usual operational events.
    pub include_firewall: bool,
}

impl Default for EventHistoryQuery {
    fn default() -> Self {
        Self {
            within_hours: 24,
            limit: 100,
            include_firewall: false,
        }
    }
}

impl Controller {
    /// Query controller history, including pages beyond the refresh snapshot.
    pub async fn list_events(&self, query: EventHistoryQuery) -> Result<Vec<Event>, CoreError> {
        if query.limit == 0 {
            return Ok(Vec::new());
        }
        #[cfg(feature = "demo")]
        if self.demo_runtime().is_some() {
            let mut history = History::new(query);
            let events = self.events_snapshot();
            for event in events.iter() {
                let firewall = event.event_type == "EVT_GW_FirewallBlock";
                history.push(event.as_ref().clone(), firewall);
            }
            return Ok(history.finish());
        }
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        Ok(fetch_events(session, query).await?)
    }
}

pub(crate) async fn fetch_events(
    session: &SessionClient,
    query: EventHistoryQuery,
) -> Result<Vec<Event>, Error> {
    if query.limit == 0 {
        return Ok(Vec::new());
    }
    let mut history = History::new(query);
    let mut page_query = SystemLogQuery {
        timestamp_from: history.start,
        timestamp_to: history.end,
        page_number: 0,
        page_size: PAGE_SIZE,
    };
    let mut progress = RawProgress::default();
    let mut crossed_short_page = false;
    loop {
        let page = match session.list_system_log(&page_query).await {
            Ok(page) => page,
            Err(error) if page_query.page_number == 0 && endpoint_missing(&error) => {
                return legacy_events(session, history).await;
            }
            Err(error) => return Err(error),
        };
        let new_records = progress.check(&page.data, |entry| entry.id.as_deref())?;
        let done = validate_page(&page, &page_query, progress.len())?;
        let short_page =
            u32::try_from(page.data.len()).is_ok_and(|count| count < page_query.page_size);
        for (entry, is_new) in page.data.into_iter().zip(new_records) {
            if !is_new {
                continue;
            }
            let firewall = entry.subcategory.as_deref() == Some("SECURITY_FIREWALL");
            history.push(Event::try_from(entry)?, firewall);
        }
        if (history.full() && !crossed_short_page) || done {
            return Ok(history.finish());
        }
        // The page-number API has no row-offset contract. A short page can
        // be legitimate, but crossing it needs an end-to-end count check.
        crossed_short_page |= short_page;
        page_query.page_number = page_query
            .page_number
            .checked_add(1)
            .ok_or_else(|| invalid_history("system-log page number overflow"))?;
    }
}

async fn legacy_events(session: &SessionClient, mut history: History) -> Result<Vec<Event>, Error> {
    let mut start = 0_u32;
    let mut progress = RawProgress::default();
    loop {
        let page = session
            .list_events_page(history.query.within_hours, start, PAGE_SIZE)
            .await?;
        let raw_count = u32::try_from(page.len())
            .map_err(|_| invalid_history("legacy event page is too large"))?;
        if raw_count > PAGE_SIZE {
            return Err(invalid_history(
                "legacy event page exceeds the requested size",
            ));
        }
        let new_records = progress.check(&page, |entry| Some(entry.id.as_str()))?;
        for (entry, is_new) in page.into_iter().zip(new_records) {
            if !is_new {
                continue;
            }
            let firewall = entry.key.as_deref() == Some("EVT_GW_FirewallBlock");
            history.push(legacy_history_event(entry)?, firewall);
        }
        if history.full() || raw_count < PAGE_SIZE {
            return Ok(history.finish());
        }
        start = start
            .checked_add(raw_count)
            .ok_or_else(|| invalid_history("legacy event offset overflow"))?;
    }
}

fn validate_page(
    page: &SystemLogPage,
    query: &SystemLogQuery,
    distinct_records: usize,
) -> Result<bool, Error> {
    if page.page_number != query.page_number {
        return Err(invalid_history(
            "system-log returned a non-advancing page number",
        ));
    }
    let count = u64::try_from(page.data.len())
        .map_err(|_| invalid_history("system-log page is too large"))?;
    if count > u64::from(query.page_size) {
        return Err(invalid_history(
            "system-log page exceeds the requested size",
        ));
    }
    let distinct_records = u64::try_from(distinct_records)
        .map_err(|_| invalid_history("system-log record count overflow"))?;
    if page.total_page_count == 0 {
        if page.page_number == 0
            && page.total_element_count == 0
            && count == 0
            && !page.has_next
            && !page.or_more
        {
            return Ok(true);
        }
        return Err(invalid_history(
            "system-log reports no pages for nonempty history",
        ));
    }
    if page.page_number >= page.total_page_count || count > page.total_element_count {
        return Err(invalid_history(
            "system-log returned inconsistent page totals",
        ));
    }
    let done = page.page_number == page.total_page_count - 1;
    if done && (page.has_next || page.or_more) {
        return Err(invalid_history(
            "system-log reports additional history beyond its declared final page",
        ));
    }
    if (done && distinct_records < page.total_element_count) || (!done && count == 0) {
        return Err(invalid_history(
            "system-log history is incomplete for its page totals",
        ));
    }
    Ok(done)
}

/// Only an absent endpoint permits a protocol fallback, never an error body
/// mentioning NotFound inside an unrelated HTTP failure.
pub(crate) fn endpoint_missing(error: &Error) -> bool {
    match error {
        Error::Transport(error) => error.status() == Some(reqwest::StatusCode::NOT_FOUND),
        Error::Integration { status: 404, .. } => true,
        Error::SessionApi { message } => {
            message == "api.err.NotFound" || message.starts_with("HTTP 404 ")
        }
        _ => false,
    }
}

struct History {
    query: EventHistoryQuery,
    start: i64,
    end: i64,
    events: Vec<Event>,
    ids: HashSet<EntityId>,
}

impl History {
    fn new(query: EventHistoryQuery) -> Self {
        let end = Utc::now().timestamp_millis();
        Self {
            query,
            start: (end - i64::from(query.within_hours) * 3_600_000).max(0),
            end,
            events: Vec::new(),
            ids: HashSet::new(),
        }
    }

    fn push(&mut self, event: Event, firewall: bool) {
        let timestamp = event.timestamp.timestamp_millis();
        if timestamp < self.start
            || timestamp > self.end
            || (firewall && !self.query.include_firewall)
        {
            return;
        }
        if let Some(id) = &event.id
            && !self.ids.insert(id.clone())
        {
            return;
        }
        self.events.push(event);
    }

    fn full(&self) -> bool {
        self.events.len() >= usize::try_from(self.query.limit).unwrap_or(usize::MAX)
    }

    fn finish(mut self) -> Vec<Event> {
        self.events.sort_by_cached_key(|event| {
            (
                Reverse(event.timestamp),
                (
                    event.id.is_none(),
                    event.id.as_ref().map(ToString::to_string),
                ),
                event.event_type.clone(),
                event.message.clone(),
                event.raw_key.clone(),
                event.device_mac.as_ref().map(|mac| mac.as_str().to_owned()),
                event.client_mac.as_ref().map(|mac| mac.as_str().to_owned()),
                event.site_id.as_ref().map(ToString::to_string),
            )
        });
        self.events
            .truncate(usize::try_from(self.query.limit).unwrap_or(usize::MAX));
        self.events
    }
}

#[cfg(test)]
mod tests;
