use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::core_error::CoreError;
use crate::model::Event;
use crate::store::{DataStore, RefreshSnapshot, event_storage_key};

use super::Controller;

mod integration;
mod merge;
mod session;

impl Controller {
    /// Fetch all data from the controller and update the DataStore.
    ///
    /// Pulls devices, clients, and events from the controller APIs, converts
    /// them to domain types, and applies them to the store. Events are
    /// broadcast through the event channel after snapshot application.
    pub async fn full_refresh(&self) -> Result<(), CoreError> {
        let integration = self.inner.integration_client.lock().await.clone();
        let site_id = *self.inner.site_id.lock().await;

        if let (Some(integration), Some(site_id)) = (integration, site_id) {
            self.refresh_integration_snapshot(integration, site_id)
                .await?;
        } else {
            self.refresh_session_snapshot().await?;
        }

        debug!(
            devices = self.inner.store.device_count(),
            clients = self.inner.store.client_count(),
            "data refresh complete"
        );

        Ok(())
    }

    async fn refresh_integration_snapshot(
        &self,
        integration: Arc<crate::IntegrationClient>,
        site_id: uuid::Uuid,
    ) -> Result<(), CoreError> {
        let mut integration = integration::fetch(integration, site_id).await?;
        let session_client = self.inner.session_client.lock().await.clone();
        let session = session::fetch_optional(session_client).await;

        merge::merge_session_clients(&mut integration.clients, &session.clients, &session.users);
        merge::merge_session_devices(&mut integration.devices, &session.devices);

        if !session.health.is_empty() {
            let health = Arc::new(session.health);
            self.inner
                .store
                .site_health
                .send_modify(|current| *current = Arc::clone(&health));
        }

        let fresh_events = unseen_events(self.store(), &session.events);
        self.inner
            .store
            .apply_integration_snapshot(RefreshSnapshot {
                devices: integration.devices,
                clients: integration.clients,
                networks: integration.networks,
                wifi: integration.wifi,
                policies: integration.policies,
                zones: integration.zones,
                acls: integration.acls,
                nat: session.nat,
                dns: integration.dns,
                vouchers: integration.vouchers,
                sites: integration.sites,
                events: session.events,
                traffic_matching_lists: integration.traffic_matching_lists,
                firewall_groups: session.firewall_groups,
            });
        self.publish_events(fresh_events);

        Ok(())
    }

    async fn refresh_session_snapshot(&self) -> Result<(), CoreError> {
        let session_client = self
            .inner
            .session_client
            .lock()
            .await
            .clone()
            .ok_or(CoreError::ControllerDisconnected)?;
        let session = session::fetch_required(session_client).await?;
        let fresh_events = unseen_events(self.store(), &session.events);

        self.inner
            .store
            .apply_integration_snapshot(RefreshSnapshot {
                devices: session.devices,
                clients: session.clients,
                networks: Vec::new(),
                wifi: Vec::new(),
                policies: Vec::new(),
                zones: Vec::new(),
                acls: Vec::new(),
                nat: session.nat,
                dns: Vec::new(),
                vouchers: Vec::new(),
                sites: session.sites,
                events: session.events,
                traffic_matching_lists: Vec::new(),
                firewall_groups: session.firewall_groups,
            });
        self.publish_events(fresh_events);

        Ok(())
    }

    fn publish_events(&self, events: Vec<Event>) {
        for event in events {
            let _ = self.inner.event_tx.send(Arc::new(event));
        }
    }
}

/// Periodically refresh data from the controller.
pub(super) async fn refresh_task(
    controller: Controller,
    interval_secs: u64,
    cancel: CancellationToken,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
    interval.tick().await;

    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => break,
            _ = interval.tick() => {
                if let Err(error) = controller.full_refresh().await {
                    warn!(error = %error, "periodic refresh failed");
                }
            }
        }
    }
}

fn unseen_events(store: &DataStore, events: &[Event]) -> Vec<Event> {
    let mut seen: HashSet<String> = store
        .events_snapshot()
        .iter()
        .map(|event| event_storage_key(event))
        .collect();

    events
        .iter()
        .filter(|event| seen.insert(event_storage_key(event)))
        .cloned()
        .collect()
}
