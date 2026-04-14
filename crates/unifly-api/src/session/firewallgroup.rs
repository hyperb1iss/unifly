// Legacy firewall group operations.
//
// Manages port groups, address groups, and IPv6 address groups via the
// classic `rest/firewallgroup` collection behind the Session API.

use serde_json::Value;
use tracing::debug;

use super::SessionClient;
use crate::error::Error;

impl SessionClient {
    /// List all firewall groups for the current site.
    pub async fn list_firewall_groups(&self) -> Result<Vec<Value>, Error> {
        let url = self.site_url("rest/firewallgroup");
        debug!("listing firewall groups");
        self.get(url).await
    }

    /// Create a firewall group for the current site.
    pub async fn create_firewall_group(&self, body: &Value) -> Result<Vec<Value>, Error> {
        let url = self.site_url("rest/firewallgroup");
        debug!("creating firewall group");
        self.post(url, body).await
    }

    /// Update a firewall group for the current site.
    pub async fn update_firewall_group(
        &self,
        record_id: &str,
        body: &Value,
    ) -> Result<Vec<Value>, Error> {
        let url = self.site_url(&format!("rest/firewallgroup/{record_id}"));
        debug!(record_id, "updating firewall group");
        self.put(url, body).await
    }

    /// Delete a firewall group for the current site.
    pub async fn delete_firewall_group(&self, record_id: &str) -> Result<Vec<Value>, Error> {
        let url = self.site_url(&format!("rest/firewallgroup/{record_id}"));
        debug!(record_id, "deleting firewall group");
        self.delete(url).await
    }
}
