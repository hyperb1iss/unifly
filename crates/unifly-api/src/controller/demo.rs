// ── Demo-mode controller glue ──
//
// A demo controller builds no HTTP clients. `connect()` seeds the store from
// the simulation and starts a ticking task under the connection's child
// token; `disconnect()` cancels it like any other background task. Query
// and command methods branch here once, at their top.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

use super::{ConnectionState, Controller};
use crate::command::{Command, CommandResult};
use crate::config::ControllerConfig;
use crate::core_error::CoreError;
use crate::demo::sim::Delta;
use crate::demo::{DemoFleetOptions, DemoRuntime};
use crate::model::Event;
use crate::store::DataStore;

/// Simulation cadence.
const TICK: Duration = Duration::from_secs(1);
/// Nominal controller address for the demo site (never contacted).
const DEMO_URL: &str = "https://10.40.0.1";

impl Controller {
    /// Build a controller backed by a synthetic, offline demo fleet.
    ///
    /// No network, credentials, or controller are involved. [`connect`]
    /// seeds the store with the generated fleet and starts a simulation
    /// that animates it about once a second; the same
    /// [`seed`](DemoFleetOptions::seed) yields the same fleet and the same
    /// animation sequence.
    ///
    /// [`connect`]: Self::connect
    pub fn demo(options: DemoFleetOptions) -> Self {
        let mut config = ControllerConfig {
            websocket_enabled: false,
            refresh_interval_secs: 0,
            polling_interval_secs: 0,
            no_session_cache: true,
            ..ControllerConfig::default()
        };
        if let Ok(url) = url::Url::parse(DEMO_URL) {
            config.url = url;
        }
        let controller = Self::new(config);
        let _ = controller
            .inner
            .demo
            .set(Arc::new(DemoRuntime::new(options)));
        controller
    }

    /// Whether this controller serves the synthetic demo fleet.
    pub fn is_demo(&self) -> bool {
        self.inner.demo.get().is_some()
    }

    pub(crate) fn demo_runtime(&self) -> Option<&Arc<DemoRuntime>> {
        self.inner.demo.get()
    }

    /// Demo `connect()`: seed the store and start the simulation. Calling
    /// it again restarts the simulation task rather than adding a second
    /// one, so connect is safe to repeat.
    pub(super) async fn demo_connect(&self, runtime: Arc<DemoRuntime>) -> Result<(), CoreError> {
        self.inner
            .connection_state
            .send_replace(ConnectionState::Connecting);
        let child = self.inner.cancel.child_token();
        let previous = std::mem::replace(&mut *self.inner.cancel_child.lock().await, child.clone());
        previous.cancel();
        let stale: Vec<_> = self.inner.task_handles.lock().await.drain(..).collect();
        for handle in stale {
            let _ = handle.await;
        }

        let (snapshot, health) = runtime.with(|sim| (sim.refresh_snapshot(), sim.health()));
        self.inner.store.apply_integration_snapshot(snapshot);
        self.inner.store.site_health.send_replace(Arc::new(health));

        let store = Arc::clone(&self.inner.store);
        let events = self.inner.event_tx.clone();
        let seed = runtime.seed();
        self.inner
            .task_handles
            .lock()
            .await
            .push(tokio::spawn(simulation_task(runtime, store, events, child)));

        self.inner
            .connection_state
            .send_replace(ConnectionState::Connected);
        info!(seed, "connected to demo fleet");
        Ok(())
    }

    /// Demo `execute()`: apply the command to the simulation. Like a real
    /// controller, commands need a live connection.
    pub(super) async fn demo_execute(
        &self,
        runtime: &DemoRuntime,
        command: Command,
    ) -> Result<CommandResult, CoreError> {
        if self.inner.task_handles.lock().await.is_empty() {
            return Err(CoreError::ControllerDisconnected);
        }
        let (result, delta) = runtime.with(|sim| sim.apply_command(command, Utc::now()))?;
        apply_delta(&self.inner.store, &self.inner.event_tx, delta);
        Ok(result)
    }
}

async fn simulation_task(
    runtime: Arc<DemoRuntime>,
    store: Arc<DataStore>,
    events: broadcast::Sender<Arc<Event>>,
    cancel: CancellationToken,
) {
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // The first tick completes immediately; the store was just seeded.
    ticker.tick().await;
    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => break,
            _ = ticker.tick() => {
                let delta = runtime.with(|sim| sim.step(Utc::now()));
                apply_delta(&store, &events, delta);
            }
        }
    }
    debug!("demo simulation stopped");
}

/// Push a simulation delta into the store and the event broadcast, the
/// same paths a refresh and the WebSocket bridge use.
fn apply_delta(store: &DataStore, events: &broadcast::Sender<Arc<Event>>, delta: Delta) {
    if !delta.devices.is_empty() {
        let _batch = store.devices.begin_batch();
        for device in delta.devices {
            let key = device.mac.as_str().to_owned();
            store.devices.update_existing(&key, move |_| device);
        }
    }

    if !delta.clients.is_empty() || !delta.removed.is_empty() {
        let _batch = store.clients.begin_batch();
        for mac in &delta.removed {
            store.clients.remove(mac.as_str());
        }
        for client in delta.clients {
            let key = client.mac.as_str().to_owned();
            let id = client.id.clone();
            store.clients.upsert(key, id, client);
        }
    }

    if !delta.policies.is_empty() {
        let _batch = store.firewall_policies.begin_batch();
        for policy in delta.policies {
            let key = format!("fwp:{}", policy.id);
            let id = policy.id.clone();
            store.firewall_policies.upsert(key, id, policy);
        }
    }

    if let Some(health) = delta.health {
        store.site_health.send_replace(Arc::new(health));
    }

    if !delta.events.is_empty() {
        store.mark_ws_event(Utc::now());
        for event in delta.events {
            let _ = events.send(Arc::new(event));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::Controller;
    use crate::demo::DemoFleetOptions;

    #[tokio::test]
    async fn repeated_connect_keeps_one_simulation_and_disconnects() {
        let controller = Controller::demo(DemoFleetOptions::default());
        for _ in 0..3 {
            controller.connect().await.expect("connect");
        }
        assert_eq!(controller.inner.task_handles.lock().await.len(), 1);
        tokio::time::timeout(Duration::from_secs(5), controller.disconnect())
            .await
            .expect("disconnect must not hang on a stale simulation task");
        assert_eq!(controller.inner.task_handles.lock().await.len(), 0);
    }

    /// Overlapping lifecycle calls on clones must never leave a disconnect
    /// awaiting a simulation task whose token nobody cancelled.
    async fn overlap_rounds(controller: &Controller) {
        let other = controller.clone();
        for _ in 0..25 {
            let (first, second) = tokio::join!(controller.connect(), other.connect());
            first.expect("connect");
            second.expect("connect");
            assert_eq!(controller.inner.task_handles.lock().await.len(), 1);

            let (connected, ()) = tokio::join!(controller.connect(), other.disconnect());
            connected.expect("connect");
            let ((), connected) = tokio::join!(controller.disconnect(), other.connect());
            connected.expect("connect");
            controller.disconnect().await;
            assert_eq!(controller.inner.task_handles.lock().await.len(), 0);
        }
    }

    #[tokio::test]
    async fn overlapping_lifecycle_calls_complete_on_one_thread() {
        let controller = Controller::demo(DemoFleetOptions::default());
        // Boxed: the round future is large and this is a test stack.
        Box::pin(tokio::time::timeout(
            Duration::from_secs(20),
            overlap_rounds(&controller),
        ))
        .await
        .expect("lifecycle calls must not hang");
    }

    /// The exact interleaving that used to hang: a disconnect cancels the
    /// current token and stalls (here on the WebSocket slot, held by the
    /// test) while a connect on a clone installs a fresh simulation; the
    /// disconnect then resumes and would join that live task forever.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn disconnect_never_joins_a_task_a_parallel_connect_installed() {
        let controller = Controller::demo(DemoFleetOptions::default());
        controller.connect().await.expect("connect");

        let stall = controller.inner.ws_handle.lock().await;
        let disconnecting = controller.clone();
        let disconnect = tokio::spawn(async move { disconnecting.disconnect().await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        let connecting = controller.clone();
        let connect = tokio::spawn(async move { connecting.connect().await });
        tokio::time::sleep(Duration::from_millis(100)).await;
        drop(stall);

        tokio::time::timeout(Duration::from_secs(10), async {
            disconnect.await.expect("join");
            connect.await.expect("join").expect("connect");
        })
        .await
        .expect("overlapping disconnect and connect must both finish");
        controller.disconnect().await;
        assert_eq!(controller.inner.task_handles.lock().await.len(), 0);
    }
}
