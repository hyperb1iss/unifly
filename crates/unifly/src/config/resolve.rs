//! CLI-specific configuration resolution.
//!
//! Adds `GlobalOpts` flag overrides on top of the shared config module.

use std::time::Duration;

use secrecy::SecretString;

use unifly_api::{AuthCredentials, ControllerConfig, TlsVerification};

use crate::cli::args::GlobalOpts;
use crate::cli::error::CliError;
use crate::config::{self, Config, Profile};

// ── Re-exports for convenience ──────────────────────────────────

pub use crate::config::{Defaults, config_path, load_config_or_default, save_config};

// ── CLI-specific helpers ────────────────────────────────────────

/// Resolve the active profile name from CLI flags and config.
pub fn active_profile_name(global: &GlobalOpts, cfg: &Config) -> String {
    global
        .profile
        .clone()
        .or_else(|| cfg.default_profile.clone())
        .unwrap_or_else(|| "default".into())
}

/// Translate a `Profile` + global flags into a `ControllerConfig`.
///
/// CLI flag overrides take priority over profile values.
pub fn resolve_profile(
    profile: &Profile,
    profile_name: &str,
    global: &GlobalOpts,
) -> Result<ControllerConfig, CliError> {
    // 1. Controller URL (flag > env > profile)
    let url_str = global.controller.as_deref().unwrap_or(&profile.controller);
    let url: url::Url = url_str.parse().map_err(|_| CliError::Validation {
        field: "controller".into(),
        reason: format!("invalid URL: {url_str}"),
    })?;

    // 2. Auth credentials (CLI flag overrides take priority)
    let auth = match profile.auth_mode.as_str() {
        "integration" => {
            let secret = resolve_api_key_with_flag(profile, profile_name, global)?;
            AuthCredentials::ApiKey(secret)
        }
        "legacy" => {
            let (username, password) = config::resolve_legacy_credentials(profile, profile_name)?;
            AuthCredentials::Credentials { username, password }
        }
        "hybrid" => {
            let api_key = resolve_api_key_with_flag(profile, profile_name, global)?;
            let (username, password) = config::resolve_legacy_credentials(profile, profile_name)?;
            AuthCredentials::Hybrid {
                api_key,
                username,
                password,
            }
        }
        other => {
            return Err(CliError::Validation {
                field: "auth_mode".into(),
                reason: format!("expected 'integration', 'legacy', or 'hybrid', got '{other}'"),
            });
        }
    };

    // 3. TLS verification
    let tls = if global.insecure || profile.insecure.unwrap_or(false) {
        TlsVerification::DangerAcceptInvalid
    } else if let Some(ref ca_path) = profile.ca_cert {
        TlsVerification::CustomCa(ca_path.clone())
    } else {
        TlsVerification::SystemDefaults
    };

    // 4. Site (flag > env > profile)
    let site = global.site.as_deref().unwrap_or(&profile.site).to_string();

    // 5. Timeout
    let timeout = Duration::from_secs(global.timeout);

    // 6. TOTP token (CLI flag > profile's totp_env > UNIFI_TOTP env)
    let totp_token = global
        .totp
        .as_ref()
        .map(|t| SecretString::from(t.clone()))
        .or_else(|| config::resolve_totp(profile));

    Ok(ControllerConfig {
        url,
        auth,
        site,
        tls,
        timeout,
        refresh_interval_secs: 0,
        websocket_enabled: false,
        polling_interval_secs: 30,
        totp_token,
    })
}

/// Resolve API key with CLI flag override, then fall through to shared resolution.
fn resolve_api_key_with_flag(
    profile: &Profile,
    profile_name: &str,
    global: &GlobalOpts,
) -> Result<SecretString, CliError> {
    // CLI flag takes priority
    if let Some(ref key) = global.api_key {
        return Ok(SecretString::from(key.clone()));
    }
    Ok(config::resolve_api_key(profile, profile_name)?)
}
