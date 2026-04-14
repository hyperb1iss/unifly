use std::sync::Arc;

use crate::command::{Command, CommandResult};
use crate::core_error::CoreError;
use crate::store::DataStore;
use crate::{IntegrationClient, SessionClient};

use super::Controller;

mod device_client;
mod network;
mod policy;
mod system;
mod vpn;

pub(super) use super::payloads::{
    build_acl_filter_value, build_create_dns_policy_fields, build_create_wifi_broadcast_payload,
    build_endpoint_json, build_update_dns_policy_fields, build_update_wifi_broadcast_payload,
    dns_policy_type_name, merge_acl_filter_value, parse_ipv4_cidr, traffic_matching_list_items,
};
pub(super) use super::{
    client_mac, device_mac, require_integration, require_session, require_uuid,
};

struct CommandContext {
    store: Arc<DataStore>,
    integration: Option<Arc<IntegrationClient>>,
    session: Option<Arc<SessionClient>>,
    site_id: Option<uuid::Uuid>,
}

impl CommandContext {
    async fn snapshot(controller: &Controller) -> Self {
        Self {
            store: controller.inner.store.clone(),
            integration: controller.inner.integration_client.lock().await.clone(),
            session: controller.inner.session_client.lock().await.clone(),
            site_id: *controller.inner.site_id.lock().await,
        }
    }
}

#[allow(clippy::cognitive_complexity)]
pub(super) async fn route_command(
    controller: &Controller,
    cmd: Command,
) -> Result<CommandResult, CoreError> {
    let ctx = CommandContext::snapshot(controller).await;

    match cmd {
        cmd @ (Command::AdoptDevice { .. }
        | Command::RestartDevice { .. }
        | Command::LocateDevice { .. }
        | Command::UpgradeDevice { .. }
        | Command::RemoveDevice { .. }
        | Command::ProvisionDevice { .. }
        | Command::SpeedtestDevice
        | Command::PowerCyclePort { .. }
        | Command::BlockClient { .. }
        | Command::UnblockClient { .. }
        | Command::KickClient { .. }
        | Command::ForgetClient { .. }
        | Command::AuthorizeGuest { .. }
        | Command::UnauthorizeGuest { .. }
        | Command::SetClientFixedIp { .. }
        | Command::RemoveClientFixedIp { .. }) => device_client::route(&ctx, cmd).await,
        cmd @ (Command::CreateNetwork(_)
        | Command::UpdateNetwork { .. }
        | Command::DeleteNetwork { .. }
        | Command::CreateWifiBroadcast(_)
        | Command::UpdateWifiBroadcast { .. }
        | Command::DeleteWifiBroadcast { .. }) => network::route(&ctx, cmd).await,
        cmd @ (Command::CreateFirewallPolicy(_)
        | Command::UpdateFirewallPolicy { .. }
        | Command::DeleteFirewallPolicy { .. }
        | Command::PatchFirewallPolicy { .. }
        | Command::ReorderFirewallPolicies { .. }
        | Command::CreateFirewallZone(_)
        | Command::UpdateFirewallZone { .. }
        | Command::DeleteFirewallZone { .. }
        | Command::CreateAclRule(_)
        | Command::UpdateAclRule { .. }
        | Command::DeleteAclRule { .. }
        | Command::ReorderAclRules { .. }
        | Command::CreateNatPolicy(_)
        | Command::UpdateNatPolicy { .. }
        | Command::DeleteNatPolicy { .. }
        | Command::CreateDnsPolicy(_)
        | Command::UpdateDnsPolicy { .. }
        | Command::DeleteDnsPolicy { .. }
        | Command::CreateTrafficMatchingList(_)
        | Command::UpdateTrafficMatchingList { .. }
        | Command::DeleteTrafficMatchingList { .. }
        | Command::CreateFirewallGroup(_)
        | Command::UpdateFirewallGroup { .. }
        | Command::DeleteFirewallGroup { .. }) => policy::route(&ctx, cmd).await,
        cmd @ (Command::CreateSiteToSiteVpn(_)
        | Command::UpdateSiteToSiteVpn { .. }
        | Command::DeleteSiteToSiteVpn { .. }
        | Command::CreateRemoteAccessVpnServer(_)
        | Command::UpdateRemoteAccessVpnServer { .. }
        | Command::DeleteRemoteAccessVpnServer { .. }
        | Command::CreateVpnClientProfile(_)
        | Command::UpdateVpnClientProfile { .. }
        | Command::DeleteVpnClientProfile { .. }
        | Command::CreateWireGuardPeer { .. }
        | Command::UpdateWireGuardPeer { .. }
        | Command::DeleteWireGuardPeer { .. }
        | Command::RestartVpnClientConnection { .. }) => vpn::route(&ctx, cmd).await,
        cmd @ (Command::SetDpiEnabled { .. }
        | Command::ArchiveAlarm { .. }
        | Command::ArchiveAllAlarms
        | Command::CreateBackup
        | Command::DeleteBackup { .. }
        | Command::CreateVouchers(_)
        | Command::DeleteVoucher { .. }
        | Command::PurgeVouchers { .. }
        | Command::CreateSite { .. }
        | Command::DeleteSite { .. }
        | Command::InviteAdmin { .. }
        | Command::RevokeAdmin { .. }
        | Command::UpdateAdmin { .. }
        | Command::RebootController
        | Command::PoweroffController) => system::route(&ctx, cmd).await,
    }
}
