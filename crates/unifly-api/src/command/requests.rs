// ── Typed request structs for Command payloads ──
//
// Every Command variant that previously took `serde_json::Value`
// now uses one of these strongly-typed request structs instead.

mod dns;
mod network;
mod policy;
mod traffic;
mod vouchers;
mod vpn;

pub use dns::{CreateDnsPolicyRequest, UpdateDnsPolicyRequest};
pub use network::{
    CreateNetworkRequest, CreateWifiBroadcastRequest, UpdateNetworkRequest,
    UpdateWifiBroadcastRequest,
};
pub use policy::{
    CreateAclRuleRequest, CreateFirewallPolicyRequest, CreateFirewallZoneRequest,
    CreateNatPolicyRequest, PortSpec, TrafficFilterSpec, UpdateAclRuleRequest,
    UpdateFirewallPolicyRequest, UpdateFirewallZoneRequest, UpdateNatPolicyRequest,
};
pub use traffic::{CreateTrafficMatchingListRequest, UpdateTrafficMatchingListRequest};
pub use vouchers::CreateVouchersRequest;
pub use vpn::{
    CreateRemoteAccessVpnServerRequest, CreateSiteToSiteVpnRequest, CreateVpnClientProfileRequest,
    CreateWireGuardPeerRequest, UpdateRemoteAccessVpnServerRequest, UpdateSiteToSiteVpnRequest,
    UpdateVpnClientProfileRequest, UpdateWireGuardPeerRequest,
};
