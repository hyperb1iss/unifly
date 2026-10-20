use thiserror::Error;

/// Top-level error type for the `unifly-api` crate.
///
/// Covers every failure mode across all API surfaces:
/// authentication, transport, Integration API, Session API, WebSocket, and cloud.
/// `unifly-core` maps these into user-facing diagnostics.
#[derive(Debug, Error)]
pub enum Error {
    // ── Authentication ──────────────────────────────────────────────
    /// Login failed (wrong credentials, account locked, etc.)
    #[error("Authentication failed: {message}")]
    Authentication { message: String },

    /// 2FA token required but not provided.
    #[error("Two-factor authentication token required")]
    TwoFactorRequired,

    /// Session has expired (cookie expired or revoked).
    #[error("Session expired -- re-authentication required")]
    SessionExpired,

    /// Invalid API key (rejected by controller).
    #[error("Invalid API key")]
    InvalidApiKey,

    /// Wrong credential type for the requested operation.
    #[error("Wrong auth strategy: expected {expected}, got {got}")]
    WrongAuthStrategy { expected: String, got: String },

    // ── Transport ───────────────────────────────────────────────────
    /// HTTP transport error (connection refused, DNS failure, etc.)
    #[error("HTTP transport error: {0}")]
    Transport(#[from] reqwest::Error),

    /// URL parsing error.
    #[error("Invalid URL: {0}")]
    InvalidUrl(#[from] url::ParseError),

    /// Request timed out.
    #[error("Request timed out after {timeout_secs}s")]
    Timeout { timeout_secs: u64 },

    /// TLS handshake or certificate error.
    #[error("TLS error: {0}")]
    Tls(String),

    // ── Cloud ───────────────────────────────────────────────────────
    /// Rate limited by the cloud API. Includes retry-after in seconds.
    #[error("Rate limited -- retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },

    /// Cloud console is offline or unreachable through the connector.
    #[error("Cloud console offline or unreachable: {host_id}")]
    ConsoleOffline { host_id: String },

    /// API key cannot access the requested cloud console.
    #[error("Not authorized to access cloud console: {host_id}")]
    ConsoleAccessDenied { host_id: String },

    // ── Integration API ─────────────────────────────────────────────
    /// Structured error from the Integration API.
    #[error("Integration API error (HTTP {status}): {message}")]
    Integration {
        message: String,
        code: Option<String>,
        status: u16,
    },

    // ── Session API ──────────────────────────────────────────────────
    /// Error from the session API (parsed from the `{meta: {rc, msg}}` envelope).
    #[error("Session API error: {message}")]
    SessionApi { message: String },

    // ── WebSocket ───────────────────────────────────────────────────
    /// WebSocket connection failed.
    #[error("WebSocket connection failed: {0}")]
    WebSocketConnect(String),

    /// WebSocket closed unexpectedly.
    #[error("WebSocket closed (code {code}): {reason}")]
    WebSocketClosed { code: u16, reason: String },

    // ── Data ────────────────────────────────────────────────────────
    /// JSON deserialization failed, with the raw body for debugging.
    #[error("Deserialization error: {message}")]
    Deserialization { message: String, body: String },

    // ── Platform ────────────────────────────────────────────────────
    /// Operation not supported on this controller platform.
    #[error("Unsupported operation: {0}")]
    UnsupportedOperation(&'static str),
}

/// First ~200 bytes of a response body for error messages, never panicking
/// on a multi-byte character boundary.
pub(crate) fn body_preview(body: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(&body[..body.len().min(200)])
}

impl Error {
    /// Returns `true` if this error indicates auth has expired
    /// and re-authentication might resolve it.
    pub fn is_auth_expired(&self) -> bool {
        matches!(self, Self::Authentication { .. } | Self::SessionExpired)
    }

    /// Returns `true` if this is a transient error worth retrying.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Transport(e) => e.is_timeout() || e.is_connect(),
            Self::Timeout { .. }
            | Self::RateLimited { .. }
            | Self::ConsoleOffline { .. }
            | Self::WebSocketConnect(_) => true,
            _ => false,
        }
    }

    /// Returns `true` if this is a "not found" error.
    pub fn is_not_found(&self) -> bool {
        match self {
            Self::Transport(e) => e.status() == Some(reqwest::StatusCode::NOT_FOUND),
            Self::Integration { status: 404, .. } => true,
            Self::SessionApi { message } => {
                message.starts_with("HTTP 404") || message.contains("api.err.NotFound")
            }
            _ => false,
        }
    }

    /// Extract the API error code, if available.
    pub fn api_error_code(&self) -> Option<&str> {
        match self {
            Self::Integration { code, .. } => code.as_deref(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn session_api_http_404_counts_as_not_found() {
        let error = Error::SessionApi {
            message: "HTTP 404 Not Found: {\"meta\":{\"rc\":\"error\",\"msg\":\"api.err.NotFound\"},\"data\":[]}".into(),
        };

        assert!(error.is_not_found());
    }

    #[test]
    fn session_api_not_found_code_counts_as_not_found() {
        let error = Error::SessionApi {
            message: "api.err.NotFound".into(),
        };

        assert!(error.is_not_found());
    }
}
