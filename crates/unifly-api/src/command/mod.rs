// ── Command API ──
//
// All write operations flow through a unified `Command` enum.
// The controller routes each variant to the appropriate API backend
// (Integration API preferred, Session API for session-only operations).

pub mod requests;

use crate::core_error::CoreError;
use crate::model::{
    AclRule, Client, Device, DnsPolicy, EntityId, FirewallPolicy, FirewallZone, MacAddress,
    Network, TrafficMatchingList, Voucher, WifiBroadcast,
};

pub use requests::{
    CreateAclRuleRequest, CreateDnsPolicyRequest, CreateFirewallPolicyRequest,
    CreateFirewallZoneRequest, CreateNatPolicyRequest, CreateNetworkRequest,
    CreateRemoteAccessVpnServerRequest, CreateSiteToSiteVpnRequest,
    CreateTrafficMatchingListRequest, CreateVouchersRequest, CreateVpnClientProfileRequest,
    CreateWifiBroadcastRequest, CreateWireGuardPeerRequest, TrafficFilterSpec,
    UpdateAclRuleRequest, UpdateDnsPolicyRequest, UpdateFirewallPolicyRequest,
    UpdateFirewallZoneRequest, UpdateNatPolicyRequest, UpdateNetworkRequest,
    UpdateRemoteAccessVpnServerRequest, UpdateSiteToSiteVpnRequest,
    UpdateTrafficMatchingListRequest, UpdateVpnClientProfileRequest, UpdateWifiBroadcastRequest,
    UpdateWireGuardPeerRequest,
};

/// A command envelope sent through the command channel.
/// Contains the command and a oneshot response channel.
pub(crate) struct CommandEnvelope {
    pub command: Command,
    pub response_tx: tokio::sync::oneshot::Sender<Result<CommandResult, CoreError>>,
}

/// All possible write operations against a UniFi controller.
#[derive(Debug, Clone)]
pub enum Command {
    // ── Device operations ────────────────────────────────────────────
    AdoptDevice {
        mac: MacAddress,
        ignore_device_limit: bool,
    },
    RemoveDevice {
        id: EntityId,
    },
    RestartDevice {
        id: EntityId,
    },
    LocateDevice {
        mac: MacAddress,
        enable: bool,
    },
    UpgradeDevice {
        mac: MacAddress,
        firmware_url: Option<String>,
    },
    ProvisionDevice {
        mac: MacAddress,
    },
    SpeedtestDevice,
    PowerCyclePort {
        device_id: EntityId,
        port_idx: u32,
    },

    // ── Client operations ────────────────────────────────────────────
    BlockClient {
        mac: MacAddress,
    },
    UnblockClient {
        mac: MacAddress,
    },
    KickClient {
        mac: MacAddress,
    },
    ForgetClient {
        mac: MacAddress,
    },
    AuthorizeGuest {
        client_id: EntityId,
        time_limit_minutes: Option<u32>,
        data_limit_mb: Option<u64>,
        rx_rate_kbps: Option<u64>,
        tx_rate_kbps: Option<u64>,
    },
    UnauthorizeGuest {
        client_id: EntityId,
    },
    SetClientFixedIp {
        mac: MacAddress,
        ip: std::net::Ipv4Addr,
        network_id: EntityId,
    },
    RemoveClientFixedIp {
        mac: MacAddress,
        network_id: Option<EntityId>,
    },

    // ── Network CRUD ─────────────────────────────────────────────────
    CreateNetwork(CreateNetworkRequest),
    UpdateNetwork {
        id: EntityId,
        update: UpdateNetworkRequest,
    },
    DeleteNetwork {
        id: EntityId,
        force: bool,
    },

    // ── WiFi CRUD ────────────────────────────────────────────────────
    CreateWifiBroadcast(CreateWifiBroadcastRequest),
    UpdateWifiBroadcast {
        id: EntityId,
        update: UpdateWifiBroadcastRequest,
    },
    DeleteWifiBroadcast {
        id: EntityId,
        force: bool,
    },

    // ── Firewall ─────────────────────────────────────────────────────
    CreateFirewallPolicy(CreateFirewallPolicyRequest),
    UpdateFirewallPolicy {
        id: EntityId,
        update: UpdateFirewallPolicyRequest,
    },
    DeleteFirewallPolicy {
        id: EntityId,
    },
    PatchFirewallPolicy {
        id: EntityId,
        enabled: Option<bool>,
        logging: Option<bool>,
    },
    ReorderFirewallPolicies {
        zone_pair: (EntityId, EntityId),
        ordered_ids: Vec<EntityId>,
        after_system: bool,
    },
    CreateFirewallZone(CreateFirewallZoneRequest),
    UpdateFirewallZone {
        id: EntityId,
        update: UpdateFirewallZoneRequest,
    },
    DeleteFirewallZone {
        id: EntityId,
    },

    // ── NAT ──────────────────────────────────────────────────────────
    CreateNatPolicy(CreateNatPolicyRequest),
    UpdateNatPolicy {
        id: EntityId,
        update: UpdateNatPolicyRequest,
    },
    DeleteNatPolicy {
        id: EntityId,
    },

    // ── VPN (Legacy) ────────────────────────────────────────────────
    CreateSiteToSiteVpn(CreateSiteToSiteVpnRequest),
    UpdateSiteToSiteVpn {
        id: EntityId,
        update: UpdateSiteToSiteVpnRequest,
    },
    DeleteSiteToSiteVpn {
        id: EntityId,
    },
    CreateRemoteAccessVpnServer(CreateRemoteAccessVpnServerRequest),
    UpdateRemoteAccessVpnServer {
        id: EntityId,
        update: UpdateRemoteAccessVpnServerRequest,
    },
    DeleteRemoteAccessVpnServer {
        id: EntityId,
    },
    CreateVpnClientProfile(CreateVpnClientProfileRequest),
    UpdateVpnClientProfile {
        id: EntityId,
        update: UpdateVpnClientProfileRequest,
    },
    DeleteVpnClientProfile {
        id: EntityId,
    },
    CreateWireGuardPeer {
        server_id: EntityId,
        peer: CreateWireGuardPeerRequest,
    },
    UpdateWireGuardPeer {
        server_id: EntityId,
        peer_id: EntityId,
        update: UpdateWireGuardPeerRequest,
    },
    DeleteWireGuardPeer {
        server_id: EntityId,
        peer_id: EntityId,
    },
    RestartVpnClientConnection {
        id: EntityId,
    },

    // ── ACL ──────────────────────────────────────────────────────────
    CreateAclRule(CreateAclRuleRequest),
    UpdateAclRule {
        id: EntityId,
        update: UpdateAclRuleRequest,
    },
    DeleteAclRule {
        id: EntityId,
    },
    ReorderAclRules {
        ordered_ids: Vec<EntityId>,
    },

    // ── DNS ──────────────────────────────────────────────────────────
    CreateDnsPolicy(CreateDnsPolicyRequest),
    UpdateDnsPolicy {
        id: EntityId,
        update: UpdateDnsPolicyRequest,
    },
    DeleteDnsPolicy {
        id: EntityId,
    },

    // ── Traffic matching lists ───────────────────────────────────────
    CreateTrafficMatchingList(CreateTrafficMatchingListRequest),
    UpdateTrafficMatchingList {
        id: EntityId,
        update: UpdateTrafficMatchingListRequest,
    },
    DeleteTrafficMatchingList {
        id: EntityId,
    },

    // ── Hotspot / Vouchers ───────────────────────────────────────────
    CreateVouchers(CreateVouchersRequest),
    DeleteVoucher {
        id: EntityId,
    },
    PurgeVouchers {
        filter: String,
    },

    // ── Site settings (Session) ─────────────────────────────────────────
    SetDpiEnabled {
        enabled: bool,
    },

    // ── System (Session) ──────────────────────────────────────────────
    ArchiveAlarm {
        id: EntityId,
    },
    ArchiveAllAlarms,
    CreateSite {
        name: String,
        description: String,
    },
    DeleteSite {
        name: String,
    },
    CreateBackup,
    DeleteBackup {
        filename: String,
    },
    RebootController,
    PoweroffController,
    InviteAdmin {
        name: String,
        email: String,
        role: String,
    },
    RevokeAdmin {
        id: EntityId,
    },
    UpdateAdmin {
        id: EntityId,
        role: Option<String>,
    },
}

/// Result of a command execution.
#[derive(Debug)]
pub enum CommandResult {
    Ok,
    Device(Device),
    Client(Client),
    Network(Network),
    WifiBroadcast(WifiBroadcast),
    FirewallPolicy(FirewallPolicy),
    FirewallZone(FirewallZone),
    AclRule(AclRule),
    DnsPolicy(DnsPolicy),
    Vouchers(Vec<Voucher>),
    TrafficMatchingList(TrafficMatchingList),
}
