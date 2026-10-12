//! Resolve a user-supplied identifier to one record from a snapshot.
//!
//! Commands that target a single record accept whatever a person or an
//! agent reaches for first: the record ID, a MAC address in any common
//! spelling, an IP address, or the display name. Matching runs in tiers,
//! strongest first (ID, MAC, IP, name), and the first tier with any match
//! decides. One match resolves; several are an error listing every
//! candidate, never a guess. Names compare case-insensitively and exactly;
//! partial matching stays with `clients find` and list filters.

mod records;
#[cfg(test)]
mod tests;

use std::net::IpAddr;
use std::sync::Arc;

use unifly_api::MacAddress;

use crate::cli::error::CliError;

/// A record that commands can target by identifier.
pub trait Lookup {
    /// Singular noun for messages, such as `device` or `firewall policy`.
    const KIND: &'static str;
    /// The command that lists candidates, such as `devices list`.
    const LIST: &'static str;

    /// Whether `needle` is this record's ID, ignoring ASCII case.
    fn has_id(&self, needle: &str) -> bool;

    /// Hardware address, for records that have one.
    fn mac(&self) -> Option<&MacAddress> {
        None
    }

    /// Network address, for records that have one.
    fn ip(&self) -> Option<IpAddr> {
        None
    }

    /// Display names the record answers to (name, hostname, ...).
    fn names(&self) -> Vec<&str>;

    /// One line that tells candidates apart in an ambiguity report.
    fn describe(&self) -> String;
}

/// Find the record `needle` identifies. `Ok(None)` means nothing matched;
/// more than one match in the deciding tier is an [`CliError::Ambiguous`].
pub fn lookup<'a, T: Lookup>(
    records: &'a [Arc<T>],
    needle: &str,
) -> Result<Option<&'a Arc<T>>, CliError> {
    let mut found = matching(records, needle.trim());
    if found.len() > 1 {
        return Err(ambiguous(needle, &found));
    }
    Ok(found.pop())
}

/// [`lookup`], with a not-found error that points at the list command.
pub fn resolve<'a, T: Lookup>(records: &'a [Arc<T>], needle: &str) -> Result<&'a Arc<T>, CliError> {
    lookup(records, needle)?.ok_or_else(|| not_found::<T>(needle))
}

/// The error for an identifier that matches no `T`.
pub fn not_found<T: Lookup>(needle: &str) -> CliError {
    CliError::NotFound {
        resource_type: T::KIND.into(),
        identifier: needle.into(),
        list_command: T::LIST.into(),
    }
}

fn matching<'a, T: Lookup>(records: &'a [Arc<T>], needle: &str) -> Vec<&'a Arc<T>> {
    let tier = |hit: &dyn Fn(&T) -> bool| -> Vec<&'a Arc<T>> {
        records.iter().filter(|record| hit(record)).collect()
    };
    if needle.is_empty() {
        return Vec::new();
    }

    let identified = tier(&|record| record.has_id(needle));
    if !identified.is_empty() {
        return identified;
    }
    if let Ok(mac) = needle.parse::<MacAddress>() {
        let by_hardware = tier(&|record| record.mac() == Some(&mac));
        if !by_hardware.is_empty() {
            return by_hardware;
        }
    }
    if let Ok(ip) = needle.parse::<IpAddr>() {
        let by_address = tier(&|record| record.ip() == Some(ip));
        if !by_address.is_empty() {
            return by_address;
        }
    }
    let name = needle.to_lowercase();
    tier(&|record| {
        record
            .names()
            .iter()
            .any(|candidate| candidate.to_lowercase() == name)
    })
}

fn ambiguous<T: Lookup>(needle: &str, found: &[&Arc<T>]) -> CliError {
    let mut candidates: Vec<String> = found
        .iter()
        .map(|record| format!("  {}", record.describe()))
        .collect();
    candidates.sort();
    CliError::Ambiguous {
        resource_type: T::KIND.into(),
        identifier: needle.into(),
        count: found.len(),
        candidates: candidates.join("\n"),
        list_command: T::LIST.into(),
    }
}
