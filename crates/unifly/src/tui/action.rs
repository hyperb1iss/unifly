//! All possible UI actions. Actions are the sole mechanism for state mutation.

use std::fmt;
use std::sync::Arc;

use serde_json::Value;
use unifly_api::model::{
    AclRule, EventCategory, FirewallPolicy, FirewallZone, NatPolicy, WifiBroadcast,
};
use unifly_api::session_models::{ChannelAvailability, RogueAp};
use unifly_api::{
    Client, Device, EntityId, Event, MacAddress, Network, Site, UpdateNetworkRequest,
};

use crate::tui::screen::ScreenId;

/// Direction for reorder operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

/// Client type filter for the clients screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientTypeFilter {
    All,
    Wireless,
    Wired,
    Vpn,
    Guest,
}

/// Device detail sub-tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeviceDetailTab {
    #[default]
    Overview,
    Performance,
    Radios,
    Clients,
    Ports,
}

/// Firewall view sub-tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FirewallSubTab {
    #[default]
    Policies,
    Zones,
    AclRules,
    NatPolicies,
}

/// WiFi dashboard sub-tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WifiSubTab {
    #[default]
    Overview,
    Clients,
    Neighbors,
    Roaming,
}

impl WifiSubTab {
    pub const ALL: [Self; 4] = [
        Self::Overview,
        Self::Clients,
        Self::Neighbors,
        Self::Roaming,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Clients => "Clients",
            Self::Neighbors => "Neighbors",
            Self::Roaming => "Roaming",
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|tab| *tab == self)
            .unwrap_or_default();
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|tab| *tab == self)
            .unwrap_or_default();
        Self::ALL[(index + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// Sort field for the WiFi dashboard tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WifiSortField {
    #[default]
    Health,
    Name,
    Clients,
    Channel,
    Signal,
    Roams,
    Security,
    Time,
}

/// WiFi band selector used by the dashboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WifiBand {
    #[default]
    TwoGhz,
    FiveGhz,
    SixGhz,
}

impl WifiBand {
    pub const ALL: [Self; 3] = [Self::TwoGhz, Self::FiveGhz, Self::SixGhz];

    pub fn label(self) -> &'static str {
        match self {
            Self::TwoGhz => "2.4 GHz",
            Self::FiveGhz => "5 GHz",
            Self::SixGhz => "6 GHz",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::TwoGhz => "2.4G",
            Self::FiveGhz => "5G",
            Self::SixGhz => "6G",
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|band| *band == self)
            .unwrap_or_default();
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|band| *band == self)
            .unwrap_or_default();
        Self::ALL[(index + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// Stats time period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatsPeriod {
    #[default]
    OneHour,
    TwentyFourHours,
    SevenDays,
    ThirtyDays,
}

impl StatsPeriod {
    /// Session API interval string for `stat/report` endpoints.
    pub fn api_interval(self) -> &'static str {
        match self {
            Self::OneHour | Self::TwentyFourHours => "5minutes",
            Self::SevenDays => "hourly",
            Self::ThirtyDays => "daily",
        }
    }

    /// Duration of this period in seconds, used to compute the `start` epoch
    /// for `stat/report` requests.
    pub fn duration_secs(self) -> i64 {
        match self {
            Self::OneHour => 3_600,
            Self::TwentyFourHours => 86_400,
            Self::SevenDays => 7 * 86_400,
            Self::ThirtyDays => 30 * 86_400,
        }
    }

    /// Duration of each report bucket in seconds.
    pub fn bucket_duration_secs(self) -> i64 {
        match self {
            Self::OneHour | Self::TwentyFourHours => 5 * 60,
            Self::SevenDays => 60 * 60,
            Self::ThirtyDays => 24 * 60 * 60,
        }
    }
}

/// Historical stats data fetched from the controller.
#[derive(Debug, Clone, Default)]
pub struct StatsData {
    /// WAN TX bandwidth: `(epoch_secs, bytes_per_sec)`
    pub bandwidth_tx: Vec<(f64, f64)>,
    /// WAN RX bandwidth: `(epoch_secs, bytes_per_sec)`
    pub bandwidth_rx: Vec<(f64, f64)>,
    /// Client count over time: `(epoch_secs, count)`
    pub client_counts: Vec<(f64, f64)>,
    /// Top DPI applications: `(name, total_bytes)`
    pub dpi_apps: Vec<(String, u64)>,
    /// Top DPI categories: `(name, total_bytes)`
    pub dpi_categories: Vec<(String, u64)>,
}

/// Sort field for table columns.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortField {
    Name,
    Status,
    Model,
    Ip,
    Cpu,
    Memory,
    Traffic,
    Uptime,
    Signal,
    Duration,
}

/// Notification severity level.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// A toast notification.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Notification {
    pub message: String,
    pub level: NotificationLevel,
}

#[allow(dead_code)]
impl Notification {
    pub fn success(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            level: NotificationLevel::Success,
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            level: NotificationLevel::Error,
        }
    }

    pub fn info(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            level: NotificationLevel::Info,
        }
    }
}

/// Pending confirmation action.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum ConfirmAction {
    RestartDevice { id: EntityId, name: String },
    UpgradeDevice { mac: MacAddress, name: String },
    UnadoptDevice { id: EntityId, name: String },
    AdoptDevice { mac: String },
    PowerCyclePort { device_id: EntityId, port_idx: u32 },
    BlockClient { id: EntityId, name: String },
    UnblockClient { id: EntityId, name: String },
    ForgetClient { id: EntityId, name: String },
    DeleteFirewallPolicy { id: EntityId, name: String },
}

impl fmt::Display for ConfirmAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RestartDevice { name, .. } => write!(f, "Restart {name}?"),
            Self::UpgradeDevice { name, .. } => write!(f, "Upgrade firmware on {name}?"),
            Self::UnadoptDevice { name, .. } => {
                write!(f, "Remove {name}? This cannot be undone.")
            }
            Self::AdoptDevice { mac } => write!(f, "Adopt device {mac}?"),
            Self::PowerCyclePort { port_idx, .. } => write!(f, "Power cycle port {port_idx}?"),
            Self::BlockClient { name, .. } => write!(f, "Block {name}?"),
            Self::UnblockClient { name, .. } => write!(f, "Unblock {name}?"),
            Self::ForgetClient { name, .. } => {
                write!(f, "Forget {name}? History will be lost.")
            }
            Self::DeleteFirewallPolicy { name, .. } => write!(f, "Delete policy {name}?"),
        }
    }
}

/// Every state transition in the TUI is expressed as an Action.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum Action {
    // ── Lifecycle ──────────────────────────────────────────────────
    Quit,
    Tick,
    Render,
    Invalidate,
    ChartPeak,
    Resize(u16, u16),

    // ── Navigation ────────────────────────────────────────────────
    SwitchScreen(ScreenId),
    GoBack,
    FocusNext,
    FocusPrev,

    // ── Data Events (from unifly-core streams) ─────────────────────
    DevicesUpdated(Arc<Vec<Arc<Device>>>),
    ClientsUpdated(Arc<Vec<Arc<Client>>>),
    NetworksUpdated(Arc<Vec<Arc<Network>>>),
    FirewallPoliciesUpdated(Arc<Vec<Arc<FirewallPolicy>>>),
    FirewallZonesUpdated(Arc<Vec<Arc<FirewallZone>>>),
    AclRulesUpdated(Arc<Vec<Arc<AclRule>>>),
    NatPoliciesUpdated(Arc<Vec<Arc<NatPolicy>>>),
    WifiBroadcastsUpdated(Arc<Vec<Arc<WifiBroadcast>>>),
    EventReceived(Arc<Event>),
    HealthUpdated(Arc<Vec<unifly_api::HealthSummary>>),
    SiteUpdated(Arc<Site>),
    WifiNeighborsUpdated(Arc<Vec<RogueAp>>),
    WifiChannelsUpdated(Arc<Vec<ChannelAvailability>>),
    WifiClientDetailLoaded {
        ip: String,
        data: Arc<Value>,
    },
    WifiRoamHistoryLoaded {
        mac: String,
        events: Arc<Vec<Value>>,
    },

    // ── Connection Status ─────────────────────────────────────────
    Connected,
    Disconnected(String),
    Reconnecting {
        attempt: u32,
    },
    RetryConnect,

    // ── Device Selection ──────────────────────────────────────────
    SelectDevice(usize),
    OpenDeviceDetail(EntityId),
    CloseDetail,
    DeviceDetailTab(DeviceDetailTab),

    // ── Client Selection ──────────────────────────────────────────
    SelectClient(usize),
    OpenClientDetail(EntityId),
    FilterClientType(ClientTypeFilter),
    WifiSubTab(WifiSubTab),
    WifiFocusAp(Option<EntityId>),
    WifiToggleChannelMap,
    WifiSortColumn(WifiSortField),
    WifiBandSelect(WifiBand),

    // ── Firewall ──────────────────────────────────────────────────
    SelectZonePair(EntityId, EntityId),
    ReorderPolicy(usize, Direction),
    FirewallSubTab(FirewallSubTab),

    // ── Network Editing ──────────────────────────────────────────
    NetworkEdit(EntityId),
    NetworkSave(EntityId, Box<UpdateNetworkRequest>),
    NetworkEditResult(Result<(), String>),

    // ── Device Commands ───────────────────────────────────────────
    RequestRestart(EntityId),
    RequestLocate(EntityId),
    RequestUpgrade(EntityId),
    RequestAdopt(String),
    RequestUnadopt(EntityId),
    RequestPortPowerCycle(EntityId, u32),

    // ── Client Commands ───────────────────────────────────────────
    RequestBlockClient(EntityId),
    RequestUnblockClient(EntityId),
    RequestKickClient(EntityId),
    RequestForgetClient(EntityId),
    RequestWifiNeighbors(Option<i64>),
    RequestWifiChannels,
    RequestWifiClientDetail(String),
    RequestWifiRoamHistory {
        mac: String,
        limit: Option<u32>,
    },

    // ── Confirm Dialog ────────────────────────────────────────────
    ShowConfirm(ConfirmAction),
    ConfirmYes,
    ConfirmNo,

    // ── Search ────────────────────────────────────────────────────
    OpenSearch,
    CloseSearch,
    SearchInput(String),
    SearchSubmit,

    // ── Help / About ───────────────────────────────────────────────
    ToggleHelp,
    ToggleAbout,

    // ── Notifications ─────────────────────────────────────────────
    Notify(Notification),
    DismissNotification,

    // ── Stats ─────────────────────────────────────────────────────
    SetStatsPeriod(StatsPeriod),
    RequestStats(StatsPeriod),
    StatsUpdated(StatsData),

    // ── Topology ──────────────────────────────────────────────────
    TopologyPan(i16, i16),
    TopologyZoom(f64),
    TopologyFit,
    TopologyReset,

    // ── Events Screen ─────────────────────────────────────────────
    ToggleEventPause,
    FilterEventType(Option<String>),
    FilterEventCategory(Option<EventCategory>),

    // ── Table Operations ──────────────────────────────────────────
    SortColumn(SortField),
    ScrollUp,
    ScrollDown,
    ScrollToTop,
    ScrollToBottom,
    PageUp,
    PageDown,

    // ── Onboarding ─────────────────────────────────────────────────
    OnboardingComplete {
        profile_name: String,
        config: Box<unifly_api::ControllerConfig>,
    },
    OnboardingTestResult(Result<(), String>),

    // ── Settings ────────────────────────────────────────────────────
    OpenSettings,
    CloseSettings,
    SettingsTestResult(Result<(), String>),
    SettingsApply {
        profile_name: String,
        config: Box<unifly_api::ControllerConfig>,
    },

    // ── Donate ───────────────────────────────────────────────────
    OpenDonate,
    SetShowDonate(bool),
}
