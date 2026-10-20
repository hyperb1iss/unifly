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

    #[error("{}", no_credentials_title(profile.as_deref()))]
    #[diagnostic(code(unifi::no_credentials))]
    NoCredentials {
        /// The profile missing a secret, or none for ad-hoc flags.
        profile: Option<String>,
        /// Built by the constructors below, which know what is missing.
        #[help]
        help: String,
    },

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
/// a profile or from flags. The match names every variant, so a new one
/// needs its own mapping decision.
impl From<crate::config::ConfigError> for CliError {
    fn from(err: crate::config::ConfigError) -> Self {
        use crate::config::ConfigError;
        match err {
            ConfigError::NoCredentials { profile } => {
                Self::profile_missing_credentials(&profile, None, false)
            }
            ConfigError::Validation { field, reason } => Self::Validation { field, reason },
            // The file could not be read or written, or lacks the profile
            // asked for: keep the config error's own message, which names
            // the file (and for a profile, the defined ones and how to
            // create one). A file that fails to load is invalid input and
            // exits 2 (see `exit_code`); the rest exit 1, as the
            // profile-not-found error did.
            err @ (ConfigError::Serialization(_)
            | ConfigError::Figment(_)
            | ConfigError::Io(_)
            | ConfigError::Load { .. }
            | ConfigError::ProfileNotDefined { .. }
            | ConfigError::Save { .. }) => Self::ConfigError(err),
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
            | Self::ConfigError(crate::config::ConfigError::Load { .. })
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

    /// An authentication failure whose hint fixes the credentials `source`
    /// supplied.
    pub fn auth_failed(message: String, source: CredentialSource<'_>) -> Self {
        let fix = match source {
            CredentialSource::ApiKeyFlag => "The key came from --api-key or UNIFI_API_KEY, \
                which override any profile: check that value."
                .to_owned(),
            CredentialSource::Profile(name) => format!(
                "To replace this profile's stored secret: unifly config set-password {}",
                shell_quote(name)
            ),
            CredentialSource::Unknown => {
                "To keep credentials in a profile: unifly config init".to_owned()
            }
        };
        Self::AuthFailed {
            message,
            help: format!(
                "Verify your API key or credentials.\n\
                 For local controller API keys: check Settings > Integrations on your controller.\n\
                 For Site Manager cloud profiles: verify the API key can see the console and your UI account has UniFi Network admin access.\n\
                 {fix}\n\
                 To re-run cloud onboarding: unifly config cloud-setup"
            ),
        }
    }

    /// Point an authentication failure's hint at where its credentials
    /// came from. Errors convert from `CoreError` without knowing that, so
    /// the binary applies this once it has the result.
    #[must_use]
    pub fn with_credential_source(self, source: CredentialSource<'_>) -> Self {
        match self {
            Self::AuthFailed { message, .. } => Self::auth_failed(message, source),
            other => other,
        }
    }

    /// No API key for an ad-hoc `--controller` run.
    pub fn no_api_key() -> Self {
        Self::NoCredentials {
            profile: None,
            help: "Pass --api-key or set UNIFI_API_KEY alongside --controller.\n\
                   To save the controller and its credentials instead: unifly config init \
                   (local) or unifly config cloud-setup (Site Manager)"
                .into(),
        }
    }

    /// No API key for a Site Manager query and no profile to read one from.
    pub fn no_cloud_api_key() -> Self {
        Self::NoCredentials {
            profile: None,
            help: "Pass --api-key or set UNIFI_API_KEY with a Site Manager API key.\n\
                   To save one in a profile instead: unifly config cloud-setup"
                .into(),
        }
    }

    /// Profile `name` lacks a secret it needs. Its `auth_mode`, when known,
    /// and whether a username is set say which secret, and so which fix.
    pub fn profile_missing_credentials(
        name: &str,
        auth_mode: Option<&str>,
        has_username: bool,
    ) -> Self {
        let quoted = shell_quote(name);
        let store = format!("unifly config set-password {quoted}");
        let set_username = format!(
            "Set the username: unifly -p {quoted} config set username <name> \
             (or set UNIFI_USERNAME)"
        );
        let help = match (auth_mode, has_username) {
            (Some("integration" | "cloud"), _) => format!(
                "Store this profile's API key: {store}\n\
                 Or pass --api-key, set UNIFI_API_KEY, or set the profile's api_key_env \
                 to a variable that holds the key."
            ),
            (Some("session" | "legacy"), false) => {
                format!("{set_username}\nThen store the password: {store} (or set UNIFI_PASSWORD)")
            }
            (Some("session" | "legacy"), true) => {
                format!("Store this profile's password: {store}\nOr set UNIFI_PASSWORD.")
            }
            (Some("hybrid"), false) => format!(
                "{set_username}\nThen store the API key and password: {store} \
                 (or set UNIFI_API_KEY and UNIFI_PASSWORD)"
            ),
            (Some("hybrid"), true) => format!(
                "Store this profile's API key and password: {store}\n\
                 Or set UNIFI_API_KEY and UNIFI_PASSWORD."
            ),
            _ => format!(
                "Store this profile's secret: {store}\n\
                 Or set UNIFI_API_KEY for an API key, or UNIFI_USERNAME and UNIFI_PASSWORD \
                 for a login."
            ),
        };
        Self::NoCredentials {
            profile: Some(name.to_owned()),
            help,
        }
    }
}

/// Where the credentials behind an authentication failure came from.
#[derive(Debug, Clone, Copy)]
pub enum CredentialSource<'a> {
    /// `--api-key` or `UNIFI_API_KEY`, which override any profile.
    ApiKeyFlag,
    /// The named profile's configured or stored secrets.
    Profile(&'a str),
    /// Not known where the error was raised.
    Unknown,
}

fn no_credentials_title(profile: Option<&str>) -> String {
    match profile {
        Some(name) => format!("No credentials configured for profile '{name}'"),
        None => "No API key given for the controller".into(),
    }
}

/// `word` as one shell word: bare when it needs no quoting, otherwise in
/// single quotes, so a hint like `set-password 'my lab'` pastes as is.
pub fn shell_quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '@'));
    if plain {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
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
                url: crate::config::redact_url_userinfo(&url),
                source: reason.into(),
            },

            CoreError::AuthenticationFailed { message } => {
                CliError::auth_failed(message, CredentialSource::Unknown)
            }

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

    use super::{CliError, CredentialSource, LIST_COMMANDS, exit_code, shell_quote};
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

        let named = help(&err.with_credential_source(CredentialSource::Profile("my lab")))
            .unwrap_or_default();
        assert!(
            named.contains("unifly config set-password 'my lab'"),
            "{named}"
        );
    }

    #[test]
    fn missing_credentials_hint_fixes_the_missing_secret() {
        let hint = |mode: Option<&str>, has_username: bool| {
            help(&CliError::profile_missing_credentials(
                "lab",
                mode,
                has_username,
            ))
            .unwrap_or_default()
        };

        let api_key = hint(Some("integration"), false);
        assert!(
            api_key.contains("API key: unifly config set-password lab"),
            "{api_key}"
        );

        let password = hint(Some("session"), true);
        assert!(
            password.contains("password: unifly config set-password lab"),
            "{password}"
        );
        assert!(!password.contains("UNIFI_API_KEY"), "{password}");
        assert!(!password.contains("username"), "{password}");

        let username = hint(Some("session"), false);
        assert!(
            username.contains("unifly -p lab config set username"),
            "{username}"
        );

        let hybrid = hint(Some("hybrid"), true);
        assert!(hybrid.contains("API key and password"), "{hybrid}");

        for mode in [Some("integration"), Some("session"), Some("hybrid"), None] {
            let text = hint(mode, true);
            assert!(!text.contains("cloud-setup"), "{mode:?}: {text}");
            assert!(!text.contains("config init"), "{mode:?}: {text}");
        }
    }

    #[test]
    fn shell_quote_keeps_hints_pasteable() {
        assert_eq!(shell_quote("lab"), "lab");
        assert_eq!(shell_quote("home-1.lan"), "home-1.lan");
        assert_eq!(shell_quote("my lab"), "'my lab'");
        assert_eq!(shell_quote("bob's"), "'bob'\\''s'");
        assert_eq!(shell_quote("$HOME"), "'$HOME'");
        assert_eq!(shell_quote(""), "''");
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
