//! Shared configuration for UniFi CLI and TUI.
//!
//! TOML profiles, credential resolution (keyring + env + plaintext),
//! and translation to `unifly_api::ControllerConfig`.

#[cfg(feature = "cli")]
pub mod resolve;
pub mod totp_command;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use figment::{
    Figment,
    providers::{Format, Serialized, Toml},
};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use unifly_api::{AuthCredentials, ControllerConfig, TlsVerification, TotpProvider};

pub const DEFAULT_CLOUD_CONTROLLER_URL: &str = "https://api.ui.com";

// ── Error ───────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid {field}: {reason}")]
    Validation { field: String, reason: String },

    #[error("no credentials configured for profile '{profile}'")]
    NoCredentials { profile: String },

    #[error("failed to serialize config: {0}")]
    Serialization(#[from] toml::ser::Error),

    #[error("config loading failed: {0}")]
    Figment(Box<figment::Error>),

    /// The config file is there but could not be read or parsed.
    #[error("could not load {}: {reason}", path.display())]
    Load { path: PathBuf, reason: String },

    /// The config file does not define the profile asked for.
    #[error(
        "profile '{name}' is not defined in {}; {}. Create one with: unifly config init \
         (local) or unifly config cloud-setup (Site Manager)",
        path.display(),
        list_defined_profiles(defined)
    )]
    ProfileNotDefined {
        name: String,
        path: PathBuf,
        defined: Vec<String>,
    },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl ConfigError {
    /// `cfg`, loaded from the config file, has no profile called `name`.
    pub fn profile_not_defined(cfg: &Config, name: &str) -> Self {
        let mut defined: Vec<_> = cfg.profiles.keys().cloned().collect();
        defined.sort();
        Self::ProfileNotDefined {
            name: name.to_owned(),
            path: config_path(),
            defined,
        }
    }
}

fn list_defined_profiles(defined: &[String]) -> String {
    if defined.is_empty() {
        "it defines no profiles yet".into()
    } else {
        format!("defined profiles: {}", defined.join(", "))
    }
}

impl From<figment::Error> for ConfigError {
    fn from(err: figment::Error) -> Self {
        Self::Figment(Box::new(err))
    }
}

// ── TOML config structs ─────────────────────────────────────────────

/// Top-level TOML configuration shared by CLI and TUI.
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    /// Default profile name.
    pub default_profile: Option<String>,

    /// Global defaults.
    #[serde(default)]
    pub defaults: Defaults,

    /// Named controller profiles.
    #[serde(default)]
    pub profiles: HashMap<String, Profile>,

    /// Demo mode configuration for PII sanitization.
    #[serde(default)]
    pub demo: DemoConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_profile: Some("default".into()),
            defaults: Defaults::default(),
            profiles: HashMap::new(),
            demo: DemoConfig::default(),
        }
    }
}

/// Demo mode settings for PII sanitization in recordings and demos.
///
/// Activated by `[demo] enabled = true` in config, `--demo` CLI flag,
/// or `UNIFI_DEMO=1` environment variable.
#[derive(Debug, Deserialize, Serialize, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct DemoConfig {
    #[serde(default)]
    pub enabled: bool,

    /// Names to redact (case-insensitive substring match across all text).
    #[serde(default)]
    pub redact_names: Vec<String>,

    /// Names to keep visible even if they'd otherwise match a redact pattern.
    #[serde(default)]
    pub keep_names: Vec<String>,

    /// Replace WiFi SSID names with generic alternatives.
    #[serde(default)]
    pub redact_ssids: bool,

    /// Replace public/WAN IP addresses with RFC 5737 documentation IPs.
    #[serde(default = "default_true")]
    pub redact_wan_ips: bool,

    /// Replace MAC addresses with locally-administered fakes.
    #[serde(default)]
    pub redact_macs: bool,

    /// Replace ISP name and upstream DNS in health data.
    #[serde(default)]
    pub redact_isp: bool,

    /// Fixed seed for deterministic replacements across sessions.
    pub seed: Option<String>,
}

impl Default for DemoConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            redact_names: Vec::new(),
            keep_names: Vec::new(),
            redact_ssids: false,
            redact_wan_ips: true,
            redact_macs: false,
            redact_isp: false,
            seed: None,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Defaults {
    #[serde(default = "default_output")]
    pub output: String,

    #[serde(default = "default_color")]
    pub color: String,

    #[serde(default)]
    pub insecure: bool,

    #[serde(default = "default_timeout")]
    pub timeout: u64,

    /// Theme name for the TUI (e.g., "nord", "dracula", "silkcircuit-neon").
    #[serde(default)]
    pub theme: Option<String>,

    /// Whether to show the donate button in the TUI status bar.
    #[serde(default = "default_show_donate")]
    pub show_donate: bool,

    /// Whether tachyonfx animations are enabled in the TUI.
    #[serde(default = "default_effects")]
    pub effects: bool,

    /// TUI chart glyph quality: "block", "braille", or "octant".
    /// Defaults to "octant" when unset.
    #[serde(default)]
    pub chart_quality: Option<String>,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            output: default_output(),
            color: default_color(),
            insecure: false,
            timeout: default_timeout(),
            theme: None,
            show_donate: default_show_donate(),
            effects: default_effects(),
            chart_quality: None,
        }
    }
}

fn default_output() -> String {
    "table".into()
}
fn default_color() -> String {
    "auto".into()
}
fn default_timeout() -> u64 {
    30
}
fn default_show_donate() -> bool {
    true
}
fn default_effects() -> bool {
    true
}

/// A named controller profile.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Profile {
    /// Controller base URL (e.g., "https://192.168.1.1").
    #[serde(default)]
    pub controller: String,

    /// Site name or UUID.
    #[serde(default = "default_site")]
    pub site: String,

    /// Auth mode: "integration", "session", "hybrid", or "cloud".
    #[serde(default = "default_auth_mode")]
    pub auth_mode: String,

    /// API key (plaintext — prefer keyring or env var).
    pub api_key: Option<String>,

    /// Environment variable name containing the API key.
    pub api_key_env: Option<String>,

    /// Host/console ID for cloud connector mode.
    pub host_id: Option<String>,

    /// Environment variable name containing the host/console ID.
    pub host_id_env: Option<String>,

    /// Username for session auth.
    pub username: Option<String>,

    /// Password for session auth (plaintext — prefer keyring).
    pub password: Option<String>,

    /// Environment variable name containing a TOTP token for MFA.
    ///
    /// Useful with 1Password CLI: `totp_env = "UNIFI_TOTP"` and
    /// `UNIFI_TOTP=$(op item get "UniFi" --otp) unifly ...`
    ///
    /// The variable is read once, at startup, and nothing outside the
    /// process can change it afterwards, so the code serves the first login
    /// only. Without `totp_command`, a session opened with it does not renew
    /// itself when the cookie expires.
    pub totp_env: Option<String>,

    /// Command that prints a current TOTP code, as an argv array: the
    /// program, then its arguments. It runs directly, without a shell, with
    /// no stdin and a 10 s limit; its trimmed stdout is the code.
    ///
    /// It runs again for every login that needs a code, so a long-running
    /// session (the TUI) can renew itself when its cookie expires. An
    /// explicit code (`--totp`, `UNIFI_TOTP`, or `totp_env`) still serves
    /// the first login. Example with 1Password CLI:
    /// `totp_command = ["op", "item", "get", "UniFi", "--otp"]`
    ///
    /// A locked password manager may prompt to unlock on every re-login
    /// attempt, about once per 30 s cooldown while the session is expired.
    /// On Windows, a helper that leaves a background process holding its
    /// stdout can leave one blocked thread until that process exits.
    pub totp_command: Option<Vec<String>>,

    /// Path to custom CA certificate.
    pub ca_cert: Option<PathBuf>,

    /// Override insecure TLS setting.
    pub insecure: Option<bool>,

    /// Override timeout.
    pub timeout: Option<u64>,
}

fn default_site() -> String {
    "default".into()
}
fn default_auth_mode() -> String {
    "integration".into()
}

// ── Config file path ────────────────────────────────────────────────

/// Resolve the config file path.
///
/// Unix (including macOS): `$XDG_CONFIG_HOME/unifly/config.toml` or `~/.config/unifly/config.toml`
/// Windows: `%APPDATA%\unifly\config.toml` via `ProjectDirs`, with env-var fallback
///
/// On macOS, automatically migrates from the old `~/Library/Application Support/unifly/`
/// location if it exists and the XDG path does not.
pub fn config_path() -> PathBuf {
    let dir = config_dir();
    #[cfg(target_os = "macos")]
    migrate_macos_config(&dir);
    dir.join("config.toml")
}

#[cfg(windows)]
fn config_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "unifly").map_or_else(
        || {
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("USERPROFILE")
                        .map(PathBuf::from)
                        .map(|home| home.join("AppData").join("Roaming"))
                })
                .unwrap_or_else(|| PathBuf::from("."))
                .join("unifly")
        },
        |dirs| dirs.config_dir().to_owned(),
    )
}

#[cfg(not(windows))]
fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map_or_else(
            || {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()))
                    .join(".config")
            },
            PathBuf::from,
        )
        .join("unifly")
}

#[cfg(target_os = "macos")]
fn migrate_macos_config(new_dir: &std::path::Path) {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let Some(old_dir) =
            directories::ProjectDirs::from("", "", "unifly").map(|d| d.config_dir().to_owned())
        else {
            return;
        };
        if old_dir == *new_dir || !old_dir.exists() || new_dir.exists() {
            return;
        }
        if let Some(parent) = new_dir.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::rename(&old_dir, new_dir) {
            Ok(()) => {
                eprintln!("unifly: migrated config to {}", new_dir.display());
            }
            Err(e) => {
                eprintln!(
                    "unifly: could not auto-migrate config from {} to {}: {e}",
                    old_dir.display(),
                    new_dir.display(),
                );
                eprintln!("unifly: please move your config.toml manually to the new location");
            }
        }
    });
}

// ── Config loading ──────────────────────────────────────────────────

/// Load the full Config from the config file.
///
/// Environment variables are not read here: the `UNIFI_*` variables belong
/// to the global flags (clap reads them), and reading them again as config
/// keys made `UNIFI_DEMO` collide with the `[demo]` table.
///
/// A file that exists but cannot be loaded is an error, never the defaults;
/// see [`load_config_from`].
pub fn load_config() -> Result<Config, ConfigError> {
    load_config_from(&config_path())
}

/// Load the Config stored at `path`.
///
/// A missing file is a fresh install and yields the defaults. A file that
/// is there but cannot be read or parsed is an error, never the defaults:
/// saving defaults over a file with one typo in it would erase every
/// profile it holds.
pub fn load_config_from(path: &Path) -> Result<Config, ConfigError> {
    let load_error = |reason: String| ConfigError::Load {
        path: path.to_owned(),
        reason,
    };

    if !path
        .try_exists()
        .map_err(|error| load_error(error.to_string()))?
    {
        return Ok(Config::default());
    }

    Figment::new()
        .merge(Serialized::defaults(Config::default()))
        .merge(Toml::file_exact(path))
        .extract()
        .map_err(|error| load_error(describe_figment_error(error)))
}

/// What went wrong and at which key, without figment's trailing "in <file>
/// TOML file" (the load error names the file already).
///
/// The message never repeats what the file holds unless the key is a plain
/// setting: these errors get pasted into issues, and the line at fault may
/// be a password or a TOTP seed. A parse error keeps its line and column but
/// drops the source excerpt under them, and any other invalid value is
/// described by its key alone.
fn describe_figment_error(error: figment::Error) -> String {
    error
        .into_iter()
        .map(|error| match error.path.as_slice() {
            [] => without_source_excerpt(&error.kind.to_string()),
            path if value_is_safe_to_show(path) => {
                format!("{} for key `{}`", error.kind, path.join("."))
            }
            _ => hidden_value_problem(&error),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Whether the value at `path` may appear in an error. This is an allowlist
/// of plain settings: anything else, including a field added later, is
/// hidden, so a new secret cannot leak by default.
fn value_is_safe_to_show(path: &[String]) -> bool {
    const DEFAULTS: [&str; 8] = [
        "output",
        "color",
        "insecure",
        "timeout",
        "theme",
        "show_donate",
        "effects",
        "chart_quality",
    ];
    const PROFILE: [&str; 11] = [
        "controller",
        "site",
        "auth_mode",
        "api_key_env",
        "host_id",
        "host_id_env",
        "username",
        "totp_env",
        "ca_cert",
        "insecure",
        "timeout",
    ];
    const DEMO: [&str; 5] = [
        "enabled",
        "redact_ssids",
        "redact_wan_ips",
        "redact_macs",
        "redact_isp",
    ];

    match path {
        [key] => key == "default_profile",
        [section, key] if section == "defaults" => DEFAULTS.contains(&key.as_str()),
        [section, _, key] if section == "profiles" => PROFILE.contains(&key.as_str()),
        [section, key] if section == "demo" => DEMO.contains(&key.as_str()),
        _ => false,
    }
}

/// Describe `error` by its key and what was expected, leaving out the value.
fn hidden_value_problem(error: &figment::Error) -> String {
    use figment::error::Kind;

    let problem = match &error.kind {
        Kind::InvalidType(_, expected) | Kind::InvalidValue(_, expected) => {
            format!("invalid value, expected {expected}")
        }
        // These name a field or a length, never the value.
        Kind::MissingField(_)
        | Kind::DuplicateField(_)
        | Kind::UnknownField(..)
        | Kind::InvalidLength(..) => error.kind.to_string(),
        _ => "invalid value".to_owned(),
    };
    format!(
        "{problem} for key `{}` (the value is not shown)",
        error.path.join(".")
    )
}

/// `message` without the TOML source excerpt (`  |` and `2 | ...` lines)
/// that parse errors quote, one line per remaining part.
fn without_source_excerpt(message: &str) -> String {
    message
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line
                    .trim_start_matches(|c: char| c.is_ascii_digit())
                    .trim_start()
                    .starts_with('|')
        })
        .collect::<Vec<_>>()
        .join("; ")
}

// ── Config saving ───────────────────────────────────────────────────

/// Serialize config to TOML and write to the canonical config path.
pub fn save_config(cfg: &Config) -> Result<(), ConfigError> {
    save_config_to(&config_path(), cfg)
}

/// Serialize config to TOML and write it to `path`.
pub fn save_config_to(path: &Path, cfg: &Config) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let toml_str = toml::to_string_pretty(cfg)?;
    std::fs::write(path, toml_str)?;

    // Restrict config file to owner-only access (may contain plaintext credentials)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

// ── Secret storage ──────────────────────────────────────────────────

/// Keyring service name every unifly secret is stored under.
pub const KEYRING_SERVICE: &str = "unifly";

/// Keyring entry holding a profile's API key.
pub fn api_key_keyring_key(profile_name: &str) -> String {
    format!("{profile_name}/api-key")
}

/// Keyring entry holding a profile's session password.
pub fn password_keyring_key(profile_name: &str) -> String {
    format!("{profile_name}/password")
}

/// Where profile secrets live outside `config.toml`: [`OsKeyring`] in the
/// app, an in-memory stand-in in tests so they never touch the real one.
pub trait SecretStore {
    /// The secret stored under `key`: `Ok(None)` when nothing is, an error
    /// when the store could not say.
    fn get(&self, key: &str) -> Result<Option<String>, SecretStoreError>;
    /// Store `secret` under `key`, replacing any previous value.
    fn set(&self, key: &str, secret: &str) -> Result<(), SecretStoreError>;
    /// Remove whatever is stored under `key`; nothing stored is not an error.
    fn delete(&self, key: &str) -> Result<(), SecretStoreError>;
}

/// Why a secret could not be stored. Only [`Self::Unavailable`] justifies
/// the documented plaintext fallback; a keyring that is there but refuses
/// a secret must not be quietly swapped for a plaintext file.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SecretStoreError {
    /// No usable keyring here: none could be opened, it is locked or
    /// unreachable, or the platform store failed.
    #[error("no usable keyring: {0}")]
    Unavailable(String),
    /// The keyring is there but refused this secret (an ambiguous entry, a
    /// value it cannot hold, an invalid name, and the like).
    #[error("the keyring refused it: {0}")]
    Rejected(String),
}

impl From<&keyring::Error> for SecretStoreError {
    fn from(error: &keyring::Error) -> Self {
        match error {
            keyring::Error::NoStorageAccess(_)
            | keyring::Error::PlatformFailure(_)
            | keyring::Error::NoDefaultStore => Self::Unavailable(error.to_string()),
            _ => Self::Rejected(error.to_string()),
        }
    }
}

/// The OS keyring, where `config init` and `config set-password` put
/// secrets and the credential resolvers below look for them.
pub struct OsKeyring;

impl SecretStore for OsKeyring {
    fn get(&self, key: &str) -> Result<Option<String>, SecretStoreError> {
        match keyring::Entry::new(KEYRING_SERVICE, key).and_then(|entry| entry.get_password()) {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(SecretStoreError::from(&error)),
        }
    }

    fn set(&self, key: &str, secret: &str) -> Result<(), SecretStoreError> {
        keyring::Entry::new(KEYRING_SERVICE, key)
            .and_then(|entry| entry.set_password(secret))
            .map_err(|error| SecretStoreError::from(&error))
    }

    fn delete(&self, key: &str) -> Result<(), SecretStoreError> {
        match keyring::Entry::new(KEYRING_SERVICE, key).and_then(|entry| entry.delete_credential())
        {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(SecretStoreError::from(&error)),
        }
    }
}

// ── Credential resolution (without CLI flags) ───────────────────────

/// Resolve an API key from the credential chain (no CLI flag step).
///
/// Precedence: explicit env var (`api_key_env`) → explicit config plaintext
/// (`api_key`) → system keyring. Explicit config beats the keyring so that
/// editing `api_key = "..."` in config.toml is not silently overridden by a
/// stale keyring entry from a previous `config init`.
pub fn resolve_api_key(profile: &Profile, profile_name: &str) -> Result<SecretString, ConfigError> {
    // 1. Profile's api_key_env → env var lookup
    if let Some(ref env_name) = profile.api_key_env
        && let Ok(val) = std::env::var(env_name)
    {
        tracing::debug!(
            profile = profile_name,
            env = env_name,
            "resolved api_key from env"
        );
        return Ok(SecretString::from(val));
    }

    // 2. Explicit plaintext in config
    if let Some(ref key) = profile.api_key {
        tracing::debug!(profile = profile_name, "resolved api_key from config");
        return Ok(SecretString::from(key.clone()));
    }

    // 3. System keyring
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, &api_key_keyring_key(profile_name))
        && let Ok(secret) = entry.get_password()
    {
        tracing::debug!(profile = profile_name, "resolved api_key from keyring");
        return Ok(SecretString::from(secret));
    }

    Err(ConfigError::NoCredentials {
        profile: profile_name.into(),
    })
}

/// Resolve a cloud connector host ID without CLI flag overrides.
pub fn resolve_host_id(profile: &Profile) -> Result<String, ConfigError> {
    if let Some(ref env_name) = profile.host_id_env
        && let Ok(val) = std::env::var(env_name)
        && !val.trim().is_empty()
    {
        return Ok(val);
    }

    if let Some(ref host_id) = profile.host_id
        && !host_id.trim().is_empty()
    {
        return Ok(host_id.clone());
    }

    Err(ConfigError::Validation {
        field: "host_id".into(),
        reason: "required when auth_mode is 'cloud'".into(),
    })
}

/// Resolve session credentials (username + password) without CLI flags.
///
/// Password precedence mirrors api_key: env var → explicit config plaintext
/// → system keyring. Explicit config beats the keyring so that editing
/// `password = "..."` in config.toml is not silently overridden by a stale
/// keyring entry.
pub fn resolve_session_credentials(
    profile: &Profile,
    profile_name: &str,
) -> Result<(String, SecretString), ConfigError> {
    let username = profile
        .username
        .clone()
        .or_else(|| std::env::var("UNIFI_USERNAME").ok())
        .ok_or_else(|| ConfigError::NoCredentials {
            profile: profile_name.into(),
        })?;

    // 1. Env var
    if let Ok(pw) = std::env::var("UNIFI_PASSWORD") {
        tracing::debug!(profile = profile_name, "resolved password from env");
        return Ok((username, SecretString::from(pw)));
    }

    // 2. Explicit plaintext in config
    if let Some(ref pw) = profile.password {
        tracing::debug!(profile = profile_name, "resolved password from config");
        return Ok((username, SecretString::from(pw.clone())));
    }

    // 3. Keyring
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, &password_keyring_key(profile_name))
        && let Ok(pw) = entry.get_password()
    {
        tracing::debug!(profile = profile_name, "resolved password from keyring");
        return Ok((username, SecretString::from(pw)));
    }

    Err(ConfigError::NoCredentials {
        profile: profile_name.into(),
    })
}

/// Resolve `AuthCredentials` from a profile's `auth_mode` field.
pub fn resolve_auth(profile: &Profile, profile_name: &str) -> Result<AuthCredentials, ConfigError> {
    match profile.auth_mode.as_str() {
        "integration" => {
            let secret = resolve_api_key(profile, profile_name)?;
            Ok(AuthCredentials::ApiKey(secret))
        }
        // Accept "legacy" as a backwards-compatible alias for "session"
        "session" | "legacy" => {
            let (username, password) = resolve_session_credentials(profile, profile_name)?;
            Ok(AuthCredentials::Credentials { username, password })
        }
        "hybrid" => {
            let api_key = resolve_api_key(profile, profile_name)?;
            let (username, password) = resolve_session_credentials(profile, profile_name)?;
            Ok(AuthCredentials::Hybrid {
                api_key,
                username,
                password,
            })
        }
        "cloud" => {
            let api_key = resolve_api_key(profile, profile_name)?;
            let host_id = resolve_host_id(profile)?;
            Ok(AuthCredentials::Cloud { api_key, host_id })
        }
        other => Err(ConfigError::Validation {
            field: "auth_mode".into(),
            reason: format!(
                "expected 'integration', 'session', 'hybrid', or 'cloud', got '{other}'"
            ),
        }),
    }
}

/// Resolve a TOTP token from the profile's `totp_env` field.
///
/// Returns `None` if no `totp_env` is configured or the env var is unset.
/// The result is a fixed code, never a `TotpProvider`: re-reading the same
/// variable later would only return the spent code again.
pub fn resolve_totp_token(profile: &Profile) -> Option<SecretString> {
    let env_name = profile.totp_env.as_deref()?;
    std::env::var(env_name).ok().map(SecretString::from)
}

/// Turn the profile's `totp_command` into a [`TotpProvider`].
///
/// Returns `None` when no command is configured, and an error when the
/// command names no program.
pub fn resolve_totp_provider(profile: &Profile) -> Result<Option<TotpProvider>, ConfigError> {
    let Some(argv) = profile.totp_command.as_ref() else {
        return Ok(None);
    };
    if argv.first().is_none_or(|program| program.trim().is_empty()) {
        return Err(ConfigError::Validation {
            field: "totp_command".into(),
            reason: "must start with the program to run, e.g. [\"op\", \"item\", \"get\", \
                     \"UniFi\", \"--otp\"]"
                .into(),
        });
    }
    Ok(Some(totp_command::provider(argv.clone())))
}

/// Build a `ControllerConfig` from a profile — no CLI flag overrides.
///
/// Suitable for the TUI and other non-CLI consumers. Sets TUI-friendly
/// defaults: `websocket_enabled: true`, `refresh_interval_secs: 10`.
/// `[defaults]` supplies `insecure` and `timeout` when the profile
/// leaves them unset, mirroring the CLI resolution ladder.
pub fn profile_to_controller_config(
    profile: &Profile,
    profile_name: &str,
    defaults: &Defaults,
) -> Result<ControllerConfig, ConfigError> {
    let is_cloud = profile.auth_mode == "cloud";
    let controller_url = if is_cloud && profile.controller.trim().is_empty() {
        DEFAULT_CLOUD_CONTROLLER_URL
    } else {
        profile.controller.as_str()
    };

    let url: url::Url = controller_url
        .parse()
        .map_err(|_| ConfigError::Validation {
            field: "controller".into(),
            reason: format!("invalid URL: {controller_url}"),
        })?;

    let auth = resolve_auth(profile, profile_name)?;

    // Profile insecure > profile ca_cert > defaults insecure: a CA is
    // still verification, so only an explicit insecure=true skips it.
    let tls = if is_cloud {
        TlsVerification::SystemDefaults
    } else if profile.insecure.unwrap_or(false) {
        TlsVerification::DangerAcceptInvalid
    } else if let Some(ref ca_path) = profile.ca_cert {
        TlsVerification::CustomCa(ca_path.clone())
    } else if profile.insecure.unwrap_or(defaults.insecure) {
        TlsVerification::DangerAcceptInvalid
    } else {
        TlsVerification::SystemDefaults
    };

    let timeout = Duration::from_secs(profile.timeout.unwrap_or(defaults.timeout));

    let totp_token = resolve_totp_token(profile);
    let totp_provider = resolve_totp_provider(profile)?;

    Ok(ControllerConfig {
        url,
        auth,
        site: profile.site.clone(),
        tls,
        timeout,
        refresh_interval_secs: if is_cloud { 60 } else { 10 },
        websocket_enabled: !is_cloud,
        polling_interval_secs: if is_cloud { 30 } else { 10 },
        totp_token,
        totp_provider,
        profile_name: Some(profile_name.to_owned()),
        no_session_cache: is_cloud,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        Config, ConfigError, DEFAULT_CLOUD_CONTROLLER_URL, Defaults, Profile, load_config_from,
        profile_to_controller_config, resolve_api_key, resolve_auth,
    };
    use secrecy::ExposeSecret;
    use std::time::Duration;
    use unifly_api::{AuthCredentials, TlsVerification};

    fn cloud_profile() -> Profile {
        Profile {
            controller: String::new(),
            site: "default".into(),
            auth_mode: "cloud".into(),
            api_key: Some("cloud-key".into()),
            api_key_env: None,
            host_id: Some(
                "70A7419783ED0000000006797F060000000006C719490000000062ABD4EA:1261206302".into(),
            ),
            host_id_env: None,
            username: None,
            password: None,
            totp_env: None,
            totp_command: None,
            ca_cert: None,
            insecure: Some(true),
            timeout: Some(45),
        }
    }

    #[test]
    fn load_config_from_starts_a_missing_file_from_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");

        let config = load_config_from(&dir.path().join("config.toml")).expect("defaults");

        assert!(config.profiles.is_empty());
        assert_eq!(config.default_profile.as_deref(), Some("default"));
    }

    #[test]
    fn load_config_from_reports_a_file_it_cannot_parse() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[defaults]\ntimeout = \"thirty\"\n").expect("write config");

        let error = load_config_from(&path).expect_err("a typo must not load as defaults");

        assert!(matches!(&error, ConfigError::Load { path: at, .. } if *at == path));
        let message = error.to_string();
        assert!(message.contains("`defaults.timeout`"), "{message}");
        assert!(message.contains("thirty"), "{message}");
    }

    /// Load errors end up in issues and agent transcripts, so they point at
    /// the line and key without echoing what the file holds there.
    #[test]
    fn load_errors_never_echo_a_secret() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");

        for (contents, secret, locator) in [
            (
                "[profiles.work]\npassword = hunter2secret\n",
                "hunter2secret",
                "line 2",
            ),
            (
                "[profiles.work]\npassword = 98765432\n",
                "98765432",
                "`profiles.work.password`",
            ),
            (
                "[profiles.work]\napi_key = 1234567890\n",
                "1234567890",
                "`profiles.work.api_key`",
            ),
            (
                "[profiles]\nwork = \"SECRETAAA\"\n",
                "SECRETAAA",
                "`profiles.work`",
            ),
            (
                "profiles = \"PROFILESECRET\"\n",
                "PROFILESECRET",
                "`profiles`",
            ),
            (
                "[profiles.work]\ntotp_command = \"oathtool --totp -b SEEDSECRET1\"\n",
                "SEEDSECRET1",
                "`profiles.work.totp_command`",
            ),
            (
                "[profiles.work]\ntotp_command = [\"oathtool\", 424242]\n",
                "424242",
                "`profiles.work.totp_command.1`",
            ),
            (
                "[demo]\nredact_names = \"Bliss Secretname\"\n",
                "Secretname",
                "`demo.redact_names`",
            ),
        ] {
            std::fs::write(&path, contents).expect("write config");

            let message = load_config_from(&path)
                .expect_err("a malformed config must not load")
                .to_string();

            assert!(!message.contains(secret), "{message}");
            assert!(message.contains(locator), "{message}");
        }
    }

    /// Keys whose values are plain settings still show the value at fault,
    /// which is what makes a typo easy to spot.
    #[test]
    fn load_errors_still_show_plain_setting_values() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");

        for (contents, value) in [
            ("[defaults]\ntimeout = \"thirty\"\n", "thirty"),
            ("[profiles.work]\ninsecure = \"yes\"\n", "yes"),
            ("default_profile = 42\n", "42"),
        ] {
            std::fs::write(&path, contents).expect("write config");

            let message = load_config_from(&path)
                .expect_err("a malformed config must not load")
                .to_string();

            assert!(message.contains(value), "{message}");
        }
    }

    #[test]
    fn profile_not_defined_names_the_profiles_that_are() {
        let mut cfg = Config::default();
        let empty = ConfigError::profile_not_defined(&cfg, "typo").to_string();
        assert!(
            empty.starts_with("profile 'typo' is not defined in "),
            "{empty}"
        );
        assert!(empty.contains("; it defines no profiles yet."), "{empty}");

        cfg.profiles.insert("work".into(), cloud_profile());
        cfg.profiles.insert("home".into(), cloud_profile());
        let listed = ConfigError::profile_not_defined(&cfg, "typo").to_string();
        assert!(
            listed.contains("; defined profiles: home, work."),
            "{listed}"
        );

        for message in [empty, listed] {
            assert!(
                message.ends_with(
                    "Create one with: unifly config init (local) or unifly config \
                     cloud-setup (Site Manager)"
                ),
                "{message}"
            );
        }
    }

    #[test]
    fn resolve_auth_supports_cloud_profiles() {
        let profile = cloud_profile();

        let auth = resolve_auth(&profile, "cloud").expect("cloud auth should resolve");

        match auth {
            AuthCredentials::Cloud { host_id, .. } => {
                assert_eq!(
                    host_id,
                    "70A7419783ED0000000006797F060000000006C719490000000062ABD4EA:1261206302"
                );
            }
            other => panic!("expected cloud auth, got {other:?}"),
        }
    }

    /// Regression: config plaintext must beat the keyring so that an
    /// explicit `api_key = "..."` in config.toml is not silently overridden
    /// by a stale keyring entry from a previous `config init`. See issue #13.
    ///
    /// We can't easily exercise the keyring branch in unit tests (no mock
    /// backend in keyring 3.x and writing to the real keyring is flaky in
    /// CI), so this asserts the config branch resolves -- combined with
    /// the explicit ordering in `resolve_api_key`, that's the precedence
    /// guarantee.
    #[test]
    fn resolve_api_key_prefers_explicit_config_over_fallback() {
        let mut profile = cloud_profile();
        profile.api_key = Some("explicit-config-key".into());
        profile.api_key_env = None;

        let secret = resolve_api_key(&profile, "cloud").expect("config api_key should resolve");

        assert_eq!(secret.expose_secret(), "explicit-config-key");
    }

    /// Only a keyring that cannot be used at all justifies the plaintext
    /// fallback; one that refuses a secret must stop the save.
    #[test]
    fn keyring_errors_split_into_unavailable_and_rejected() {
        use super::SecretStoreError;

        let platform = || -> Box<dyn std::error::Error + Send + Sync> {
            Box::new(std::io::Error::other("locked"))
        };
        let unavailable = [
            keyring::Error::NoStorageAccess(platform()),
            keyring::Error::PlatformFailure(platform()),
            keyring::Error::NoDefaultStore,
        ];
        for error in &unavailable {
            assert!(
                matches!(
                    SecretStoreError::from(error),
                    SecretStoreError::Unavailable(_)
                ),
                "{error}"
            );
        }

        let rejected = [
            keyring::Error::Ambiguous(Vec::new()),
            keyring::Error::BadEncoding(Vec::new()),
            keyring::Error::TooLong("password".into(), 4),
            keyring::Error::Invalid("service".into(), "empty".into()),
            keyring::Error::NotSupportedByStore("write".into()),
        ];
        for error in &rejected {
            assert!(
                matches!(SecretStoreError::from(error), SecretStoreError::Rejected(_)),
                "{error}"
            );
        }
    }

    #[test]
    fn profile_to_controller_config_wires_the_totp_command() {
        let mut profile = cloud_profile();
        profile.auth_mode = "session".into();
        profile.controller = "https://10.0.1.1".into();
        profile.username = Some("admin".into());
        profile.password = Some("pw".into());
        profile.totp_command = Some(vec!["op".into(), "item".into(), "get".into()]);

        let config = profile_to_controller_config(&profile, "default", &Defaults::default())
            .expect("profile should resolve");

        assert!(config.totp_provider.is_some());
        assert!(config.totp_token.is_none());
    }

    #[test]
    fn profile_to_controller_config_defaults_cloud_transport() {
        let profile = cloud_profile();

        let config = profile_to_controller_config(&profile, "cloud", &Defaults::default())
            .expect("cloud config should resolve");

        assert_eq!(
            config.url.as_str(),
            format!("{DEFAULT_CLOUD_CONTROLLER_URL}/")
        );
        assert!(matches!(config.tls, TlsVerification::SystemDefaults));
        assert!(!config.websocket_enabled);
        assert_eq!(config.refresh_interval_secs, 60);
        assert_eq!(config.polling_interval_secs, 30);
        assert!(config.no_session_cache);
    }

    fn direct_profile() -> Profile {
        let mut profile = cloud_profile();
        profile.auth_mode = "integration".into();
        profile.controller = "https://10.0.1.1".into();
        profile.insecure = None;
        profile.timeout = None;
        profile
    }

    #[test]
    fn profile_to_controller_config_applies_defaults_insecure_and_timeout() {
        let profile = direct_profile();

        let defaults = Defaults {
            insecure: true,
            timeout: 77,
            ..Defaults::default()
        };

        let config = profile_to_controller_config(&profile, "default", &defaults)
            .expect("profile should resolve");

        assert!(matches!(config.tls, TlsVerification::DangerAcceptInvalid));
        assert_eq!(config.timeout, Duration::from_secs(77));
    }

    #[test]
    fn profile_to_controller_config_ca_cert_beats_defaults_insecure() {
        let mut profile = direct_profile();
        profile.ca_cert = Some("/certs/controller-ca.pem".into());

        let defaults = Defaults {
            insecure: true,
            ..Defaults::default()
        };

        let config = profile_to_controller_config(&profile, "default", &defaults)
            .expect("profile should resolve");

        assert!(matches!(config.tls, TlsVerification::CustomCa(_)));
    }
}
