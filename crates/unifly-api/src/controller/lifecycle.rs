use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use reqwest::header::{HeaderMap, HeaderValue};
use secrecy::ExposeSecret;

use crate::config::AuthCredentials;
use crate::core_error::CoreError;
use crate::session::ReauthCredentials;
use crate::session::totp::TotpSource;
use crate::websocket::DeviceSync;
use crate::websocket::{CookieProvider, ReconnectConfig, WebSocketHandle};
use crate::{IntegrationClient, SessionClient};

use super::support::{build_transport, resolve_site, tls_to_transport};
use super::{COMMAND_CHANNEL_SIZE, ConnectionState, Controller, refresh};

/// How often coalesced `device:sync` frames are applied to the store.
const DEVICE_SYNC_FLUSH_INTERVAL: Duration = Duration::from_secs(1);

impl Controller {
    // ── Connection lifecycle ─────────────────────────────────────

    /// Connect to the controller.
    ///
    /// Detects the platform, authenticates, performs an initial data
    /// refresh, and spawns background tasks (periodic refresh, command
    /// processor).
    #[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
    pub async fn connect(&self) -> Result<(), CoreError> {
        self.connect_with_refresh(true).await
    }

    /// Connect to the controller without eagerly loading snapshot data.
    ///
    /// Useful for one-shot commands that issue a direct API call and do
    /// not read from the reactive `DataStore`.
    pub async fn connect_lightweight(&self) -> Result<(), CoreError> {
        self.connect_with_refresh(false).await
    }

    async fn connect_with_refresh(&self, initial_refresh: bool) -> Result<(), CoreError> {
        let _lifecycle = self.inner.lifecycle.lock().await;

        #[cfg(feature = "demo")]
        if let Some(demo) = self.demo_runtime() {
            return self.demo_connect(std::sync::Arc::clone(demo)).await;
        }

        self.inner
            .connection_state
            .send_replace(ConnectionState::Connecting);
        let result = self.establish_connection(initial_refresh).await;
        if result.is_err() {
            self.inner
                .connection_state
                .send_replace(ConnectionState::Failed);
        }
        result
    }

    /// Authenticate, load the initial snapshot, and start the background
    /// tasks. Runs under the lifecycle lock held by the caller.
    #[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
    async fn establish_connection(&self, initial_refresh: bool) -> Result<(), CoreError> {
        // Fresh child token for this connection (supports reconnect).
        let child = self.inner.cancel.child_token();
        *self.inner.cancel_child.lock().await = child.clone();

        let config = &self.inner.config;
        let transport = build_transport(config);

        match &config.auth {
            AuthCredentials::ApiKey(api_key) => {
                // Detect platform so we use the right URL prefix
                let platform = SessionClient::detect_platform(&config.url).await?;
                debug!(?platform, "detected controller platform");

                // Integration API client (preferred)
                let integration = IntegrationClient::from_api_key(
                    config.url.as_str(),
                    api_key,
                    &transport,
                    platform,
                )?;

                // Resolve site UUID from Integration API.
                // A 404 here usually means the controller doesn't expose
                // the Integration API (older or self-hosted UNA without
                // Settings > Integrations). Surface a targeted hint.
                let resolved = resolve_site(&integration, &config.site)
                    .await
                    .map_err(|e| match &e {
                        CoreError::Api {
                            status: Some(404), ..
                        } => {
                            debug!(error = %e, "Integration API returned 404 during site resolution");
                            CoreError::Unsupported {
                                operation: "API-key authentication".into(),
                                required: "a controller with the Integration API \
                                     (Settings > Integrations).\n\
                                     For older UniFi Network Application installs, \
                                     use --username/--password instead"
                                    .into(),
                            }
                        }
                        _ => e,
                    })?;
                debug!(site_id = %resolved.id, slug = %resolved.slug, "resolved Integration API site");

                *self.inner.integration_client.lock().await = Some(Arc::new(integration));
                *self.inner.site_id.lock().await = Some(resolved.id);

                // Also create a session client using the same API key.
                // UniFi OS accepts X-API-KEY on session endpoints, which
                // gives us access to /rest/user (DHCP reservations),
                // /stat/sta (client stats), and health data. Some
                // legacy routes such as /stat/event vary by controller.
                let mut headers = HeaderMap::new();
                let mut key_value =
                    HeaderValue::from_str(api_key.expose_secret()).map_err(|e| {
                        CoreError::from(crate::error::Error::Authentication {
                            message: format!("invalid API key header value: {e}"),
                        })
                    })?;
                key_value.set_sensitive(true);
                headers.insert("X-API-KEY", key_value);
                let legacy_http = transport.build_client_with_headers(headers)?;
                let session = SessionClient::with_client(
                    legacy_http,
                    config.url.clone(),
                    resolved.slug,
                    platform,
                    crate::session::client::SessionAuth::ApiKey,
                );
                *self.inner.session_client.lock().await = Some(Arc::new(session));
            }
            AuthCredentials::Credentials { username, password } => {
                // Session-only auth
                let platform = SessionClient::detect_platform(&config.url).await?;
                debug!(?platform, "detected controller platform");

                let client = SessionClient::new(
                    config.url.clone(),
                    config.site.clone(),
                    platform,
                    &transport,
                )?;

                let cache = build_session_cache(config);
                let totp = connect_totp(config);
                if let Some(ref cache) = cache {
                    client
                        .login_with_cache_and_totp(username, password, totp, cache)
                        .await?;
                } else {
                    client.login_with_totp(username, password, totp).await?;
                }
                debug!("session authentication successful");
                retain_reauth(&client, username, password, config, cache).await;

                *self.inner.session_client.lock().await = Some(Arc::new(client));
            }
            AuthCredentials::Hybrid {
                api_key,
                username,
                password,
            } => {
                // Hybrid: both Integration API (API key) and Session API (session auth)
                let platform = SessionClient::detect_platform(&config.url).await?;
                debug!(?platform, "detected controller platform (hybrid)");

                // Integration API client
                let integration = IntegrationClient::from_api_key(
                    config.url.as_str(),
                    api_key,
                    &transport,
                    platform,
                )?;

                let resolved = resolve_site(&integration, &config.site)
                    .await
                    .map_err(|e| match &e {
                        CoreError::Api {
                            status: Some(404), ..
                        } => {
                            debug!(error = %e, "Integration API returned 404 during site resolution");
                            CoreError::Unsupported {
                                operation: "API-key authentication".into(),
                                required: "a controller with the Integration API \
                                     (Settings > Integrations).\n\
                                     For older UniFi Network Application installs, \
                                     use --username/--password instead"
                                    .into(),
                            }
                        }
                        _ => e,
                    })?;
                debug!(site_id = %resolved.id, slug = %resolved.slug, "resolved Integration API site");

                *self.inner.integration_client.lock().await = Some(Arc::new(integration));
                *self.inner.site_id.lock().await = Some(resolved.id);

                // Session API client — attempt login but degrade gracefully
                // if it fails. The Integration API is the primary surface;
                // Session adds events, stats, and admin ops.
                match SessionClient::new(config.url.clone(), resolved.slug, platform, &transport) {
                    Ok(client) => {
                        let cache = build_session_cache(config);
                        let totp = connect_totp(config);
                        let login_result = if let Some(ref cache) = cache {
                            client
                                .login_with_cache_and_totp(username, password, totp, cache)
                                .await
                        } else {
                            client.login_with_totp(username, password, totp).await
                        };
                        match login_result {
                            Ok(()) => {
                                debug!("session authentication successful (hybrid)");
                                retain_reauth(&client, username, password, config, cache).await;
                                *self.inner.session_client.lock().await = Some(Arc::new(client));
                            }
                            Err(e) => {
                                let msg = format!(
                                    "Session login failed: {e} — events, health stats, and client traffic will be unavailable"
                                );
                                warn!("{msg}");
                                self.inner.warnings.lock().await.push(msg);
                            }
                        }
                    }
                    Err(e) => {
                        let msg = format!("Session client setup failed: {e}");
                        warn!("{msg}");
                        self.inner.warnings.lock().await.push(msg);
                    }
                }
            }
            AuthCredentials::Cloud { api_key, host_id } => {
                let connector_base = format!(
                    "{}/v1/connector/consoles/{}",
                    config.url.as_str().trim_end_matches('/'),
                    host_id,
                );

                let integration = IntegrationClient::from_api_key(
                    &connector_base,
                    api_key,
                    &transport,
                    crate::ControllerPlatform::Cloud,
                )?;

                let resolved = resolve_site(&integration, &config.site).await?;
                debug!(site_id = %resolved.id, slug = %resolved.slug, "resolved cloud Integration API site");

                *self.inner.integration_client.lock().await = Some(Arc::new(integration));
                *self.inner.site_id.lock().await = Some(resolved.id);

                let msg =
                    "Cloud connector mode active: events watch, Wi-Fi observability, admin/session features, and live WebSocket data are unavailable"
                        .to_string();
                self.inner.warnings.lock().await.push(msg);
            }
        }

        if initial_refresh {
            self.full_refresh().await?;
        }

        // Spawn background tasks
        let mut handles = self.inner.task_handles.lock().await;

        // Each connection gets its own channel. Replacing the sender drops
        // any previous one, so a command can only ever reach the processor
        // that serves this connection.
        let (command_tx, command_rx) = mpsc::channel(COMMAND_CHANNEL_SIZE);
        handles.push(tokio::spawn(super::runtime::command_processor_task(
            self.clone(),
            command_rx,
            child.clone(),
        )));
        *self.inner.command_tx.lock().await = Some(command_tx);

        let interval_secs = config.refresh_interval_secs;
        if interval_secs > 0 {
            let ctrl = self.clone();
            let cancel = child.clone();
            handles.push(tokio::spawn(refresh::refresh_task(
                ctrl,
                interval_secs,
                cancel,
            )));
        }

        if config.websocket_enabled {
            self.spawn_websocket(&child, &mut handles).await;
        }

        // Without a WebSocket nothing pushes live device stats, so fall back
        // to a lightweight statistics poll (Integration API only) that runs
        // faster than the full refresh.
        let ws_active = self.inner.ws_handle.lock().await.is_some();
        let poll_secs = config.polling_interval_secs;
        if !ws_active && poll_secs > 0 && (interval_secs == 0 || poll_secs < interval_secs) {
            debug!(poll_secs, "no WebSocket stream; polling device statistics");
            handles.push(tokio::spawn(refresh::stats_poll_task(
                self.clone(),
                poll_secs,
                child.clone(),
            )));
        }

        self.inner
            .connection_state
            .send_replace(ConnectionState::Connected);
        info!("connected to controller");
        Ok(())
    }

    /// Spawn the WebSocket event stream and a bridge task that converts
    /// raw [`UnifiEvent`]s into domain [`Event`]s and broadcasts them.
    ///
    /// Non-fatal on failure — the TUI falls back to polling.
    async fn spawn_websocket(&self, cancel: &CancellationToken, handles: &mut Vec<JoinHandle<()>>) {
        let Some(session) = self.inner.session_client.lock().await.clone() else {
            debug!("no session client — WebSocket unavailable");
            return;
        };

        let platform = session.platform();
        let Some(ws_path_template) = platform.websocket_path() else {
            debug!("platform does not support WebSocket");
            return;
        };

        // Use the resolved slug stored on the session client, not the
        // raw `config.site` -- which may be a display name or UUID that
        // the WebSocket route does not understand.
        let ws_path = ws_path_template.replace("{site}", session.site());
        let base_url = &self.inner.config.url;
        let scheme = if base_url.scheme() == "https" {
            "wss"
        } else {
            "ws"
        };
        let host = base_url.host_str().unwrap_or("localhost");
        let ws_url_str = match base_url.port() {
            Some(port) => format!("{scheme}://{host}:{port}{ws_path}"),
            None => format!("{scheme}://{host}{ws_path}"),
        };
        let ws_url = match url::Url::parse(&ws_url_str) {
            Ok(url) => url,
            Err(error) => {
                warn!(error = %error, url = %ws_url_str, "invalid WebSocket URL");
                return;
            }
        };

        if session.cookie_header().is_none() {
            warn!("no session cookie — WebSocket requires session auth (skipping)");
            return;
        }
        // Read on every (re)connect so a re-login mid-session is picked up.
        let cookie_session = Arc::clone(&session);
        let cookie: CookieProvider = Arc::new(move || cookie_session.cookie_header());

        let ws_tls = tls_to_transport(&self.inner.config.tls);
        let ws_cancel = cancel.child_token();
        let handle = match WebSocketHandle::connect(
            ws_url,
            ReconnectConfig::default(),
            ws_cancel.clone(),
            cookie,
            ws_tls,
        ) {
            Ok(handle) => handle,
            Err(error) => {
                warn!(error = %error, "WebSocket connection failed (non-fatal)");
                return;
            }
        };

        // Bridge task: WS events → domain Events → broadcast channel.
        // Also extracts real-time device stats from `device:sync` messages
        // to feed the dashboard chart without waiting for full_refresh().
        //
        // `device:sync` frames arrive several times a second per device.
        // They are coalesced by MAC (newer fields win, omitted fields are
        // kept) and applied under one mutation batch on a fixed cadence, so
        // subscribers see at most one devices snapshot per
        // `DEVICE_SYNC_FLUSH_INTERVAL` instead of a full snapshot rebuild per
        // frame. `EVT_*` events stay immediate.
        let mut ws_rx = handle.subscribe();
        let event_tx = self.inner.event_tx.clone();
        let store = Arc::clone(&self.inner.store);
        let bridge_cancel = ws_cancel;

        handles.push(tokio::spawn(async move {
            let mut pending_sync: HashMap<String, Box<DeviceSync>> = HashMap::new();
            let mut flush = tokio::time::interval(DEVICE_SYNC_FLUSH_INTERVAL);
            flush.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    biased;
                    () = bridge_cancel.cancelled() => break,
                    _ = flush.tick(), if !pending_sync.is_empty() => {
                        tracing::debug!(devices = pending_sync.len(), "applying coalesced device:sync batch");
                        let batch = store.devices.begin_batch();
                        for sync in pending_sync.values() {
                            super::runtime::apply_device_sync(&store, sync);
                        }
                        drop(batch);
                        pending_sync.clear();
                    }
                    result = ws_rx.recv() => {
                        match result {
                            Ok(ws_event) => {
                                store.mark_ws_event(chrono::Utc::now());

                                if let Some(sync) = ws_event.device_sync.as_ref()
                                    && let Some(mac) = sync.mac.as_deref()
                                {
                                    pending_sync
                                        .entry(mac.to_ascii_lowercase())
                                        .and_modify(|pending| pending.merge_newer(sync))
                                        .or_insert_with(|| sync.clone());
                                }

                                if ws_event.key.starts_with("EVT_") {
                                    let event = crate::model::event::Event::from((*ws_event).clone());
                                    let _ = event_tx.send(Arc::new(event));
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                warn!(skipped, "WS bridge: receiver lagged");
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }
        }));

        *self.inner.ws_handle.lock().await = Some(handle);
        info!("WebSocket event stream spawned (handshake in progress)");
    }

    /// Disconnect from the controller.
    ///
    /// Cancels background tasks, logs out if session-based, and resets
    /// the connection state to [`Disconnected`](ConnectionState::Disconnected).
    pub async fn disconnect(&self) {
        let _lifecycle = self.inner.lifecycle.lock().await;
        // Stop accepting commands before the processor goes away. Commands
        // still queued are dropped with its receiver, which fails them.
        *self.inner.command_tx.lock().await = None;
        self.inner.cancel_child.lock().await.cancel();

        if let Some(handle) = self.inner.ws_handle.lock().await.take() {
            handle.shutdown();
        }

        let mut handles = self.inner.task_handles.lock().await;
        for handle in handles.drain(..) {
            let _ = handle.await;
        }

        let session = self.inner.session_client.lock().await.clone();

        // Skip logout when session caching is active — we want the
        // session cookie to survive for the next CLI invocation.
        let cache_active = build_session_cache(&self.inner.config).is_some();

        if !cache_active
            && matches!(
                self.inner.config.auth,
                AuthCredentials::Credentials { .. } | AuthCredentials::Hybrid { .. }
            )
            && let Some(client) = session
            && let Err(error) = client.logout().await
        {
            warn!(error = %error, "logout failed (non-fatal)");
        }

        *self.inner.session_client.lock().await = None;
        *self.inner.integration_client.lock().await = None;
        *self.inner.site_id.lock().await = None;

        self.inner
            .connection_state
            .send_replace(ConnectionState::Disconnected);
        debug!("disconnected");
    }
}

/// Where the connect-time login gets a TOTP code if the controller asks
/// for one: the configured code, else the provider.
fn connect_totp(config: &crate::config::ControllerConfig) -> TotpSource<'_> {
    TotpSource::new(config.totp_token.as_ref(), config.totp_provider.as_ref())
}

/// Let a cookie session log in again on its own when it expires.
///
/// An MFA re-login needs a code that is current at that moment, so it is
/// handed the TOTP provider when there is one. A session opened with only
/// a fixed TOTP code is not renewed at all: the code is spent, and a
/// re-login that replays it fails on every attempt while presenting the
/// correct password. See [`ReauthCredentials`].
async fn retain_reauth(
    client: &SessionClient,
    username: &str,
    password: &secrecy::SecretString,
    config: &crate::config::ControllerConfig,
    cache: Option<crate::session::session_cache::SessionCache>,
) {
    if config.totp_token.is_some() && config.totp_provider.is_none() {
        info!("MFA login: this session cannot renew itself when the cookie expires");
        return;
    }
    client
        .enable_reauth(ReauthCredentials {
            username: username.to_owned(),
            password: password.clone(),
            totp_provider: config.totp_provider.clone(),
            cache,
        })
        .await;
}

/// Build a `SessionCache` if caching is enabled for this config.
fn build_session_cache(
    config: &crate::config::ControllerConfig,
) -> Option<crate::session::session_cache::SessionCache> {
    if config.no_session_cache {
        return None;
    }
    let name = config.profile_name.as_deref()?;
    crate::session::session_cache::SessionCache::new(name, config.url.as_str())
}
