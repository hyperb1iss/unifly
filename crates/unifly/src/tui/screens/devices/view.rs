//! What the Devices table shows and in which order: the type and state
//! filter, the sort key, and the words the screen uses for a device's type
//! and state.

use std::cmp::Ordering;

use unifly_api::{Device, DeviceState, DeviceType};

/// Which devices the table lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum DeviceFilter {
    #[default]
    All,
    Gateways,
    Switches,
    AccessPoints,
    Offline,
    Updates,
}

impl DeviceFilter {
    /// Every filter, in filter-bar order.
    pub(super) const ALL: [Self; 6] = [
        Self::All,
        Self::Gateways,
        Self::Switches,
        Self::AccessPoints,
        Self::Offline,
        Self::Updates,
    ];

    pub(super) const LABELS: [&'static str; 6] =
        ["All", "Gateways", "Switches", "APs", "Offline", "Updates"];

    pub(super) fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|filter| *filter == self)
            .unwrap_or_default()
    }

    /// The filter `step` places along the bar, wrapping.
    pub(super) fn stepped(self, step: isize) -> Self {
        Self::ALL[wrapped(self.index(), step, Self::ALL.len())]
    }

    pub(super) fn matches(self, device: &Device) -> bool {
        match self {
            Self::All => true,
            Self::Gateways => device.device_type == DeviceType::Gateway,
            Self::Switches => device.device_type == DeviceType::Switch,
            Self::AccessPoints => device.device_type == DeviceType::AccessPoint,
            Self::Offline => is_down(device.state),
            Self::Updates => device.firmware_updatable,
        }
    }

    /// What an empty table says under this filter.
    pub(super) fn empty_note(self) -> &'static str {
        match self {
            Self::All => "No devices on this site",
            Self::Gateways => "No gateways on this site",
            Self::Switches => "No switches on this site",
            Self::AccessPoints => "No access points on this site",
            Self::Offline => "Every device is online",
            Self::Updates => "Every device is up to date",
        }
    }
}

/// `index` moved `step` places around a ring of `len`.
pub(in crate::tui::screens) fn wrapped(index: usize, step: isize, len: usize) -> usize {
    let len = isize::try_from(len.max(1)).unwrap_or(1);
    let next = (isize::try_from(index).unwrap_or(0) + step).rem_euclid(len);
    usize::try_from(next).unwrap_or(0)
}

/// What the table is ordered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum SortKey {
    #[default]
    Name,
    Status,
    Clients,
    Traffic,
    Uptime,
}

impl SortKey {
    const ALL: [Self; 5] = [
        Self::Name,
        Self::Status,
        Self::Clients,
        Self::Traffic,
        Self::Uptime,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Status => "status",
            Self::Clients => "clients",
            Self::Traffic => "traffic",
            Self::Uptime => "uptime",
        }
    }

    /// Whether the key starts out largest first: names and trouble read
    /// top down, while counts and rates are looked at for their peaks.
    fn starts_descending(self) -> bool {
        matches!(self, Self::Clients | Self::Traffic | Self::Uptime)
    }
}

/// The table's sort: a key and a direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct DeviceSort {
    pub(super) key: SortKey,
    pub(super) descending: bool,
}

impl DeviceSort {
    /// The next key, in its usual direction.
    pub(super) fn next(self) -> Self {
        let at = SortKey::ALL
            .iter()
            .position(|key| *key == self.key)
            .unwrap_or_default();
        let key = SortKey::ALL[(at + 1) % SortKey::ALL.len()];
        Self {
            key,
            descending: key.starts_descending(),
        }
    }

    pub(super) fn reversed(self) -> Self {
        Self {
            descending: !self.descending,
            ..self
        }
    }

    /// The sort as the filter bar shows it, such as `name ↑`.
    pub(super) fn label(self) -> String {
        format!("{} {}", self.key.label(), arrow(self.descending))
    }

    /// Order `devices`, which arrive by name. Devices with no value for
    /// the key go last in either direction, and ties keep name order.
    pub(super) fn apply<T: AsRef<Device>>(self, devices: &mut [T]) {
        if self.key == SortKey::Name {
            if self.descending {
                devices.reverse();
            }
            return;
        }
        let key = |device: &T| -> Option<u64> {
            let device = device.as_ref();
            match self.key {
                SortKey::Name => None,
                SortKey::Status => Some(health_rank(device.state)),
                SortKey::Clients => device.client_count.map(u64::from),
                SortKey::Traffic => device
                    .stats
                    .uplink_bandwidth
                    .as_ref()
                    .map(|rate| rate.tx_bytes_per_sec.saturating_add(rate.rx_bytes_per_sec)),
                SortKey::Uptime => device.stats.uptime_secs,
            }
        };
        devices.sort_by(|a, b| known_first(key(a), key(b), self.descending));
    }
}

/// The arrow for a sort direction.
pub(in crate::tui::screens) fn arrow(descending: bool) -> &'static str {
    if descending { "\u{2193}" } else { "\u{2191}" }
}

/// Compare two optional sort values in the chosen direction, with missing
/// values last either way.
pub(in crate::tui::screens) fn known_first<T: Ord>(
    a: Option<T>,
    b: Option<T>,
    descending: bool,
) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) if descending => b.cmp(&a),
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Trouble first: devices that are down, then ones waiting or in
/// transition, then the healthy ones.
fn health_rank(state: DeviceState) -> u64 {
    if is_down(state) {
        0
    } else if state == DeviceState::Online {
        2
    } else {
        1
    }
}

/// Whether the controller has lost the device.
pub(super) fn is_down(state: DeviceState) -> bool {
    matches!(
        state,
        DeviceState::Offline | DeviceState::ConnectionInterrupted | DeviceState::Isolated
    )
}

/// A device state in words.
pub(super) fn state_label(state: DeviceState) -> &'static str {
    match state {
        DeviceState::Online => "Online",
        DeviceState::Offline => "Offline",
        DeviceState::PendingAdoption => "Pending adoption",
        DeviceState::Updating => "Updating",
        DeviceState::GettingReady => "Getting ready",
        DeviceState::Adopting => "Adopting",
        DeviceState::Deleting => "Removing",
        DeviceState::ConnectionInterrupted => "Connection interrupted",
        DeviceState::Isolated => "Isolated",
        _ => "Unknown",
    }
}

/// A device type in words.
pub(super) fn type_label(device_type: DeviceType) -> &'static str {
    match device_type {
        DeviceType::Gateway => "Gateway",
        DeviceType::Switch => "Switch",
        DeviceType::AccessPoint => "Access Point",
        _ => "Device",
    }
}

/// `1 device`, `3 devices`.
pub(in crate::tui::screens) fn counted(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}
