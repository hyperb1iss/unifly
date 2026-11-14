//! How the Clients table is ordered, and the words the screen uses for a
//! client's type and for an empty filter.

use unifly_api::{Client, ClientType};

use crate::tui::action::ClientTypeFilter;
use crate::tui::screens::devices::view::{arrow, known_first};

/// What the table is ordered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum SortKey {
    #[default]
    Name,
    Signal,
    Traffic,
    Duration,
}

impl SortKey {
    const ALL: [Self; 4] = [Self::Name, Self::Signal, Self::Traffic, Self::Duration];

    fn label(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Signal => "signal",
            Self::Traffic => "traffic",
            Self::Duration => "duration",
        }
    }
}

/// The table's sort: a key and a direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct ClientSort {
    pub(super) key: SortKey,
    pub(super) descending: bool,
}

impl ClientSort {
    /// The next key, in its usual direction: names read top down, the
    /// measures start at their largest.
    pub(super) fn next(self) -> Self {
        let at = SortKey::ALL
            .iter()
            .position(|key| *key == self.key)
            .unwrap_or_default();
        let key = SortKey::ALL[(at + 1) % SortKey::ALL.len()];
        Self {
            key,
            descending: key != SortKey::Name,
        }
    }

    pub(super) fn reversed(self) -> Self {
        Self {
            descending: !self.descending,
            ..self
        }
    }

    /// The sort as the filter bar shows it, such as `signal ↓`.
    pub(super) fn label(self) -> String {
        format!("{} {}", self.key.label(), arrow(self.descending))
    }

    /// Order `clients`, which arrive by name. Clients with no value for
    /// the key go last in either direction, and ties keep name order.
    pub(super) fn apply<T: AsRef<Client>>(self, clients: &mut [T]) {
        if self.key == SortKey::Name {
            if self.descending {
                clients.reverse();
            }
            return;
        }
        let key = |client: &T| -> Option<i64> {
            let client = client.as_ref();
            match self.key {
                SortKey::Name => None,
                SortKey::Signal => client
                    .wireless
                    .as_ref()
                    .and_then(|wireless| wireless.signal_dbm)
                    .map(i64::from),
                SortKey::Traffic => match (client.tx_bytes, client.rx_bytes) {
                    (None, None) => None,
                    (tx, rx) => i64::try_from(tx.unwrap_or(0).saturating_add(rx.unwrap_or(0))).ok(),
                },
                // An earlier connection time is a longer duration.
                SortKey::Duration => client.connected_at.map(|at| -at.timestamp()),
            }
        };
        clients.sort_by(|a, b| known_first(key(a), key(b), self.descending));
    }
}

/// The filter-bar labels, in [`ClientTypeFilter::ALL`] order.
pub(super) const FILTER_LABELS: [&str; 5] = ["All", "Wireless", "Wired", "VPN", "Guest"];

pub(super) fn filter_matches(filter: ClientTypeFilter, client: &Client) -> bool {
    match filter {
        ClientTypeFilter::All => true,
        ClientTypeFilter::Wireless => client.client_type == ClientType::Wireless,
        ClientTypeFilter::Wired => client.client_type == ClientType::Wired,
        ClientTypeFilter::Vpn => client.client_type == ClientType::Vpn,
        ClientTypeFilter::Guest => client.is_guest,
    }
}

/// What an empty table says under `filter`.
pub(super) fn empty_note(filter: ClientTypeFilter) -> &'static str {
    match filter {
        ClientTypeFilter::All => "No clients connected",
        ClientTypeFilter::Wireless => "No wireless clients connected",
        ClientTypeFilter::Wired => "No wired clients connected",
        ClientTypeFilter::Vpn => "No VPN clients connected",
        ClientTypeFilter::Guest => "No guests connected",
    }
}

/// A client type in words.
pub(super) fn type_label(client_type: ClientType) -> &'static str {
    match client_type {
        ClientType::Wired => "Wired",
        ClientType::Wireless => "Wireless",
        ClientType::Vpn => "VPN",
        ClientType::Teleport => "Teleport",
        _ => "Unknown",
    }
}

/// A client's name as the screen shows it: its alias, else its hostname.
pub(super) fn client_name(client: &Client) -> &str {
    client
        .name
        .as_deref()
        .or(client.hostname.as_deref())
        .unwrap_or("unknown")
}
