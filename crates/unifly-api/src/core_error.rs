// ── Core error types ──
//
// User-facing errors from unifly-core. These are NOT API-specific --
// consumers never see HTTP status codes or JSON parse failures directly.
// The `From<crate::error::Error>` impl translates transport-layer errors
// into domain-appropriate variants.

use std::fmt::Write as _;

use thiserror::Error;

/// Lightweight site descriptor returned with `SiteNotFound` to help the
/// caller see what slugs/labels the controller actually exposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteHint {
    pub internal_reference: String,
    pub display_name: String,
}

fn format_site_not_found(name: &str, available: &[SiteHint]) -> String {
    let mut out = format!("Site not found: {name}");
    if !available.is_empty() {
        out.push_str(". Available sites:");
        for hint in available {
            // safe: writing into a String never errors
            let _ = write!(
                &mut out,
                " {} ({})",
                hint.internal_reference, hint.display_name
            );
        }
    }
    out
}

fn format_site_ambiguous(name: &str, matches: &[SiteHint]) -> String {
    let mut out = format!("Site '{name}' is ambiguous; multiple sites match:");
    for hint in matches {
        let _ = write!(
            &mut out,
            " {} ({})",
            hint.internal_reference, hint.display_name
        );
    }
    out.push_str(". Use the slug or UUID instead");
    out
}

/// Unified error type for the core crate.
#[derive(Debug, Error)]
pub enum CoreError {
    // ── Connection errors ────────────────────────────────────────────
    #[error("Cannot connect to controller at {url}: {reason}")]
    ConnectionFailed { url: String, reason: String },

    #[error("Authentication failed: {message}")]
    AuthenticationFailed { message: String },

    #[error("Controller disconnected")]
    ControllerDisconnected,

    #[error("Controller connection timed out after {timeout_secs}s")]
    Timeout { timeout_secs: u64 },

    // ── Data errors ──────────────────────────────────────────────────
    #[error("Device not found: {identifier}")]
    DeviceNotFound { identifier: String },

    #[error("Client not found: {identifier}")]
    ClientNotFound { identifier: String },

    #[error("Network not found: {identifier}")]
    NetworkNotFound { identifier: String },

    #[error("{}", format_site_not_found(.name, .available))]
    SiteNotFound {
        name: String,
        /// Available sites discovered on the controller, used to render a
        /// helpful "did you mean..." list in the error message.
        available: Vec<SiteHint>,
    },

    #[error("{}", format_site_ambiguous(.name, .matches))]
    SiteAmbiguous {
        name: String,
        /// Sites whose `name` (or case-insensitive variants) all matched.
        /// At least two entries; the user must disambiguate by slug or UUID.
        matches: Vec<SiteHint>,
    },

    #[error("Entity not found: {entity_type} with id {identifier}")]
    NotFound {
        entity_type: String,
        identifier: String,
    },

    // ── Operation errors ─────────────────────────────────────────────
    #[error("Operation not supported: {operation} (requires {required})")]
    Unsupported { operation: String, required: String },

    #[error("Operation rejected by controller: {message}")]
    Rejected { message: String },

    #[error("Validation failed: {message}")]
    ValidationFailed { message: String },

    #[error("Operation failed: {message}")]
    OperationFailed { message: String },

    // ── API errors (wrapped, not exposed raw) ────────────────────────
    #[error("API error: {message}")]
    Api {
        message: String,
        /// The API-specific error code (e.g., "api.authentication.missing-credentials").
        code: Option<String>,
        /// HTTP status code (if applicable).
        status: Option<u16>,
    },

    // ── Configuration errors ─────────────────────────────────────────
    #[error("Configuration error: {message}")]
    Config { message: String },

    // ── Internal errors ──────────────────────────────────────────────
    #[error("Internal error: {0}")]
    Internal(String),
}

// ── Conversion from transport-layer errors ───────────────────────────

impl From<crate::error::Error> for CoreError {
    fn from(err: crate::error::Error) -> Self {
        match err {
            crate::error::Error::Authentication { message } => {
                CoreError::AuthenticationFailed { message }
            }
            crate::error::Error::TwoFactorRequired => CoreError::AuthenticationFailed {
                message: "Two-factor authentication token required".into(),
            },
            crate::error::Error::SessionExpired => CoreError::AuthenticationFailed {
                message: "Session expired -- re-authentication required".into(),
            },
            crate::error::Error::InvalidApiKey => CoreError::AuthenticationFailed {
                message: "Invalid API key".into(),
            },
            crate::error::Error::WrongAuthStrategy { expected, got } => {
                CoreError::AuthenticationFailed {
                    message: format!("Wrong auth strategy: expected {expected}, got {got}"),
                }
            }
            crate::error::Error::Transport(ref e) => {
                if e.is_timeout() {
                    CoreError::Timeout { timeout_secs: 0 }
                } else if e.is_connect() {
                    CoreError::ConnectionFailed {
                        url: e
                            .url()
                            .map_or_else(|| "<unknown>".into(), ToString::to_string),
                        reason: e.to_string(),
                    }
                } else if e.status().map(|s| s.as_u16()) == Some(404) {
                    CoreError::NotFound {
                        entity_type: "resource".into(),
                        identifier: e.url().map(|u| u.path().to_string()).unwrap_or_default(),
                    }
                } else {
                    CoreError::Api {
                        message: e.to_string(),
                        code: None,
                        status: e.status().map(|s| s.as_u16()),
                    }
                }
            }
            crate::error::Error::InvalidUrl(e) => CoreError::Config {
                message: format!("Invalid URL: {e}"),
            },
            crate::error::Error::Timeout { timeout_secs } => CoreError::Timeout { timeout_secs },
            crate::error::Error::Tls(msg) => CoreError::ConnectionFailed {
                url: String::new(),
                reason: format!("TLS error: {msg}"),
            },
            crate::error::Error::RateLimited { retry_after_secs } => CoreError::Api {
                message: format!("Rate limited -- retry after {retry_after_secs}s"),
                code: Some("rate_limited".into()),
                status: Some(429),
            },
            crate::error::Error::ConsoleOffline { host_id } => CoreError::ConnectionFailed {
                url: format!("https://api.ui.com (host {host_id})"),
                reason: "cloud console offline or unreachable".into(),
            },
            crate::error::Error::ConsoleAccessDenied { host_id } => {
                CoreError::AuthenticationFailed {
                    message: format!(
                        "Not authorized to access cloud console {host_id} with this API key"
                    ),
                }
            }
            crate::error::Error::Integration {
                message,
                code,
                status,
            } => CoreError::Api {
                message,
                code,
                status: Some(status),
            },
            crate::error::Error::SessionApi { message } => CoreError::Api {
                message,
                code: None,
                status: None,
            },
            crate::error::Error::WebSocketConnect(reason) => CoreError::ConnectionFailed {
                url: String::new(),
                reason: format!("WebSocket connection failed: {reason}"),
            },
            crate::error::Error::WebSocketClosed { code, reason } => CoreError::ConnectionFailed {
                url: String::new(),
                reason: format!("WebSocket closed (code {code}): {reason}"),
            },
            crate::error::Error::Deserialization { message, body: _ } => {
                CoreError::Internal(format!("Deserialization error: {message}"))
            }
            crate::error::Error::UnsupportedOperation(op) => CoreError::Unsupported {
                operation: op.to_string(),
                required: "a newer controller firmware".into(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CoreError, SiteHint};

    #[test]
    fn site_not_found_renders_with_no_candidates() {
        let err = CoreError::SiteNotFound {
            name: "ghost".into(),
            available: Vec::new(),
        };
        assert_eq!(err.to_string(), "Site not found: ghost");
    }

    #[test]
    fn site_ambiguous_lists_matches_and_recommends_slug() {
        let err = CoreError::SiteAmbiguous {
            name: "Home".into(),
            matches: vec![
                SiteHint {
                    internal_reference: "home1".into(),
                    display_name: "Home".into(),
                },
                SiteHint {
                    internal_reference: "home2".into(),
                    display_name: "Home".into(),
                },
            ],
        };
        let rendered = err.to_string();
        assert!(rendered.contains("ambiguous"));
        assert!(rendered.contains("home1 (Home)"));
        assert!(rendered.contains("home2 (Home)"));
        assert!(rendered.contains("slug or UUID"));
    }

    #[test]
    fn site_not_found_lists_available_sites() {
        let err = CoreError::SiteNotFound {
            name: "Default".into(),
            available: vec![
                SiteHint {
                    internal_reference: "default".into(),
                    display_name: "Main Site".into(),
                },
                SiteHint {
                    internal_reference: "guest".into(),
                    display_name: "Guest Network".into(),
                },
            ],
        };
        let rendered = err.to_string();
        assert!(rendered.contains("Site not found: Default"));
        assert!(rendered.contains("default (Main Site)"));
        assert!(rendered.contains("guest (Guest Network)"));
    }
}
