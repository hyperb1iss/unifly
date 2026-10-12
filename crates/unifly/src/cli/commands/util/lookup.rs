//! Resolve a user-supplied identifier to one record from a snapshot.
//!
//! Commands that target a single record accept whatever a person or an
//! agent reaches for first: the record ID, a MAC address in any common
//! spelling, an IP address, or the display name. The record ID is tried
//! first. After that the identifier's [`Shape`] alone decides how it is
//! read: an IP address matches only IP addresses, a MAC only MACs, and
//! anything else only display names (case-insensitive, exact). A MAC- or
//! IP-shaped identifier never falls through to names, so a device named
//! after some other MAC or IP can never capture a command aimed at that
//! address. One match resolves; several are an error listing every
//! candidate, never a guess. Partial matching stays with `clients find`
//! and list filters.

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

/// What an identifier is, judged by its spelling before any record is
/// consulted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    Ip(IpAddr),
    Mac(MacAddress),
    /// Neither: a display name (or an ID, which is tried first anyway).
    Name,
}

impl Shape {
    pub fn of(needle: &str) -> Self {
        let needle = needle.trim();
        // IP before MAC: MAC parsing drops every colon, so an IPv6 address
        // with twelve hex digits would otherwise pass for a MAC. No MAC
        // spelling parses as an IP address.
        if let Ok(ip) = needle.parse::<IpAddr>() {
            Self::Ip(ip)
        } else if let Ok(mac) = needle.parse::<MacAddress>() {
            Self::Mac(mac)
        } else {
            Self::Name
        }
    }
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

/// The MAC of the record `needle` identifies. A MAC-shaped `needle` that
/// matches no record passes through unchanged, so MAC-addressed commands
/// can reach something the snapshot lacks; any other miss is not found.
pub fn lookup_mac<T: Lookup>(records: &[Arc<T>], needle: &str) -> Result<MacAddress, CliError> {
    if let Some(record) = lookup(records, needle)? {
        return record.mac().cloned().ok_or_else(|| not_found::<T>(needle));
    }
    match Shape::of(needle) {
        Shape::Mac(mac) => Ok(mac),
        Shape::Ip(_) | Shape::Name => Err(not_found::<T>(needle)),
    }
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
    match Shape::of(needle) {
        Shape::Ip(ip) => tier(&|record| record.ip() == Some(ip)),
        Shape::Mac(mac) => tier(&|record| record.mac() == Some(&mac)),
        Shape::Name => {
            let name = needle.to_lowercase();
            tier(&|record| {
                record
                    .names()
                    .iter()
                    .any(|candidate| candidate.to_lowercase() == name)
            })
        }
    }
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
