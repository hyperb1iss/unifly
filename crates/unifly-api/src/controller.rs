// ── Controller abstraction ──
//
// Full lifecycle management for a UniFi controller connection.
// Handles authentication, background refresh, command routing,
// and reactive data streaming through the DataStore.

use std::sync::Arc;

use crate::command::{Command, CommandEnvelope, CommandResult};
use crate::config::ControllerConfig;
use crate::core_error::CoreError;
use crate::model::Event;
use crate::store::DataStore;
use crate::websocket::WebSocketHandle;
use crate::{IntegrationClient, SessionClient};
use tokio::sync::{Mutex, broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

mod commands;
mod lifecycle;
mod payloads;
mod ports;
mod query;
mod refresh;
mod runtime;
mod session_queries;
mod subscriptions;
mod support;

pub use ports::{ApplyPortsSummary, PortProfileUpdate};

use self::support::{
    client_mac, device_mac, integration_client_context, integration_site_context,
    require_integration, require_session, require_uuid,
};

const COMMAND_CHANNEL_SIZE: usize = 64;
const EVENT_CHANNEL_SIZE: usize = 256;
const REFRESH_DETAIL_CONCURRENCY: usize = 16;

// ── ConnectionState ──────────────────────────────────────────────

/// Connection state observable by consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting { attempt: u32 },
    Failed,
}

// ── Controller ───────────────────────────────────────────────────

/// The main entry point for consumers.
///
/// Cheaply cloneable via `Arc<ControllerInner>`. Manages the full
/// connection lifecycle: authentication, background data refresh,
/// command routing, and reactive entity streaming.
#[derive(Clone)]
pub struct Controller {
    inner: Arc<ControllerInner>,
}

struct ControllerInner {
    config: ControllerConfig,
    store: Arc<DataStore>,
    connection_state: watch::Sender<ConnectionState>,
    event_tx: broadcast::Sender<Arc<Event>>,
    command_tx: Mutex<mpsc::Sender<CommandEnvelope>>,
    command_rx: Mutex<Option<mpsc::Receiver<CommandEnvelope>>>,
    cancel: CancellationToken,
    /// Child token for the current connection — cancelled on disconnect,
    /// replaced on reconnect (avoids permanent cancellation).
    cancel_child: Mutex<CancellationToken>,
    session_client: Mutex<Option<Arc<SessionClient>>>,
    integration_client: Mutex<Option<Arc<IntegrationClient>>>,
    /// Resolved Integration API site UUID (populated on connect).
    site_id: Mutex<Option<uuid::Uuid>>,
    /// WebSocket event stream handle (populated on connect if enabled).
    ws_handle: Mutex<Option<WebSocketHandle>>,
    task_handles: Mutex<Vec<JoinHandle<()>>>,
    /// Warnings accumulated during connect (e.g. Session auth failure in Hybrid mode).
    warnings: Mutex<Vec<String>>,
}

impl Controller {
    /// Create a new Controller from configuration. Does NOT connect --
    /// call [`connect()`](Self::connect) to authenticate and start background tasks.
    pub fn new(config: ControllerConfig) -> Self {
        let store = Arc::new(DataStore::new());
        let (connection_state, _) = watch::channel(ConnectionState::Disconnected);
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_SIZE);
        let (command_tx, command_rx) = mpsc::channel(COMMAND_CHANNEL_SIZE);
        let cancel = CancellationToken::new();
        let cancel_child = cancel.child_token();

        Self {
            inner: Arc::new(ControllerInner {
                config,
                store,
                connection_state,
                event_tx,
                command_tx: Mutex::new(command_tx),
                command_rx: Mutex::new(Some(command_rx)),
                cancel,
                cancel_child: Mutex::new(cancel_child),
                session_client: Mutex::new(None),
                integration_client: Mutex::new(None),
                warnings: Mutex::new(Vec::new()),
                site_id: Mutex::new(None),
                ws_handle: Mutex::new(None),
                task_handles: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Access the controller configuration.
    pub fn config(&self) -> &ControllerConfig {
        &self.inner.config
    }

    /// Access the underlying DataStore.
    pub fn store(&self) -> &Arc<DataStore> {
        &self.inner.store
    }

    // ── Command execution ────────────────────────────────────────

    /// Execute a command against the controller.
    ///
    /// Sends the command through the internal channel to the command
    /// processor task and awaits the result.
    pub async fn execute(&self, cmd: Command) -> Result<CommandResult, CoreError> {
        let (tx, rx) = tokio::sync::oneshot::channel();

        let command_tx = self.inner.command_tx.lock().await.clone();

        command_tx
            .send(CommandEnvelope {
                command: cmd,
                response_tx: tx,
            })
            .await
            .map_err(|_| CoreError::ControllerDisconnected)?;

        rx.await.map_err(|_| CoreError::ControllerDisconnected)?
    }

    // ── One-shot convenience ─────────────────────────────────────

    /// One-shot: connect, run closure, disconnect.
    ///
    /// Optimized for CLI: disables WebSocket and periodic refresh since
    /// we only need a single request-response cycle.
    pub async fn oneshot<F, Fut, T>(config: ControllerConfig, f: F) -> Result<T, CoreError>
    where
        F: FnOnce(Controller) -> Fut,
        Fut: std::future::Future<Output = Result<T, CoreError>>,
    {
        let mut cfg = config;
        cfg.websocket_enabled = false;
        cfg.refresh_interval_secs = 0;

        let controller = Controller::new(cfg);
        controller.connect().await?;
        let result = f(controller.clone()).await;
        controller.disconnect().await;
        result
    }
}
