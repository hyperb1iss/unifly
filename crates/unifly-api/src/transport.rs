// Shared transport configuration for building reqwest::Client instances.
//
// Both Session and Integration clients share TLS, timeout, and cookie
// settings through this module, avoiding duplicated builder logic.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use reqwest::cookie::Jar;

/// TLS verification mode (api-level mirror of core's TlsVerification).
#[derive(Debug, Clone)]
pub enum TlsMode {
    /// Use the system certificate store.
    System,
    /// Use a custom CA certificate from the given PEM file.
    CustomCa(PathBuf),
    /// Accept any certificate (for self-signed controllers).
    DangerAcceptInvalid,
}

/// Shared transport configuration for building HTTP clients.
#[derive(Debug, Clone)]
pub struct TransportConfig {
    pub tls: TlsMode,
    pub timeout: Duration,
    pub cookie_jar: Option<Arc<Jar>>,
}

/// Install `ring` as the process-wide rustls crypto provider.
///
/// reqwest is built with `rustls-no-provider`, so a single provider (ring,
/// enabled on our direct `rustls` dependency) is linked instead of both
/// ring and aws-lc-rs. Installing is idempotent; a second call is a no-op.
pub fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            tls: TlsMode::DangerAcceptInvalid,
            timeout: Duration::from_secs(30),
            cookie_jar: None,
        }
    }
}

impl TransportConfig {
    /// Build a `reqwest::Client` from this config.
    pub fn build_client(&self) -> Result<reqwest::Client, crate::error::Error> {
        ensure_crypto_provider();
        let mut builder = reqwest::Client::builder()
            .timeout(self.timeout)
            .user_agent("unifly/0.1.0");

        match &self.tls {
            TlsMode::System => {}
            TlsMode::CustomCa(path) => {
                let cert_pem = std::fs::read(path).map_err(|e| {
                    crate::error::Error::Tls(format!("failed to read CA cert: {e}"))
                })?;
                let cert = reqwest::Certificate::from_pem(&cert_pem)
                    .map_err(|e| crate::error::Error::Tls(format!("invalid CA cert: {e}")))?;
                builder = builder.add_root_certificate(cert);
            }
            TlsMode::DangerAcceptInvalid => {
                builder = builder.danger_accept_invalid_certs(true);
            }
        }

        if let Some(ref jar) = self.cookie_jar {
            builder = builder.cookie_provider(Arc::clone(jar));
        }

        builder
            .build()
            .map_err(|e| crate::error::Error::Tls(format!("failed to build HTTP client: {e}")))
    }

    /// Build a `reqwest::Client` with additional default headers.
    ///
    /// Used by the Integration API client to inject the `X-API-KEY` header.
    pub fn build_client_with_headers(
        &self,
        headers: reqwest::header::HeaderMap,
    ) -> Result<reqwest::Client, crate::error::Error> {
        ensure_crypto_provider();
        let mut builder = reqwest::Client::builder()
            .timeout(self.timeout)
            .user_agent("unifly/0.1.0")
            .default_headers(headers);

        match &self.tls {
            TlsMode::System => {}
            TlsMode::CustomCa(path) => {
                let cert_pem = std::fs::read(path).map_err(|e| {
                    crate::error::Error::Tls(format!("failed to read CA cert: {e}"))
                })?;
                let cert = reqwest::Certificate::from_pem(&cert_pem)
                    .map_err(|e| crate::error::Error::Tls(format!("invalid CA cert: {e}")))?;
                builder = builder.add_root_certificate(cert);
            }
            TlsMode::DangerAcceptInvalid => {
                builder = builder.danger_accept_invalid_certs(true);
            }
        }

        if let Some(ref jar) = self.cookie_jar {
            builder = builder.cookie_provider(Arc::clone(jar));
        }

        builder
            .build()
            .map_err(|e| crate::error::Error::Tls(format!("failed to build HTTP client: {e}")))
    }

    /// Create a config with a fresh cookie jar (for session auth).
    pub fn with_cookie_jar(mut self) -> Self {
        self.cookie_jar = Some(Arc::new(Jar::default()));
        self
    }
}
