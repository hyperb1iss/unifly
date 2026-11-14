//! All possible UI actions. Actions are the sole mechanism for state mutation.

use std::sync::Arc;

use crossterm::event::KeyEvent;
use serde_json::Value;
use unifly_api::model::{
    AclRule, Alarm, FirewallPolicy, FirewallZone, NatPolicy, PoeMode, PortProfile, WifiBroadcast,
};
use unifly_api::session_models::{ChannelAvailability, RogueAp};
use unifly_api::{
    Client, CreateNetworkRequest, Device, EntityId, Event, Network, Site, UpdateNetworkRequest,
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

impl ClientTypeFilter {
    /// All filters in filter-bar order.
    pub const ALL: [Self; 5] = [
        Self::All,
        Self::Wireless,
        Self::Wired,
        Self::Vpn,
        Self::Guest,
    ];
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

impl DeviceDetailTab {
    /// All tabs in tab-bar order.
    pub const ALL: [Self; 5] = [
        Self::Overview,
        Self::Performance,
        Self::Radios,
        Self::Clients,
        Self::Ports,
    ];
}

/// One change to a switch port's configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortChange {
    /// Give the port this label.
    Rename(String),
    /// Set how the port supplies PoE.
    Poe(PoeMode),
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

impl FirewallSubTab {
    /// All tabs in tab-bar order.
    pub const ALL: [Self; 4] = [
        Self::Policies,
        Self::Zones,
        Self::AclRules,
        Self::NatPolicies,
    ];
}

/// WiFi dashboard sub-tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WifiSubTab {
    #[default]
    Overview,
    Clients,
    Ssids,
    Neighbors,
    Roaming,
}

impl WifiSubTab {
    pub const ALL: [Self; 5] = [
        Self::Overview,
        Self::Clients,
        Self::Ssids,
        Self::Neighbors,
        Self::Roaming,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Clients => "Clients",
            Self::Ssids => "SSIDs",
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
    /// All periods in period-bar order.
    pub const ALL: [Self; 4] = [
        Self::OneHour,
        Self::TwentyFourHours,
        Self::SevenDays,
        Self::ThirtyDays,
    ];

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
    /// The period the series cover.
    pub period: StatsPeriod,
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
    /// Why application or category traffic could not be read.
    pub dpi_error: Option<String>,
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

    pub fn warning(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            level: NotificationLevel::Warning,
        }
    }
}

/// What the connected controller can reach. Screens use it to say why
/// something is empty instead of drawing a bare table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Access {
    /// The Integration API (networks, WiFi, firewall, ACL, DNS).
    pub integration: bool,
    /// Session HTTP (stats, events, WiFi observability, NAT, ports).
    pub session: bool,
    /// The live event WebSocket.
    pub live_events: bool,
}

/// The toasts for one command: what shows while it runs, and what
/// replaces that once the controller has accepted it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub pending: String,
    pub done: String,
}

impl Notice {
    pub fn new(pending: impl Into<String>, done: impl Into<String>) -> Self {
        Self {
            pending: pending.into(),
            done: done.into(),
        }
    }
}

/// A one-line question. The answer goes back to the screen that asked as
/// [`Action::PromptSubmitted`]; the screen remembers what it asked for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptRequest {
    /// The dialog title, such as `Fixed IP for nas-vault`.
    pub title: String,
    /// What to type, shown above the field.
    pub label: String,
    /// The value the field starts with.
    pub initial: String,
    /// A line of guidance under the field.
    pub hint: Option<String>,
    /// Whether the typed value is drawn masked.
    pub secret: bool,
}

/// A question the user answers before `action` is dispatched.
#[derive(Debug, Clone)]
pub struct ConfirmRequest {
    /// The question, such as `Restart sw-core?`.
    pub prompt: String,
    /// What saying yes costs, on its own line.
    pub detail: Option<String>,
    /// Dispatched on yes.
    pub action: Box<Action>,
}

impl ConfirmRequest {
    pub fn new(prompt: impl Into<String>, action: Action) -> Self {
        Self {
            prompt: prompt.into(),
            detail: None,
            action: Box::new(action),
        }
    }

    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
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
    /// Clear the terminal and draw the whole frame again.
    Repaint,
    /// Fetch the controller's data now, or retry a failed connection.
    Refresh,
    /// Switch to the Devices screen with this device's detail open.
    ShowDevice(EntityId),
    /// Switch to the Clients screen with this client's detail open.
    ShowClient(EntityId),
    /// Press a key as if it were typed, for a tapped key hint.
    PressKey(KeyEvent),
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
    WifiClientDetailFailed {
        ip: String,
        error: String,
    },
    WifiRoamHistoryLoaded {
        mac: String,
        events: Arc<Vec<Value>>,
    },
    WifiRoamHistoryFailed {
        mac: String,
        error: String,
    },

    // ── Connection Status ─────────────────────────────────────────
    Connecting,
    Connected,
    Disconnected(String),
    Reconnecting {
        attempt: u32,
    },
    RetryConnect,
    /// What the controller can reach, sent once it has connected.
    AccessUpdated(Access),
    /// A different controller is about to connect: every screen drops the
    /// data it holds, so nothing from the old one stays on screen.
    ControllerChanged,

    // ── Device Selection ──────────────────────────────────────────
    CloseDetail,

    // ── Client Selection ──────────────────────────────────────────
    WifiSubTab(WifiSubTab),
    WifiFocusAp(Option<EntityId>),
    WifiToggleChannelMap,
    WifiSortColumn(WifiSortField),
    WifiBandSelect(WifiBand),

    // ── Firewall ──────────────────────────────────────────────────
    FirewallSubTab(FirewallSubTab),

    // ── Network Editing ──────────────────────────────────────────
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
    ShowConfirm(ConfirmRequest),
    /// Ask for one line of text.
    OpenPrompt(PromptRequest),
    /// The prompt's answer, delivered to the active screen.
    PromptSubmitted(String),
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
    /// Dismiss every warning and error toast.
    DismissNotification,
    /// Dismiss one toast, by its id.
    DismissToast(u64),
    /// Show or hide the list of past notifications.
    ToggleNotices,
    /// Run a command on the controller, with toasts for its progress.
    Run {
        command: Box<unifly_api::Command>,
        notice: Notice,
    },
    /// A command or other controller operation finished: its progress
    /// toast, when it has one, becomes this notification.
    OpFinished {
        progress: Option<u64>,
        notification: Notification,
    },

    // ── Stats ─────────────────────────────────────────────────────
    RequestStats(StatsPeriod),
    StatsUpdated(StatsData),

    // ── Events Screen ─────────────────────────────────────────────
    /// Pause the live event feed, or resume it.
    ToggleEventPause,

    // ── Table Operations ──────────────────────────────────────────
    SortColumn(SortField),
    ScrollUp,
    ScrollDown,
    ScrollToTop,
    ScrollToBottom,
    PageUp,
    PageDown,

    // ── Settings ────────────────────────────────────────────────────
    OpenSettings,
    CloseSettings,

    // ── Donate ───────────────────────────────────────────────────
    OpenDonate,
    SetShowDonate(bool),

    // ── networks-firewall ──
    /// Create the network the form describes.
    NetworkCreate(Box<CreateNetworkRequest>),
    /// Check what uses a network, then ask before deleting it.
    RequestDeleteNetwork(EntityId),
    /// Move this policy `steps` places through its zone pair's ordering on
    /// the controller, up when negative. It names the policy, not a row
    /// index, so refreshes queued ahead of the action cannot redirect the
    /// move to another one.
    MovePolicy {
        policy: Arc<FirewallPolicy>,
        steps: i32,
    },
    /// A policy move ended; `moved` says whether the order changed.
    PolicyMoveSettled {
        moved: bool,
    },
    // ── WiFi ──────────────────────────────────────────────────────
    /// The neighbor scan could not be read; the WiFi screen shows why.
    WifiNeighborsFailed(String),
    /// The regulatory channel list could not be read.
    WifiChannelsFailed(String),
    // ── monitor ───────────────────────────────────────────────────
    /// Fetch events from further back than the feed holds.
    RequestEventHistory {
        within_hours: u32,
        limit: u32,
        include_firewall: bool,
    },
    /// The events a history fetch returned, for the Events screen to merge.
    EventHistoryLoaded {
        within_hours: u32,
        include_firewall: bool,
        events: Arc<Vec<Arc<Event>>>,
    },
    EventHistoryFailed(String),
    /// Fetch the controller's alarms.
    RequestAlarms,
    AlarmsLoaded(Arc<Vec<Alarm>>),
    AlarmsFailed(String),
    /// A stats fetch for this period is under way.
    StatsLoading(StatsPeriod),
    /// A stats fetch for this period failed.
    StatsFailed {
        period: StatsPeriod,
        message: String,
    },
    // ── devices-clients ───────────────────────────────────────────
    /// Adopt the pending device with this ID, after a confirmation.
    RequestAdoptDevice(EntityId),
    /// Push a device's configuration to it again, after a confirmation.
    RequestProvision(EntityId),
    /// Fetch a device's per-port configuration for the Devices screen.
    RequestPortProfiles(EntityId),
    PortProfilesLoaded {
        device: EntityId,
        ports: Arc<Vec<PortProfile>>,
    },
    PortProfilesFailed {
        device: EntityId,
        error: String,
    },
    /// Change one port's configuration on the controller.
    RequestPortUpdate {
        device: EntityId,
        port: u32,
        change: PortChange,
    },
    /// Reserve `ip` for a client on the network whose subnet holds it.
    RequestSetFixedIp {
        client: EntityId,
        ip: std::net::Ipv4Addr,
    },
    /// Drop a client's reserved address, after a confirmation.
    RequestRemoveFixedIp(EntityId),
    // ── setup ────────────────────────────────────────────────────
    /// A connection test started by the setup wizard or Settings finished.
    /// Only the test with this generation may act on it.
    SetupProbeFinished {
        generation: u64,
        result: Box<crate::tui::forms::probe::ProbeResult>,
    },
    /// The save that follows a passed test finished: the notices to show,
    /// or why nothing was saved.
    SetupSaveFinished {
        generation: u64,
        result: Result<Vec<String>, String>,
    },
    /// Which credentials the keyring holds for a profile a form is showing.
    SetupKeyringChecked {
        profile: String,
        api_key: bool,
        password: bool,
    },
    /// Connect the session with a profile a form has tested and saved.
    ApplyConnection {
        profile_name: String,
        config: Box<unifly_api::ControllerConfig>,
        notices: Vec<String>,
    },
    /// Open the setup wizard for a new profile with this name.
    SetupNewProfile(String),
    /// Leave a setup wizard that was opened from a running session.
    SetupCancelled,
}
