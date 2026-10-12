use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::broadcast;
use tokio_tungstenite::Connector;
use tokio_tungstenite::tungstenite::{self, ClientRequestBuilder};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::error::Error;
use crate::transport::TlsMode;

use super::parser::{UnifiEvent, parse_and_broadcast};
use super::tls::build_tls_connector;

/// Broadcast ring depth. The parser sends every record of an envelope
/// without yielding, so the ring must absorb a whole burst before the bridge
/// task drains it. A slot only holds its event until the last subscriber has
/// read it, which is why [`WebSocketHandle`] keeps the sender rather than an
/// idle receiver that would pin every slot.
const EVENT_CHANNEL_CAPACITY: usize = 1024;

/// Exponential backoff configuration for WebSocket reconnection.
#[derive(Debug, Clone)]
pub struct ReconnectConfig {
    /// Delay before the first reconnection attempt. Default: 1s.
    pub initial_delay: Duration,

    /// Upper bound on backoff delay. Default: 30s.
    pub max_delay: Duration,

    /// Maximum reconnection attempts before giving up.
    /// `None` means retry forever.
    pub max_retries: Option<u32>,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            max_retries: None,
        }
    }
}

/// Supplies the `Cookie` header for each connection attempt.
///
/// Read fresh on every (re)connect so that a session re-login performed
/// elsewhere (see `SessionClient::enable_reauth`) is picked up without
/// tearing the stream down by hand.
pub type CookieProvider = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// Handle to a running WebSocket event stream.
///
/// Cheaply cloneable via the inner broadcast sender. Drop all handles
/// and call [`shutdown`](Self::shutdown) to tear down the background task.
pub struct WebSocketHandle {
    /// Sender side only: holding a `Receiver` here would pin every slot in
    /// the ring for a consumer that never reads.
    event_tx: broadcast::Sender<Arc<UnifiEvent>>,
    cancel: CancellationToken,
}

impl WebSocketHandle {
    /// Connect to the controller WebSocket and spawn the reconnection loop.
    ///
    /// Returns immediately once the background task is spawned.
    /// The first connection attempt happens asynchronously -- subscribe to
    /// the event receiver to start consuming events.
    pub fn connect(
        ws_url: Url,
        reconnect: ReconnectConfig,
        cancel: CancellationToken,
        cookie: CookieProvider,
        tls_mode: TlsMode,
    ) -> Result<Self, Error> {
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);

        let task_cancel = cancel.clone();
        let loop_tx = event_tx.clone();
        tokio::spawn(async move {
            ws_loop(ws_url, loop_tx, reconnect, task_cancel, cookie, tls_mode).await;
        });

        Ok(Self { event_tx, cancel })
    }

    /// Get a new broadcast receiver for the event stream.
    ///
    /// Multiple consumers can subscribe concurrently. If a consumer falls
    /// behind, it receives [`broadcast::error::RecvError::Lagged`].
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<UnifiEvent>> {
        self.event_tx.subscribe()
    }

    /// Signal the background task to shut down gracefully.
    pub fn shutdown(&self) {
        self.cancel.cancel();
    }
}

async fn ws_loop(
    ws_url: Url,
    event_tx: broadcast::Sender<Arc<UnifiEvent>>,
    reconnect: ReconnectConfig,
    cancel: CancellationToken,
    cookie: CookieProvider,
    tls_mode: TlsMode,
) {
    let mut attempt: u32 = 0;

    loop {
        let current_cookie = cookie();
        tokio::select! {
            biased;
            () = cancel.cancelled() => break,
            result = connect_and_read(&ws_url, &event_tx, &cancel, current_cookie.as_deref(), &tls_mode) => {
                match result {
                    Ok(()) => {
                        // A server-side close (e.g. the session cookie lapsed)
                        // used to reconnect immediately, which spins when the
                        // controller keeps accepting and closing. Pause for the
                        // initial backoff instead; the cookie provider hands us
                        // a fresh cookie once the session re-authenticates.
                        tracing::info!("WebSocket disconnected cleanly, reconnecting");
                        attempt = 0;
                        tokio::select! {
                            biased;
                            () = cancel.cancelled() => break,
                            () = tokio::time::sleep(reconnect.initial_delay) => {}
                        }
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, attempt, "WebSocket error");

                        if let Some(max) = reconnect.max_retries
                            && attempt >= max {
                                tracing::error!(
                                    max_retries = max,
                                    "WebSocket reconnection limit reached, giving up"
                                );
                                break;
                            }

                        let delay = calculate_backoff(attempt, &reconnect);
                        let delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX);
                        tracing::info!(delay_ms, attempt, "Waiting before reconnect");

                        tokio::select! {
                            biased;
                            () = cancel.cancelled() => break,
                            () = tokio::time::sleep(delay) => {}
                        }

                        attempt += 1;
                    }
                }
            }
        }
    }

    #[allow(unreachable_code)]
    {
        tracing::debug!("WebSocket loop exiting");
    }
}

async fn connect_and_read(
    url: &Url,
    event_tx: &broadcast::Sender<Arc<UnifiEvent>>,
    cancel: &CancellationToken,
    cookie: Option<&str>,
    tls_mode: &TlsMode,
) -> Result<(), Error> {
    tracing::info!(url = %url, "Connecting to WebSocket");

    let uri: tungstenite::http::Uri =
        url.as_str()
            .parse()
            .map_err(|error: tungstenite::http::uri::InvalidUri| {
                Error::WebSocketConnect(error.to_string())
            })?;

    let mut request = ClientRequestBuilder::new(uri);
    if let Some(cookie_val) = cookie {
        request = request.with_header("Cookie", cookie_val);
    }

    let connector = if url.scheme() == "wss" {
        build_tls_connector(tls_mode)?
    } else {
        Some(Connector::Plain)
    };

    let (ws_stream, _response) =
        tokio_tungstenite::connect_async_tls_with_config(request, None, false, connector)
            .await
            .map_err(|error| Error::WebSocketConnect(error.to_string()))?;

    tracing::info!("WebSocket connected");

    let (_write, mut read) = ws_stream.split();

    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => return Ok(()),
            frame = read.next() => {
                match frame {
                    Some(Ok(tungstenite::Message::Text(text))) => {
                        parse_and_broadcast(&text, event_tx);
                    }
                    Some(Ok(tungstenite::Message::Ping(_))) => {
                        tracing::trace!("WebSocket ping");
                    }
                    Some(Ok(tungstenite::Message::Close(frame))) => {
                        if let Some(ref cf) = frame {
                            tracing::info!(
                                code = %cf.code,
                                reason = %cf.reason,
                                "WebSocket close frame received"
                            );
                        } else {
                            tracing::info!("WebSocket close frame received (no payload)");
                        }
                        return Ok(());
                    }
                    Some(Err(error)) => {
                        return Err(Error::WebSocketConnect(error.to_string()));
                    }
                    None => {
                        tracing::info!("WebSocket stream ended");
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
    }
}

fn calculate_backoff(attempt: u32, config: &ReconnectConfig) -> Duration {
    let base = config.initial_delay.as_secs_f64()
        * 2.0_f64.powi(i32::try_from(attempt).unwrap_or(i32::MAX));
    let capped = base.min(config.max_delay.as_secs_f64());

    let jitter_factor = 1.0 + 0.25 * ((f64::from(attempt) * 7.3).sin());
    let with_jitter = (capped * jitter_factor).max(0.0);

    Duration::from_secs_f64(with_jitter)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn default_reconnect_config() {
        let config = ReconnectConfig::default();
        assert_eq!(config.initial_delay, Duration::from_secs(1));
        assert_eq!(config.max_delay, Duration::from_secs(30));
        assert!(config.max_retries.is_none());
    }

    #[test]
    fn backoff_increases_exponentially() {
        let config = ReconnectConfig::default();

        let d0 = calculate_backoff(0, &config);
        let d1 = calculate_backoff(1, &config);
        let d2 = calculate_backoff(2, &config);

        assert!(d1 > d0, "d1 ({d1:?}) should be greater than d0 ({d0:?})");
        assert!(d2 > d1, "d2 ({d2:?}) should be greater than d1 ({d1:?})");
    }

    #[test]
    fn backoff_caps_at_max_delay() {
        let config = ReconnectConfig {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(10),
            max_retries: None,
        };

        let d10 = calculate_backoff(10, &config);
        assert!(
            d10 <= Duration::from_secs(13),
            "delay at attempt 10 ({d10:?}) should be capped near max_delay"
        );
    }
}
