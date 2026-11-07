use std::collections::HashSet;

use serde::Serialize;

use crate::Error;
use crate::convert::system_log::invalid_history;

#[derive(Default)]
pub(super) struct RawProgress {
    seen: HashSet<Identity>,
}

#[derive(PartialEq, Eq, Hash)]
enum Identity {
    Id(String),
    Contents(Vec<u8>),
}

impl RawProgress {
    pub(super) fn len(&self) -> usize {
        self.seen.len()
    }

    /// Track every raw record before visibility filtering, including ID-less
    /// records and firewall pages. A repeated page cannot advance the walk.
    pub(super) fn check<T: Serialize>(
        &mut self,
        entries: &[T],
        id: impl Fn(&T) -> Option<&str>,
    ) -> Result<Vec<bool>, Error> {
        let previous_count = self.seen.len();
        let mut new_records = Vec::with_capacity(entries.len());
        for entry in entries {
            let identity = match id(entry).filter(|value| !value.is_empty()) {
                Some(id) => Identity::Id(id.to_owned()),
                None => Identity::Contents(
                    serde_json::to_vec(entry)
                        .map_err(|_| invalid_history("event identity could not be encoded"))?,
                ),
            };
            new_records.push(self.seen.insert(identity));
        }
        if !entries.is_empty() && self.seen.len() == previous_count {
            return Err(invalid_history(
                "event history returned a page with no new records",
            ));
        }
        Ok(new_records)
    }
}
