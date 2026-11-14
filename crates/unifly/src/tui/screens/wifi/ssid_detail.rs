//! Each SSID's full record. The controller's list is a summary: it says
//! nothing of visibility, isolation, steering, or roaming. The SSIDs tab
//! reads every SSID it shows in full, one request a tick, and keeps each
//! record until it ages, a write changes it, or the list disagrees.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use unifly_api::EntityId;
use unifly_api::model::WifiBroadcast;

use super::fetch::{recent, record};
use super::{Failure, WifiScreen};
use crate::tui::action::Action;

/// How old a record gets before it is read again while the tab shows it.
pub(super) const SSID_DETAIL_REFRESH_INTERVAL: Duration = Duration::from_mins(1);

struct Detail {
    record: Arc<WifiBroadcast>,
    /// When it was read, or `None` once something says it is out of date.
    fresh_since: Option<Instant>,
}

/// The records on hand, the reads in flight, and the reads that failed.
#[derive(Default)]
pub(super) struct SsidDetails {
    known: HashMap<EntityId, Detail>,
    pending: HashSet<EntityId>,
    /// Reads asked for before a write to their SSID ended. What they
    /// bring back may be the record from before the write, so it is
    /// dropped.
    outdated: HashSet<EntityId>,
    failures: HashMap<String, Failure>,
}

/// Reads one such setting off a broadcast.
pub(super) type Pick = fn(&WifiBroadcast) -> Option<bool>;

/// What is known of one setting the list leaves out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Setting {
    On,
    Off,
    /// The record was read and does not carry the setting.
    Absent,
    /// The record has not arrived yet.
    Reading,
    /// The record could not be read.
    Unknown,
}

impl SsidDetails {
    fn wanted(&self, id: &EntityId, now: Instant) -> bool {
        !self.pending.contains(id)
            && !recent(&self.failures, &id.to_string(), now)
            && self.known.get(id).is_none_or(|detail| {
                detail
                    .fresh_since
                    .is_none_or(|read| now.duration_since(read) >= SSID_DETAIL_REFRESH_INTERVAL)
            })
    }

    /// Mark the record of `id` out of date, so the next tick reads it.
    fn expire(&mut self, id: &EntityId) {
        if let Some(detail) = self.known.get_mut(id) {
            detail.fresh_since = None;
        }
    }

    /// Keep step with a new list: forget SSIDs that left, and expire a
    /// record the summary disagrees with, since something changed it.
    pub(super) fn follow(&mut self, listed: &[Arc<WifiBroadcast>]) {
        let ids: HashSet<&EntityId> = listed.iter().map(|ssid| &ssid.id).collect();
        self.known.retain(|id, _| ids.contains(id));
        self.pending.retain(|id| ids.contains(id));
        self.outdated.retain(|id| ids.contains(id));
        for ssid in listed {
            if let Some(detail) = self.known.get_mut(&ssid.id) {
                let record = &detail.record;
                if record.name != ssid.name
                    || record.enabled != ssid.enabled
                    || record.security != ssid.security
                {
                    detail.fresh_since = None;
                }
            }
        }
    }
}

impl WifiScreen {
    /// The next record the SSIDs tab is missing: the selected SSID's
    /// first, then the rest of the table in order.
    pub(super) fn ssid_detail_request(&mut self, now: Instant) -> Option<Action> {
        let id = self
            .selected_ssid()
            .into_iter()
            .chain(self.visible_ssids())
            .map(|ssid| &ssid.id)
            .find(|id| self.ssid_details.wanted(id, now))?
            .clone();
        self.ssid_details.pending.insert(id.clone());
        Some(Action::RequestWifiBroadcastDetail(id))
    }

    pub(super) fn ssid_detail_loaded(&mut self, id: &EntityId, record: &Arc<WifiBroadcast>) {
        self.ssid_details.pending.remove(id);
        self.ssid_details.failures.remove(&id.to_string());
        if self.ssid_details.outdated.remove(id) {
            return;
        }
        self.keep_ssid_record(id, record);
    }

    /// Keep `record` as the fresh record of `id`, unless the list no
    /// longer has that SSID.
    fn keep_ssid_record(&mut self, id: &EntityId, record: &Arc<WifiBroadcast>) {
        if self.ssids.iter().any(|ssid| &ssid.id == id) {
            self.ssid_details.known.insert(
                id.clone(),
                Detail {
                    record: Arc::clone(record),
                    fresh_since: Some(Instant::now()),
                },
            );
        }
    }

    pub(super) fn ssid_detail_failed(&mut self, id: &EntityId, error: &str) {
        self.ssid_details.pending.remove(id);
        self.ssid_details.outdated.remove(id);
        record(&mut self.ssid_details.failures, &id.to_string(), error);
    }

    /// A write to `id` ended. The record read back after it is the one to
    /// show; without it the record on hand is out of date. A read still
    /// in flight was asked for before the write ended and may carry the
    /// record from before it.
    pub(super) fn ssid_write_settled(&mut self, id: &EntityId, read: Option<&Arc<WifiBroadcast>>) {
        if self.ssid_details.pending.contains(id) {
            self.ssid_details.outdated.insert(id.clone());
        }
        match read {
            Some(record) => self.keep_ssid_record(id, record),
            None => self.ssid_details.expire(id),
        }
    }

    /// Why the last read of `ssid`'s record failed.
    pub(super) fn ssid_detail_error(&self, ssid: &WifiBroadcast) -> Option<&str> {
        self.ssid_details
            .failures
            .get(&ssid.id.to_string())
            .map(|failure| failure.error.as_str())
    }

    /// One of the settings only the full record carries, from the record
    /// when it has been read and from the list entry when that has it.
    pub(super) fn ssid_setting(&self, ssid: &WifiBroadcast, pick: Pick) -> Setting {
        let detail = self.ssid_details.known.get(&ssid.id);
        let value = detail
            .and_then(|detail| pick(&detail.record))
            .or_else(|| pick(ssid));
        match value {
            Some(true) => Setting::On,
            Some(false) => Setting::Off,
            None if detail.is_some() => Setting::Absent,
            None if self.ssid_detail_error(ssid).is_some() => Setting::Unknown,
            None => Setting::Reading,
        }
    }

    /// How many networks have a key of their own on `ssid`, from whichever
    /// of the list entry and the full record says more.
    pub(super) fn ssid_preshared_keys(&self, ssid: &WifiBroadcast) -> usize {
        self.ssid_details
            .known
            .get(&ssid.id)
            .map_or(0, |detail| detail.record.preshared_key_count)
            .max(ssid.preshared_key_count)
    }
}
