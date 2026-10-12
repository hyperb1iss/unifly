//! Lightweight live-statistics poll for controllers without a WebSocket.
//!
//! API-key-only and cloud sessions cannot open the session WebSocket, so
//! nothing pushes `device:sync` frames and the dashboard would only move on
//! the (slow, heavy) full refresh. This task re-fetches just the per-device
//! statistics from the Integration API on `polling_interval_secs` and
//! updates `Device.stats` in place under one snapshot publish, which is a
//! handful of ~1 KB requests instead of the ~30-request, ~450 KB full pass.

use std::sync::Arc;
use std::time::Duration;

use futures_util::stream::{self, StreamExt};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::core_error::CoreError;
use crate::model::{Device, EntityId, MacAddress};

use super::super::REFRESH_DETAIL_CONCURRENCY;
use super::Controller;

/// Background loop: poll device statistics every `interval_secs` until cancelled.
pub(in crate::controller) async fn stats_poll_task(
    controller: Controller,
    interval_secs: u64,
    cancel: CancellationToken,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    interval.tick().await;

    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => break,
            _ = interval.tick() => {
                if let Err(error) = controller.poll_device_stats().await {
                    warn!(error = %error, "device statistics poll failed");
                }
            }
        }
    }
}

impl Controller {
    /// Refresh only `Device.stats` (and the radio stats derived from the
    /// same response) for every device already in the store.
    ///
    /// No-op when the Integration client or site is not available (e.g. a
    /// session-only connection), because the WebSocket covers that case.
    pub(crate) async fn poll_device_stats(&self) -> Result<(), CoreError> {
        let Some(integration) = self.inner.integration_client.lock().await.clone() else {
            return Ok(());
        };
        let Some(site_id) = *self.inner.site_id.lock().await else {
            return Ok(());
        };

        let store = &self.inner.store;
        let targets: Vec<(MacAddress, uuid::Uuid)> = store
            .devices_snapshot()
            .iter()
            .filter_map(|device| match &device.id {
                EntityId::Uuid(id) => Some((device.mac.clone(), *id)),
                EntityId::Legacy(_) => None,
            })
            .collect();
        if targets.is_empty() {
            return Ok(());
        }

        let fetched: Vec<_> = stream::iter(targets.into_iter().map(|(mac, device_id)| {
            let integration = Arc::clone(&integration);
            async move {
                match integration
                    .get_device_statistics(&site_id, &device_id)
                    .await
                {
                    Ok(stats) => Some((mac, stats)),
                    Err(error) => {
                        warn!(device = %mac, error = %error, "device stats poll failed");
                        None
                    }
                }
            }
        }))
        .buffer_unordered(REFRESH_DETAIL_CONCURRENCY)
        .filter_map(std::future::ready)
        .collect()
        .await;

        // Apply onto whatever the store holds now, not the snapshot taken
        // before the requests went out: a full refresh may have landed in
        // between, and its names, ports, and removals must win.
        let batch = store.devices.begin_batch();
        for (mac, stats) in &fetched {
            let Some(current) = store.device_by_mac(mac) else {
                continue;
            };
            let mut device = Device::clone(&current);
            device.stats = crate::convert::device_stats_from_integration(stats);
            crate::convert::enrich_radios_from_stats(&mut device.radios, &stats.interfaces);
            store
                .devices
                .upsert(mac.as_str().to_owned(), device.id.clone(), device);
        }
        drop(batch);

        debug!(devices = fetched.len(), "device statistics poll applied");
        Ok(())
    }
}
