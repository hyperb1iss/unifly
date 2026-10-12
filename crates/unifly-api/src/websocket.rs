//! WebSocket event stream with auto-reconnect.
//!
//! Connects to a UniFi controller's session WebSocket endpoint and streams
//! parsed events through a [`tokio::sync::broadcast`] channel. Handles
//! reconnection with exponential backoff + jitter automatically.
//!
//! # Example
//!
//! ```rust,ignore
//! use unifly_api::websocket::{WebSocketHandle, ReconnectConfig};
//! use unifly_api::transport::TlsMode;
//! use tokio_util::sync::CancellationToken;
//! use url::Url;
//!
//! let cancel = CancellationToken::new();
//! let ws_url = Url::parse("wss://192.168.1.1/proxy/network/wss/s/default/events")?;
//!
//! let handle = WebSocketHandle::connect(
//!     ws_url, ReconnectConfig::default(), cancel.clone(), None,
//!     TlsMode::DangerAcceptInvalid,
//! )?;
//! let mut rx = handle.subscribe();
//!
//! while let Ok(event) = rx.recv().await {
//!     println!("{}: {}", event.key, event.message.as_deref().unwrap_or(""));
//! }
//!
//! handle.shutdown();
//! ```

mod parser;
mod runtime;
mod tls;

pub use parser::{
    DeviceSync, DeviceSyncSysStats, DeviceSyncUplink, DeviceSyncWan, NumOrStr, UnifiEvent,
    is_device_sync,
};
pub use runtime::{CookieProvider, ReconnectConfig, WebSocketHandle};
