// ── Runtime connection configuration ──
//
// These types describe *how* to connect to a UniFi controller.
// They carry credential data and connection tuning, but never touch disk.
// The CLI/TUI constructs a `ControllerConfig` and hands it in.

use secrecy::SecretString;
use url::Url;

/// How to authenticate with a controller.
///
/// Named `AuthCredentials` (not `AuthMethod`) to avoid collision with
/// `unifly_api::AuthStrategy` which is a zero-data marker enum.
/// This type carries the actual credential data.
#[derive(Debug, Clone)]
pub enum AuthCredentials {
    /// Integration API key (preferred).
    ApiKey(SecretString),
    /// Legacy cookie-based auth.
    Credentials {
        username: String,
        password: SecretString,
    },
    /// Hybrid: API key for Integration API + credentials for Legacy API.
    ///
    /// Gives full access to both APIs in a single session — Integration API
    /// for CRUD and reads, Legacy API for stats, events, alarms, and admin.
    Hybrid {
        api_key: SecretString,
        username: String,
        password: SecretString,
    },
    /// Cloud connector via api.ui.com.
    Cloud {
        api_key: SecretString,
        host_id: String,
    },
}

/// TLS verification strategy.
#[derive(Debug, Clone, Default)]
pub enum TlsVerification {
    /// System CA store (strict).
    SystemDefaults,
    /// Custom CA certificate file.
    CustomCa(std::path::PathBuf),
    /// Skip verification (self-signed certs). Default for local controllers.
    #[default]
    DangerAcceptInvalid,
}

impl PartialEq for TlsVerification {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::CustomCa(a), Self::CustomCa(b)) => a == b,
            (Self::SystemDefaults, Self::SystemDefaults)
            | (Self::DangerAcceptInvalid, Self::DangerAcceptInvalid) => true,
            _ => false,
        }
    }
}

impl Eq for TlsVerification {}

/// Configuration for connecting to a single controller.
///
/// Built by CLI/TUI, passed to `Controller` -- core never reads config files.
#[derive(Debug, Clone)]
pub struct ControllerConfig {
    /// Controller URL (e.g., `https://192.168.1.1`).
    pub url: Url,
    /// Authentication method and credentials.
    pub auth: AuthCredentials,
    /// Site to operate on (defaults to "default").
    pub site: String,
    /// TLS verification strategy.
    pub tls: TlsVerification,
    /// Request timeout.
    pub timeout: std::time::Duration,
    /// How often to perform a full refresh (seconds). 0 = never.
    pub refresh_interval_secs: u64,
    /// Enable WebSocket event stream.
    pub websocket_enabled: bool,
    /// Polling interval when WebSocket is unavailable (seconds).
    pub polling_interval_secs: u64,
    /// Optional TOTP token for MFA-enabled controllers.
    pub totp_token: Option<SecretString>,
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            url: "https://192.168.1.1:8443"
                .parse()
                .expect("default controller URL is valid"),
            auth: AuthCredentials::Credentials {
                username: "admin".into(),
                password: SecretString::from(String::new()),
            },
            site: "default".into(),
            tls: TlsVerification::default(),
            timeout: std::time::Duration::from_secs(30),
            refresh_interval_secs: 300,
            websocket_enabled: true,
            polling_interval_secs: 10,
            totp_token: None,
        }
    }
}
