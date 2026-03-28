//! Shared configuration for UniFi CLI and TUI.
//!
//! TOML profiles, credential resolution (keyring + env + plaintext),
//! and translation to `unifly_api::ControllerConfig`.

#[cfg(feature = "cli")]
pub mod resolve;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use directories::ProjectDirs;
use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use unifly_api::{AuthCredentials, ControllerConfig, TlsVerification};

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

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_profile: Some("default".into()),
            defaults: Defaults::default(),
            profiles: HashMap::new(),
        }
    }
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
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            output: default_output(),
            color: default_color(),
            insecure: false,
            timeout: default_timeout(),
            theme: None,
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

/// A named controller profile.
#[derive(Debug, Deserialize, Serialize)]
pub struct Profile {
    /// Controller base URL (e.g., "https://192.168.1.1").
    pub controller: String,

    /// Site name or UUID.
    #[serde(default = "default_site")]
    pub site: String,

    /// Auth mode: "integration", "legacy", or "hybrid".
    #[serde(default = "default_auth_mode")]
    pub auth_mode: String,

    /// API key (plaintext — prefer keyring or env var).
    pub api_key: Option<String>,

    /// Environment variable name containing the API key.
    pub api_key_env: Option<String>,

    /// Username for legacy auth.
    pub username: Option<String>,

    /// Password for legacy auth (plaintext — prefer keyring).
    pub password: Option<String>,

    /// Path to custom CA certificate.
    pub ca_cert: Option<PathBuf>,

    /// Override insecure TLS setting.
    pub insecure: Option<bool>,

    /// Override timeout.
    pub timeout: Option<u64>,

    /// Environment variable name containing a TOTP token for MFA.
    pub totp_env: Option<String>,
}

fn default_site() -> String {
    "default".into()
}
fn default_auth_mode() -> String {
    "integration".into()
}

// ── Config file path ────────────────────────────────────────────────

/// Resolve the config file path via XDG / platform conventions.
pub fn config_path() -> PathBuf {
    ProjectDirs::from("com", "unifly", "unifly").map_or_else(
        || {
            let mut p = dirs_fallback();
            p.push("config.toml");
            p
        },
        |dirs| dirs.config_dir().join("config.toml"),
    )
}

fn dirs_fallback() -> PathBuf {
    let mut p = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()));
    p.push(".config");
    p.push("unifly");
    p
}

// ── Config loading ──────────────────────────────────────────────────

/// Load the full Config from file + environment.
pub fn load_config() -> Result<Config, ConfigError> {
    let path = config_path();

    let figment = Figment::new()
        .merge(Serialized::defaults(Config::default()))
        .merge(Toml::file(&path))
        .merge(Env::prefixed("UNIFI_").split("_"));

    let config: Config = figment.extract()?;
    Ok(config)
}

/// Load config, returning a default if the file doesn't exist.
pub fn load_config_or_default() -> Config {
    load_config().unwrap_or_default()
}

// ── Config saving ───────────────────────────────────────────────────

/// Serialize config to TOML and write to the canonical config path.
pub fn save_config(cfg: &Config) -> Result<(), ConfigError> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let toml_str = toml::to_string_pretty(cfg)?;
    std::fs::write(&path, toml_str)?;

    // Restrict config file to owner-only access (may contain plaintext credentials)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

// ── Credential resolution (without CLI flags) ───────────────────────

/// Resolve an API key from the credential chain (no CLI flag step).
pub fn resolve_api_key(profile: &Profile, profile_name: &str) -> Result<SecretString, ConfigError> {
    // 1. Profile's api_key_env → env var lookup
    if let Some(ref env_name) = profile.api_key_env
        && let Ok(val) = std::env::var(env_name)
    {
        return Ok(SecretString::from(val));
    }

    // 2. System keyring
    if let Ok(entry) = keyring::Entry::new("unifly", &format!("{profile_name}/api-key"))
        && let Ok(secret) = entry.get_password()
    {
        return Ok(SecretString::from(secret));
    }

    // 3. Plaintext in config
    if let Some(ref key) = profile.api_key {
        return Ok(SecretString::from(key.clone()));
    }

    Err(ConfigError::NoCredentials {
        profile: profile_name.into(),
    })
}

/// Resolve legacy credentials (username + password) without CLI flags.
pub fn resolve_legacy_credentials(
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
        return Ok((username, SecretString::from(pw)));
    }

    // 2. Keyring
    if let Ok(entry) = keyring::Entry::new("unifly", &format!("{profile_name}/password"))
        && let Ok(pw) = entry.get_password()
    {
        return Ok((username, SecretString::from(pw)));
    }

    // 3. Plaintext in config
    if let Some(ref pw) = profile.password {
        return Ok((username, SecretString::from(pw.clone())));
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
        "legacy" => {
            let (username, password) = resolve_legacy_credentials(profile, profile_name)?;
            Ok(AuthCredentials::Credentials { username, password })
        }
        "hybrid" => {
            let api_key = resolve_api_key(profile, profile_name)?;
            let (username, password) = resolve_legacy_credentials(profile, profile_name)?;
            Ok(AuthCredentials::Hybrid {
                api_key,
                username,
                password,
            })
        }
        other => Err(ConfigError::Validation {
            field: "auth_mode".into(),
            reason: format!("expected 'integration', 'legacy', or 'hybrid', got '{other}'"),
        }),
    }
}

/// Resolve a TOTP token from the profile's `totp_env` field.
pub fn resolve_totp(profile: &Profile) -> Option<SecretString> {
    profile
        .totp_env
        .as_ref()
        .and_then(|env_name| std::env::var(env_name).ok())
        .map(SecretString::from)
}

/// Build a `ControllerConfig` from a profile — no CLI flag overrides.
///
/// Suitable for the TUI and other non-CLI consumers. Sets TUI-friendly
/// defaults: `websocket_enabled: true`, `refresh_interval_secs: 10`.
pub fn profile_to_controller_config(
    profile: &Profile,
    profile_name: &str,
) -> Result<ControllerConfig, ConfigError> {
    let url: url::Url = profile
        .controller
        .parse()
        .map_err(|_| ConfigError::Validation {
            field: "controller".into(),
            reason: format!("invalid URL: {}", profile.controller),
        })?;

    let auth = resolve_auth(profile, profile_name)?;

    let tls = if profile.insecure.unwrap_or(false) {
        TlsVerification::DangerAcceptInvalid
    } else if let Some(ref ca_path) = profile.ca_cert {
        TlsVerification::CustomCa(ca_path.clone())
    } else {
        TlsVerification::SystemDefaults
    };

    let timeout = Duration::from_secs(profile.timeout.unwrap_or(30));
    let totp_token = resolve_totp(profile);

    Ok(ControllerConfig {
        url,
        auth,
        site: profile.site.clone(),
        tls,
        timeout,
        refresh_interval_secs: 10,
        websocket_enabled: true,
        polling_interval_secs: 10,
        totp_token,
    })
}
