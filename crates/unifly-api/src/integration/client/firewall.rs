use tracing::debug;
use uuid::Uuid;

use super::{Error, IntegrationClient, types};

impl IntegrationClient {
    // ── Firewall Policies ────────────────────────────────────────────

    pub async fn list_firewall_policies(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::FirewallPolicyResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/firewall/policies"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    pub async fn get_firewall_policy(
        &self,
        site_id: &Uuid,
        policy_id: &Uuid,
    ) -> Result<types::FirewallPolicyResponse, Error> {
        self.get(&format!("v1/sites/{site_id}/firewall/policies/{policy_id}"))
            .await
    }

    pub async fn create_firewall_policy(
        &self,
        site_id: &Uuid,
        body: &types::FirewallPolicyCreateUpdate,
    ) -> Result<types::FirewallPolicyResponse, Error> {
        self.post(&format!("v1/sites/{site_id}/firewall/policies"), body)
            .await
    }

    pub async fn update_firewall_policy(
        &self,
        site_id: &Uuid,
        policy_id: &Uuid,
        body: &types::FirewallPolicyCreateUpdate,
    ) -> Result<types::FirewallPolicyResponse, Error> {
        self.put(
            &format!("v1/sites/{site_id}/firewall/policies/{policy_id}"),
            body,
        )
        .await
    }

    pub async fn patch_firewall_policy(
        &self,
        site_id: &Uuid,
        policy_id: &Uuid,
        body: &types::FirewallPolicyPatch,
    ) -> Result<types::FirewallPolicyResponse, Error> {
        self.patch(
            &format!("v1/sites/{site_id}/firewall/policies/{policy_id}"),
            body,
        )
        .await
    }

    pub async fn delete_firewall_policy(
        &self,
        site_id: &Uuid,
        policy_id: &Uuid,
    ) -> Result<(), Error> {
        self.delete(&format!("v1/sites/{site_id}/firewall/policies/{policy_id}"))
            .await
    }

    pub async fn get_firewall_policy_ordering(
        &self,
        site_id: &Uuid,
        source_zone_id: &Uuid,
        destination_zone_id: &Uuid,
    ) -> Result<types::FirewallPolicyOrdering, Error> {
        let envelope: types::FirewallPolicyOrderingEnvelope = self
            .get_with_params(
                &format!("v1/sites/{site_id}/firewall/policies/ordering"),
                &[
                    ("sourceFirewallZoneId", source_zone_id.to_string()),
                    ("destinationFirewallZoneId", destination_zone_id.to_string()),
                ],
            )
            .await?;
        Ok(envelope.ordered_firewall_policy_ids)
    }

    pub async fn set_firewall_policy_ordering(
        &self,
        site_id: &Uuid,
        source_zone_id: &Uuid,
        destination_zone_id: &Uuid,
        body: &types::FirewallPolicyOrdering,
    ) -> Result<types::FirewallPolicyOrdering, Error> {
        let url = self.url(&format!("v1/sites/{site_id}/firewall/policies/ordering"));
        debug!(
            "PUT {url} params={:?}",
            &[
                ("sourceFirewallZoneId", source_zone_id.to_string()),
                ("destinationFirewallZoneId", destination_zone_id.to_string(),),
            ]
        );

        let envelope = types::FirewallPolicyOrderingEnvelope {
            ordered_firewall_policy_ids: body.clone(),
        };
        let resp = self
            .send(
                self.http
                    .put(url)
                    .query(&[
                        ("sourceFirewallZoneId", source_zone_id.to_string()),
                        ("destinationFirewallZoneId", destination_zone_id.to_string()),
                    ])
                    .json(&envelope),
            )
            .await?;
        let result: types::FirewallPolicyOrderingEnvelope = self.handle_response(resp).await?;
        Ok(result.ordered_firewall_policy_ids)
    }

    // ── NAT Policies ─────────────────────────────────────────────────

    pub async fn list_nat_policies(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::NatPolicyResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/firewall/nat"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    pub async fn get_nat_policy(
        &self,
        site_id: &Uuid,
        policy_id: &Uuid,
    ) -> Result<types::NatPolicyResponse, Error> {
        self.get(&format!("v1/sites/{site_id}/firewall/nat/{policy_id}"))
            .await
    }

    pub async fn create_nat_policy(
        &self,
        site_id: &Uuid,
        body: &types::NatPolicyCreateUpdate,
    ) -> Result<types::NatPolicyResponse, Error> {
        self.post(&format!("v1/sites/{site_id}/firewall/nat"), body)
            .await
    }

    pub async fn update_nat_policy(
        &self,
        site_id: &Uuid,
        policy_id: &Uuid,
        body: &types::NatPolicyCreateUpdate,
    ) -> Result<types::NatPolicyResponse, Error> {
        self.put(
            &format!("v1/sites/{site_id}/firewall/nat/{policy_id}"),
            body,
        )
        .await
    }

    pub async fn delete_nat_policy(&self, site_id: &Uuid, policy_id: &Uuid) -> Result<(), Error> {
        self.delete(&format!("v1/sites/{site_id}/firewall/nat/{policy_id}"))
            .await
    }

    // ── Firewall Zones ───────────────────────────────────────────────

    pub async fn list_firewall_zones(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::FirewallZoneResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/firewall/zones"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    pub async fn get_firewall_zone(
        &self,
        site_id: &Uuid,
        zone_id: &Uuid,
    ) -> Result<types::FirewallZoneResponse, Error> {
        self.get(&format!("v1/sites/{site_id}/firewall/zones/{zone_id}"))
            .await
    }

    pub async fn create_firewall_zone(
        &self,
        site_id: &Uuid,
        body: &types::FirewallZoneCreateUpdate,
    ) -> Result<types::FirewallZoneResponse, Error> {
        self.post(&format!("v1/sites/{site_id}/firewall/zones"), body)
            .await
    }

    pub async fn update_firewall_zone(
        &self,
        site_id: &Uuid,
        zone_id: &Uuid,
        body: &types::FirewallZoneCreateUpdate,
    ) -> Result<types::FirewallZoneResponse, Error> {
        self.put(
            &format!("v1/sites/{site_id}/firewall/zones/{zone_id}"),
            body,
        )
        .await
    }

    pub async fn delete_firewall_zone(&self, site_id: &Uuid, zone_id: &Uuid) -> Result<(), Error> {
        self.delete(&format!("v1/sites/{site_id}/firewall/zones/{zone_id}"))
            .await
    }
}
