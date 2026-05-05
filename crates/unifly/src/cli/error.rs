//! CLI error types with miette diagnostics.
//!
//! Maps `CoreError` variants into user-facing errors with actionable help text.
// thiserror + miette derive macros generate Display impls where struct fields
// appear as "assigned but never read" — the fields ARE used in format strings
// but clippy can't see through the macro expansion.
#![allow(unused_assignments)]

use miette::Diagnostic;
use thiserror::Error;

use unifly_api::CoreError;

/// Exit codes per the CLI spec.
pub mod exit_code {
    pub const GENERAL: i32 = 1;
    pub const USAGE: i32 = 2;
    pub const AUTH: i32 = 3;
    pub const NOT_FOUND: i32 = 4;
    pub const PERMISSION: i32 = 5;
    pub const CONFLICT: i32 = 6;
    pub const CONNECTION: i32 = 7;
    pub const TIMEOUT: i32 = 8;
}

#[derive(Debug, Error, Diagnostic)]
#[allow(dead_code)]
pub enum CliError {
    // ── Connection ───────────────────────────────────────────────────
    #[error("Could not connect to controller at {url}")]
    #[diagnostic(
        code(unifi::connection_failed),
        help(
            "Check that the controller is running and accessible.\n\
             URL: {url}\n\
             Try: unifly system info --insecure"
        )
    )]
    ConnectionFailed {
        url: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("TLS certificate verification failed for {url}")]
    #[diagnostic(
        code(unifi::tls_error),
        help(
            "The controller is using a self-signed certificate.\n\
             Use --insecure (-k) to accept it, or configure ca_cert in your profile."
        )
    )]
    TlsError { url: String },

    // ── Authentication ───────────────────────────────────────────────
    #[error("Authentication failed: {message}")]
    #[diagnostic(
        code(unifi::auth_failed),
        help(
            "Verify your API key or credentials.\n\
             For local controller API keys: check Settings > Integrations on your controller.\n\
             For Site Manager cloud profiles: verify the API key can see the console and your UI account has UniFi Network admin access.\n\
             For local username/password profiles: run unifly config set-password --profile {profile}\n\
             To re-run cloud onboarding: unifly config cloud-setup"
        )
    )]
    AuthFailed { profile: String, message: String },

    #[error("No credentials configured for profile '{profile}'")]
    #[diagnostic(
        code(unifi::no_credentials),
        help(
            "Configure credentials with: unifly config init (local)\n\
             Or: unifly config cloud-setup (Site Manager)\n\
             Or set UNIFI_API_KEY environment variable."
        )
    )]
    NoCredentials { profile: String },

    // ── Resources ────────────────────────────────────────────────────
    #[error("{resource_type} '{identifier}' not found")]
    #[diagnostic(
        code(unifi::not_found),
        help("Run: unifly {list_command} to see available {resource_type}s")
    )]
    NotFound {
        resource_type: String,
        identifier: String,
        list_command: String,
    },

    #[error("{resource_type} '{identifier}' already exists")]
    #[diagnostic(code(unifi::conflict))]
    Conflict {
        resource_type: String,
        identifier: String,
    },

    #[error("Site '{name}' not found on this controller")]
    #[diagnostic(
        code(unifi::site_not_found),
        help(
            "Available sites: {available}\n\
             Set `site = \"<slug>\"` in your profile to one of the slugs above (the part before the parens),\n\
             or pass --site <slug>. The display name and slug are not always the same."
        )
    )]
    SiteNotFound { name: String, available: String },

    #[error("Site '{name}' is ambiguous on this controller")]
    #[diagnostic(
        code(unifi::site_ambiguous),
        help(
            "Multiple sites match: {matches}\n\
             Use the slug or UUID (the part before the parens) to disambiguate."
        )
    )]
    SiteAmbiguous { name: String, matches: String },

    // ── API ──────────────────────────────────────────────────────────
    #[error("API error{}: {message}", if code.is_empty() { String::new() } else { format!(" ({code})") })]
    #[diagnostic(code(unifi::api_error))]
    ApiError {
        code: String,
        message: String,
        request_id: Option<String>,
    },

    // ── Unsupported ──────────────────────────────────────────────────
    #[error("Operation '{operation}' is not supported with the current auth mode")]
    #[diagnostic(
        code(unifi::unsupported),
        help(
            "This command requires {required}.\n\
             Configure the appropriate credentials with: unifly config init or unifly config cloud-setup"
        )
    )]
    Unsupported { operation: String, required: String },

    #[error("'{feature}' is not yet implemented")]
    #[diagnostic(
        code(unifi::not_implemented),
        help("This feature requires direct Session API access, planned for a future release.")
    )]
    NotYetImplemented { feature: String },

    // ── Validation ───────────────────────────────────────────────────
    #[error("Invalid value for {field}: {reason}")]
    #[diagnostic(code(unifi::validation))]
    Validation { field: String, reason: String },

    // ── Configuration ────────────────────────────────────────────────
    #[error("Profile '{name}' not found in configuration")]
    #[diagnostic(
        code(unifi::profile_not_found),
        help(
            "Available profiles: {available}\n\
             Create one with: unifly config init or unifly config cloud-setup"
        )
    )]
    ProfileNotFound { name: String, available: String },

    #[error("Configuration file not found")]
    #[diagnostic(
        code(unifi::no_config),
        help(
            "Create one with: unifly config init (local)\n\
             Or: unifly config cloud-setup (Site Manager)\n\
             Expected at: {path}"
        )
    )]
    NoConfig { path: String },

    #[error(transparent)]
    #[diagnostic(code(unifi::config))]
    Config(Box<figment::Error>),

    // ── Interactive ──────────────────────────────────────────────────
    #[error("Destructive operation '{action}' requires confirmation")]
    #[diagnostic(
        code(unifi::confirmation_required),
        help("Use --yes (-y) to skip confirmation in non-interactive contexts.")
    )]
    NonInteractiveRequiresYes { action: String },

    // ── Timeout ──────────────────────────────────────────────────────
    #[error("Request timed out after {seconds}s")]
    #[diagnostic(
        code(unifi::timeout),
        help("Increase timeout with --timeout or check controller responsiveness.")
    )]
    Timeout { seconds: u64 },

    // ── IO / Serialization ────────────────────────────────────────────
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("Invalid JSON payload: {0}")]
    #[diagnostic(code(unifi::json), help("Check the JSON file contents and try again."))]
    Json(#[from] serde_json::Error),

    // ── Config crate errors ──────────────────────────────────────────
    #[error(transparent)]
    #[diagnostic(code(unifi::config_error))]
    ConfigError(#[from] crate::config::ConfigError),
}

impl From<figment::Error> for CliError {
    fn from(err: figment::Error) -> Self {
        Self::Config(Box::new(err))
    }
}

impl CliError {
    /// Map this error to an exit code for process termination.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::ConnectionFailed { .. } | Self::TlsError { .. } => exit_code::CONNECTION,
            Self::AuthFailed { .. } | Self::NoCredentials { .. } => exit_code::AUTH,
            Self::NotFound { .. } | Self::SiteNotFound { .. } => exit_code::NOT_FOUND,
            Self::Conflict { .. } => exit_code::CONFLICT,
            Self::Timeout { .. } => exit_code::TIMEOUT,
            Self::Validation { .. }
            | Self::NonInteractiveRequiresYes { .. }
            | Self::SiteAmbiguous { .. } => exit_code::USAGE,
            Self::Unsupported { .. } | Self::NotYetImplemented { .. } => exit_code::PERMISSION,
            _ => exit_code::GENERAL,
        }
    }
}

// ── CoreError → CliError mapping ─────────────────────────────────────

impl From<CoreError> for CliError {
    fn from(err: CoreError) -> Self {
        match err {
            CoreError::ConnectionFailed { url, reason } => CliError::ConnectionFailed {
                url,
                source: reason.into(),
            },

            CoreError::AuthenticationFailed { message } => CliError::AuthFailed {
                profile: "current".into(),
                message,
            },

            CoreError::ControllerDisconnected => CliError::ConnectionFailed {
                url: "(disconnected)".into(),
                source: "Controller connection was lost".into(),
            },

            CoreError::Timeout { timeout_secs } => CliError::Timeout {
                seconds: timeout_secs,
            },

            CoreError::DeviceNotFound { identifier } => CliError::NotFound {
                resource_type: "device".into(),
                identifier,
                list_command: "devices list".into(),
            },

            CoreError::ClientNotFound { identifier } => CliError::NotFound {
                resource_type: "client".into(),
                identifier,
                list_command: "clients list".into(),
            },

            CoreError::SiteNotFound { name, available } => CliError::SiteNotFound {
                name,
                available: render_site_hints(&available),
            },

            CoreError::SiteAmbiguous { name, matches } => CliError::SiteAmbiguous {
                name,
                matches: render_site_hints(&matches),
            },

            CoreError::NetworkNotFound { identifier } => CliError::NotFound {
                resource_type: "network".into(),
                identifier,
                list_command: "networks list".into(),
            },

            CoreError::NotFound {
                entity_type,
                identifier,
            } => CliError::NotFound {
                list_command: format!("{entity_type}s list"),
                resource_type: entity_type,
                identifier,
            },

            CoreError::Unsupported {
                operation,
                required,
            } => CliError::Unsupported {
                operation,
                required,
            },

            CoreError::ValidationFailed { message } => CliError::Validation {
                field: "input".into(),
                reason: message,
            },

            CoreError::Rejected { message } => CliError::ApiError {
                code: "rejected".into(),
                message,
                request_id: None,
            },

            CoreError::OperationFailed { message } => CliError::ApiError {
                code: "operation_failed".into(),
                message,
                request_id: None,
            },

            CoreError::Api {
                message,
                code,
                status: _,
            } => CliError::ApiError {
                code: code.unwrap_or_default(),
                message,
                request_id: None,
            },

            CoreError::Config { message } => {
                if message.contains("profile") {
                    CliError::ProfileNotFound {
                        name: message,
                        available: String::new(),
                    }
                } else {
                    CliError::NoConfig {
                        path: String::new(),
                    }
                }
            }

            CoreError::Internal(message) => CliError::ApiError {
                code: "internal".into(),
                message,
                request_id: None,
            },
        }
    }
}

fn render_site_hints(hints: &[unifly_api::SiteHint]) -> String {
    if hints.is_empty() {
        return "(none discovered -- run `unifly sites list` to verify connectivity)".into();
    }
    hints
        .iter()
        .map(|h| format!("{} ({})", h.internal_reference, h.display_name))
        .collect::<Vec<_>>()
        .join(", ")
}
