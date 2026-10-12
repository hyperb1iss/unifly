use crate::core_error::CoreError;
use crate::model::{EntityId, WifiBroadcast};

use super::super::{Controller, integration_site_context, require_uuid};

impl Controller {
    pub async fn get_wifi_broadcast_detail(
        &self,
        id: &EntityId,
    ) -> Result<WifiBroadcast, CoreError> {
        // The demo store already holds every broadcast in full.
        #[cfg(feature = "demo")]
        if self.is_demo() {
            return self
                .wifi_broadcasts_snapshot()
                .iter()
                .find(|broadcast| &broadcast.id == id)
                .map(|broadcast| WifiBroadcast::clone(broadcast))
                .ok_or_else(|| CoreError::NotFound {
                    entity_type: "wifi broadcast".into(),
                    identifier: id.to_string(),
                });
        }
        let (client, site_id) = integration_site_context(self, "get_wifi_broadcast_detail").await?;
        let uuid = require_uuid(id)?;
        let detail = client.get_wifi_broadcast(&site_id, &uuid).await?;
        Ok(WifiBroadcast::from(detail))
    }

    pub async fn list_pending_devices(&self) -> Result<Vec<serde_json::Value>, CoreError> {
        let integration = self.inner.integration_client.lock().await.clone();
        let site_id = *self.inner.site_id.lock().await;

        if let (Some(client), Some(site_id)) = (integration, site_id) {
            let raw = client
                .paginate_all(200, |offset, limit| {
                    client.list_pending_devices(&site_id, offset, limit)
                })
                .await?;
            return Ok(raw
                .into_iter()
                .map(|value| serde_json::to_value(value).unwrap_or_default())
                .collect());
        }

        // Store snapshots carry no stable order, so sort by MAC to keep
        // paged output repeatable.
        let snapshot = self.devices_snapshot();
        let mut pending: Vec<_> = snapshot
            .iter()
            .filter(|device| device.state == crate::model::DeviceState::PendingAdoption)
            .collect();
        pending.sort_by(|left, right| left.mac.as_str().cmp(right.mac.as_str()));
        Ok(pending
            .into_iter()
            .map(|device| serde_json::to_value(device.as_ref()).unwrap_or_default())
            .collect())
    }

    pub async fn list_device_tags(&self) -> Result<Vec<serde_json::Value>, CoreError> {
        let integration = self.inner.integration_client.lock().await.clone();
        let site_id = *self.inner.site_id.lock().await;

        if let (Some(client), Some(site_id)) = (integration, site_id) {
            let raw = client
                .paginate_all(200, |offset, limit| {
                    client.list_device_tags(&site_id, offset, limit)
                })
                .await?;
            return Ok(raw
                .into_iter()
                .map(|value| serde_json::to_value(value).unwrap_or_default())
                .collect());
        }

        Ok(Vec::new())
    }
}
