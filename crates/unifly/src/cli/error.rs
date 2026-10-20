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
    #[diagnostic(code(unifi::auth_failed))]
    AuthFailed {
        message: String,
        /// Built by [`CliError::auth_failed`], which knows the profile.
        #[help]
        help: String,
    },

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
        help("See what exists with: unifly {list_command}")
    )]
    NotFound {
        resource_type: String,
        identifier: String,
        list_command: String,
    },

    /// A missing record that no list command enumerates.
    #[error("{resource_type} '{identifier}' not found")]
    #[diagnostic(code(unifi::not_found))]
    NotFoundUnlisted {
        resource_type: String,
        identifier: String,
    },

    #[error("{resource_type} '{identifier}' is ambiguous ({count} matches)")]
    #[diagnostic(
        code(unifi::ambiguous),
        help(
            "Candidates:\n{candidates}\n\
             Pass the ID of the one you mean, or run: unifly {list_command}"
        )
    )]
    Ambiguous {
        resource_type: String,
        identifier: String,
        count: usize,
        candidates: String,
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

    #[error("`unifly {command}` is not available in demo mode")]
    #[diagnostic(
        code(unifi::demo_unavailable),
        help(
            "The demo fleet is synthetic and offline and answers only these read-only \
             commands: {supported}. Drop --demo-fleet to run this against a real controller."
        )
    )]
    DemoUnavailable { command: String, supported: String },

    #[error("`unifly {command}` cannot sanitize its output for demo mode")]
    #[diagnostic(code(unifi::demo_not_sanitized))]
    DemoNotSanitized {
        command: String,
        /// Built by `demo_fleet::refuse_demo`, which knows the switch.
        #[help]
        help: String,
    },

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

    #[error("Invalid controller configuration: {message}")]
    #[diagnostic(
        code(unifi::invalid_config),
        help(
            "The controller URL must be an http:// or https:// address, set with \
             --controller or the profile's `controller` key."
        )
    )]
    InvalidConfig { message: String },

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

    // ── Agent skill ──────────────────────────────────────────────────
    #[error("{path} exists and is not a unifly skill this version recognizes")]
    #[diagnostic(
        code(unifi::skill_exists),
        help(
            "It may hold local edits, or come from a newer unifly release.\n\
             Review it, then rerun with --force to replace it."
        )
    )]
    SkillFileExists { path: String },

    #[error("Could not install the agent skill at {path}")]
    #[diagnostic(code(unifi::skill_install))]
    SkillInstallFailed {
        path: String,
        #[source]
        source: std::io::Error,
    },

    // ── IO / Serialization ────────────────────────────────────────────
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("Invalid JSON payload: {0}")]
    #[diagnostic(code(unifi::json), help("Check the JSON file contents and try again."))]
    Json(#[from] serde_json::Error),

    // ── Config crate errors ──────────────────────────────────────────
    #[error(transparent)]
    #[diagnostic(code(unifi::config_error))]
    ConfigError(crate::config::ConfigError),
}

impl From<figment::Error> for CliError {
    fn from(err: figment::Error) -> Self {
        Self::Config(Box::new(err))
    }
}

/// Config resolution reports the same failures the CLI detects itself, so
/// those map onto the CLI's own variants: a missing credential or invalid
/// value then exits and explains itself the same way whether it came from
/// a profile or from flags.
impl From<crate::config::ConfigError> for CliError {
    fn from(err: crate::config::ConfigError) -> Self {
        use crate::config::ConfigError;
        match err {
            ConfigError::NoCredentials { profile } => Self::NoCredentials { profile },
            ConfigError::Validation { field, reason } => Self::Validation { field, reason },
            other => Self::ConfigError(other),
        }
    }
}

impl CliError {
    /// Map this error to an exit code for process termination.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::ConnectionFailed { .. } | Self::TlsError { .. } => exit_code::CONNECTION,
            Self::AuthFailed { .. } | Self::NoCredentials { .. } => exit_code::AUTH,
            Self::NotFound { .. } | Self::NotFoundUnlisted { .. } | Self::SiteNotFound { .. } => {
                exit_code::NOT_FOUND
            }
            Self::Conflict { .. } | Self::SkillFileExists { .. } => exit_code::CONFLICT,
            Self::Timeout { .. } => exit_code::TIMEOUT,
            Self::Validation { .. }
            | Self::InvalidConfig { .. }
            | Self::NonInteractiveRequiresYes { .. }
            | Self::SiteAmbiguous { .. }
            | Self::Ambiguous { .. } => exit_code::USAGE,
            Self::Unsupported { .. }
            | Self::NotYetImplemented { .. }
            | Self::DemoUnavailable { .. }
            | Self::DemoNotSanitized { .. } => exit_code::PERMISSION,
            _ => exit_code::GENERAL,
        }
    }

    /// An authentication failure whose recovery hint names `profile`, the
    /// profile the command ran with, or none for ad-hoc `--controller` use.
    pub fn auth_failed(message: String, profile: Option<&str>) -> Self {
        let stored_secret = match profile {
            Some(name) => format!(
                "To replace this profile's stored secret: unifly config set-password {name}"
            ),
            None => "To keep credentials in a profile instead: unifly config init".into(),
        };
        Self::AuthFailed {
            message,
            help: format!(
                "Verify your API key or credentials.\n\
                 For local controller API keys: check Settings > Integrations on your controller.\n\
                 For Site Manager cloud profiles: verify the API key can see the console and your UI account has UniFi Network admin access.\n\
                 {stored_secret}\n\
                 To re-run cloud onboarding: unifly config cloud-setup"
            ),
        }
    }

    /// Name `profile` in an authentication failure's hint. Errors convert
    /// from `CoreError` without knowing which profile was active, so the
    /// binary applies this once it has the result.
    #[must_use]
    pub fn for_profile(self, profile: Option<&str>) -> Self {
        match self {
            Self::AuthFailed { message, .. } => Self::auth_failed(message, profile),
            other => other,
        }
    }
}

/// The list command for each entity type `CoreError::NotFound` names. The
/// types are free text from the API layer, so a missing entry means no
/// hint rather than a guessed command.
const LIST_COMMANDS: &[(&str, &str)] = &[
    ("network", "networks list"),
    ("wifi broadcast", "wifi list"),
    ("firewall policy", "firewall policies list"),
    ("firewall group", "firewall groups list"),
    ("NAT rule", "nat policies list"),
    ("site-to-site VPN", "vpn site-to-site list"),
    ("remote-access VPN server", "vpn remote-access list"),
    ("WireGuard peer", "vpn peers list"),
    ("VPN client profile", "vpn clients list"),
    ("VPN client connection", "vpn connections list"),
    (
        "magic site-to-site VPN config",
        "vpn magic-site-to-site list",
    ),
    ("vpn setting", "vpn settings list"),
    ("setting", "settings list"),
];

fn list_command_for(entity_type: &str) -> Option<&'static str> {
    LIST_COMMANDS
        .iter()
        .find(|(kind, _)| kind.eq_ignore_ascii_case(entity_type))
        .map(|(_, command)| *command)
}

// ── CoreError → CliError mapping ─────────────────────────────────────

impl From<CoreError> for CliError {
    fn from(err: CoreError) -> Self {
        match err {
            CoreError::ConnectionFailed { url, reason } => CliError::ConnectionFailed {
                url: redact_userinfo(&url),
                source: reason.into(),
            },

            CoreError::AuthenticationFailed { message } => CliError::auth_failed(message, None),

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
            } => match list_command_for(&entity_type) {
                Some(list_command) => CliError::NotFound {
                    list_command: list_command.into(),
                    resource_type: entity_type,
                    identifier,
                },
                None => CliError::NotFoundUnlisted {
                    resource_type: entity_type,
                    identifier,
                },
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

            // The API layer raises this when the controller URL cannot
            // anchor its endpoint paths; config files are read before then.
            CoreError::Config { message } => CliError::InvalidConfig { message },

            CoreError::Internal(message) => CliError::ApiError {
                code: "internal".into(),
                message,
                request_id: None,
            },
        }
    }
}

/// `url` with any userinfo replaced by `***`, for messages that echo a
/// controller URL. Works on text that does not parse as a URL too, since
/// those are exactly the URLs an error reports.
pub fn redact_userinfo(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_owned();
    };
    let (prefix, rest) = url.split_at(scheme_end + 3);
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    match rest[..authority_end].rfind('@') {
        Some(at) => format!("{prefix}***@{}", &rest[at + 1..]),
        None => url.to_owned(),
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

#[cfg(test)]
mod tests {
    use clap::CommandFactory;
    use miette::Diagnostic;
    use unifly_api::model::{DnsPolicy, Voucher, WifiBroadcast};
    use unifly_api::{
        AclRule, Client, CoreError, Device, FirewallGroup, FirewallPolicy, FirewallZone, NatPolicy,
        Network, TrafficMatchingList,
    };

    use super::{CliError, LIST_COMMANDS, exit_code};
    use crate::cli::args::Cli;
    use crate::cli::commands::util::lookup::Lookup;

    fn assert_is_command(path: &str) {
        let root = Cli::command();
        let mut command = &root;
        for part in path.split(' ') {
            command = command
                .find_subcommand(part)
                .unwrap_or_else(|| panic!("hint names `unifly {path}`, which does not exist"));
        }
    }

    fn help(err: &CliError) -> Option<String> {
        err.help().map(|help| help.to_string())
    }

    #[test]
    fn every_list_hint_is_a_real_command() {
        let lookups = [
            Device::LIST,
            Client::LIST,
            Network::LIST,
            WifiBroadcast::LIST,
            FirewallPolicy::LIST,
            FirewallZone::LIST,
            FirewallGroup::LIST,
            NatPolicy::LIST,
            AclRule::LIST,
            TrafficMatchingList::LIST,
            DnsPolicy::LIST,
            Voucher::LIST,
        ];
        for path in LIST_COMMANDS.iter().map(|(_, path)| *path).chain(lookups) {
            assert_is_command(path);
        }
    }

    #[test]
    fn core_not_found_points_at_the_real_list_command() {
        for (entity_type, list) in [
            ("wifi broadcast", "wifi list"),
            ("NAT rule", "nat policies list"),
            ("firewall policy", "firewall policies list"),
            ("WireGuard peer", "vpn peers list"),
        ] {
            let err = CliError::from(CoreError::NotFound {
                entity_type: entity_type.into(),
                identifier: "x".into(),
            });
            let help = help(&err).unwrap_or_default();
            assert!(
                help.contains(&format!("unifly {list}")),
                "{entity_type}: {help}"
            );
            assert!(!help.contains("policys"), "{help}");
        }
    }

    #[test]
    fn core_not_found_without_a_list_command_offers_no_hint() {
        let err = CliError::from(CoreError::NotFound {
            entity_type: "resource".into(),
            identifier: "/proxy/network/x".into(),
        });
        assert_eq!(help(&err), None);
        assert_eq!(err.exit_code(), exit_code::NOT_FOUND);
    }

    #[test]
    fn auth_failure_hint_names_only_a_real_profile() {
        let err = CliError::from(CoreError::AuthenticationFailed {
            message: "Invalid API key".into(),
        });
        let generic = help(&err).unwrap_or_default();
        assert!(!generic.contains("current"), "{generic}");
        assert!(!generic.contains("set-password"), "{generic}");

        let named = help(&err.for_profile(Some("lab"))).unwrap_or_default();
        assert!(named.contains("unifly config set-password lab"), "{named}");
    }

    #[test]
    fn redact_userinfo_hides_credentials_only() {
        for (url, redacted) in [
            (
                "https://admin:hunter2@10.0.0.1:8443/x",
                "https://***@10.0.0.1:8443/x",
            ),
            ("http://admin@host", "http://***@host"),
            ("http://a:b@[::1", "http://***@[::1"),
            (
                "https://10.0.0.1/path@not-userinfo",
                "https://10.0.0.1/path@not-userinfo",
            ),
            ("https://unifi.local", "https://unifi.local"),
            ("not a url", "not a url"),
        ] {
            assert_eq!(super::redact_userinfo(url), redacted, "{url}");
        }
    }

    #[test]
    fn bad_controller_url_is_a_usage_error_not_a_missing_file() {
        let err = CliError::from(CoreError::Config {
            message: "Invalid URL: relative URL with a cannot-be-a-base base".into(),
        });
        assert!(matches!(err, CliError::InvalidConfig { .. }), "{err:?}");
        assert_eq!(err.exit_code(), exit_code::USAGE);
        assert!(err.to_string().contains("cannot-be-a-base"), "{err}");
    }
}
