//! Shared configuration for UniFi CLI and TUI.
//!
//! TOML profiles, credential resolution (keyring + env + plaintext),
//! and translation to `unifly_api::ControllerConfig`.

#[cfg(feature = "cli")]
pub mod resolve;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use unifly_api::{AuthCredentials, ControllerConfig, TlsVerification};

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
    pub totp_env: Option<String>,

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
pub fn resolve_totp_token(profile: &Profile) -> Option<SecretString> {
    let env_name = profile.totp_env.as_deref()?;
    std::env::var(env_name).ok().map(SecretString::from)
}

/// Build a `ControllerConfig` from a profile — no CLI flag overrides.
///
/// Suitable for the TUI and other non-CLI consumers. Sets TUI-friendly
/// defaults: `websocket_enabled: true`, `refresh_interval_secs: 10`.
pub fn profile_to_controller_config(
    profile: &Profile,
    profile_name: &str,
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

    let tls = if is_cloud {
        TlsVerification::SystemDefaults
    } else if profile.insecure.unwrap_or(false) {
        TlsVerification::DangerAcceptInvalid
    } else if let Some(ref ca_path) = profile.ca_cert {
        TlsVerification::CustomCa(ca_path.clone())
    } else {
        TlsVerification::SystemDefaults
    };

    let timeout = Duration::from_secs(profile.timeout.unwrap_or(30));

    let totp_token = resolve_totp_token(profile);

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
        profile_name: Some(profile_name.to_owned()),
        no_session_cache: is_cloud,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_CLOUD_CONTROLLER_URL, Profile, profile_to_controller_config, resolve_auth,
    };
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
            ca_cert: None,
            insecure: Some(true),
            timeout: Some(45),
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

    #[test]
    fn profile_to_controller_config_defaults_cloud_transport() {
        let profile = cloud_profile();

        let config =
            profile_to_controller_config(&profile, "cloud").expect("cloud config should resolve");

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
}
