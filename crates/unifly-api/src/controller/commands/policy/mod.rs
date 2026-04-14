use crate::command::{Command, CommandResult};
use crate::core_error::CoreError;

use super::CommandContext;

mod acl;
mod dns;
mod firewall;
mod firewall_groups;
mod nat;
mod traffic_lists;

pub(super) async fn route(ctx: &CommandContext, cmd: Command) -> Result<CommandResult, CoreError> {
    match cmd {
        cmd @ (Command::CreateFirewallPolicy(_)
        | Command::UpdateFirewallPolicy { .. }
        | Command::DeleteFirewallPolicy { .. }
        | Command::PatchFirewallPolicy { .. }
        | Command::ReorderFirewallPolicies { .. }
        | Command::CreateFirewallZone(_)
        | Command::UpdateFirewallZone { .. }
        | Command::DeleteFirewallZone { .. }) => firewall::route(ctx, cmd).await,
        cmd @ (Command::CreateAclRule(_)
        | Command::UpdateAclRule { .. }
        | Command::DeleteAclRule { .. }
        | Command::ReorderAclRules { .. }) => acl::route(ctx, cmd).await,
        cmd @ (Command::CreateDnsPolicy(_)
        | Command::UpdateDnsPolicy { .. }
        | Command::DeleteDnsPolicy { .. }) => dns::route(ctx, cmd).await,
        cmd @ (Command::CreateNatPolicy(_)
        | Command::UpdateNatPolicy { .. }
        | Command::DeleteNatPolicy { .. }) => nat::route(ctx, cmd).await,
        cmd @ (Command::CreateTrafficMatchingList(_)
        | Command::UpdateTrafficMatchingList { .. }
        | Command::DeleteTrafficMatchingList { .. }) => traffic_lists::route(ctx, cmd).await,
        cmd @ (Command::CreateFirewallGroup(_)
        | Command::UpdateFirewallGroup { .. }
        | Command::DeleteFirewallGroup { .. }) => firewall_groups::route(ctx, cmd).await,
        _ => unreachable!("policy::route received non-policy command"),
    }
}
