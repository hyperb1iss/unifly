use crate::core_error::CoreError;
use crate::model::EntityId;

use super::super::{Controller, integration_site_context, require_uuid};

impl Controller {
    pub async fn get_firewall_policy_ordering(
        &self,
        source_zone_id: &EntityId,
        destination_zone_id: &EntityId,
    ) -> Result<crate::integration_types::FirewallPolicyOrdering, CoreError> {
        #[cfg(feature = "demo")]
        if let Some(demo) = self.demo_runtime() {
            return Ok(demo.with(|sim| sim.policy_ordering(source_zone_id, destination_zone_id)));
        }
        let (client, site_id) =
            integration_site_context(self, "get_firewall_policy_ordering").await?;
        let source_zone_uuid = require_uuid(source_zone_id)?;
        let destination_zone_uuid = require_uuid(destination_zone_id)?;
        Ok(client
            .get_firewall_policy_ordering(&site_id, &source_zone_uuid, &destination_zone_uuid)
            .await?)
    }

    pub async fn get_acl_rule_ordering(
        &self,
    ) -> Result<crate::integration_types::AclRuleOrdering, CoreError> {
        let (client, site_id) = integration_site_context(self, "get_acl_rule_ordering").await?;
        Ok(client.get_acl_rule_ordering(&site_id).await?)
    }
}
