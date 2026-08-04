//! CLI-specific configuration resolution.
//!
//! Adds `GlobalOpts` flag overrides on top of the shared config module.

use std::time::Duration;

use secrecy::SecretString;

use unifly_api::{AuthCredentials, ControllerConfig, TlsVerification};

use crate::cli::args::GlobalOpts;
use crate::cli::error::CliError;
use crate::config::{self, Config, Profile, resolve_totp_token};

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
    let is_cloud = profile.auth_mode == "cloud";

    // 1. Controller URL (flag > profile > cloud default)
    let url_str = global.controller.as_deref().unwrap_or_else(|| {
        if is_cloud && profile.controller.trim().is_empty() {
            config::DEFAULT_CLOUD_CONTROLLER_URL
        } else {
            profile.controller.as_str()
        }
    });
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
        "session" | "legacy" => {
            let (username, password) = config::resolve_session_credentials(profile, profile_name)?;
            AuthCredentials::Credentials { username, password }
        }
        "hybrid" => {
            let api_key = resolve_api_key_with_flag(profile, profile_name, global)?;
            let (username, password) = config::resolve_session_credentials(profile, profile_name)?;
            AuthCredentials::Hybrid {
                api_key,
                username,
                password,
            }
        }
        "cloud" => {
            let api_key = resolve_api_key_with_flag(profile, profile_name, global)?;
            let host_id = resolve_host_id_with_flag(profile, global)?;
            AuthCredentials::Cloud { api_key, host_id }
        }
        other => {
            return Err(CliError::Validation {
                field: "auth_mode".into(),
                reason: format!(
                    "expected 'integration', 'session', 'hybrid', or 'cloud', got '{other}'"
                ),
            });
        }
    };

    // 3. TLS verification
    let tls = if is_cloud {
        TlsVerification::SystemDefaults
    } else if global.insecure || profile.insecure.unwrap_or(false) {
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

    // 6. TOTP (flag > env var from profile's totp_env)
    let totp_token = resolve_totp_with_flag(profile, global);

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
        profile_name: Some(profile_name.to_owned()),
        no_session_cache: global.no_cache || is_cloud,
    })
}

/// Resolve TOTP token: CLI flag takes priority, then profile's `totp_env`.
fn resolve_totp_with_flag(profile: &Profile, global: &GlobalOpts) -> Option<SecretString> {
    if let Some(ref totp) = global.totp {
        return Some(SecretString::from(totp.clone()));
    }
    resolve_totp_token(profile)
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

/// Resolve host ID with CLI flag override, then fall through to shared resolution.
fn resolve_host_id_with_flag(profile: &Profile, global: &GlobalOpts) -> Result<String, CliError> {
    if let Some(ref host_id) = global.host_id {
        return Ok(host_id.clone());
    }

    Ok(config::resolve_host_id(profile)?)
}

#[cfg(test)]
mod tests {
    use super::resolve_profile;
    use crate::cli::args::{ColorMode, GlobalOpts, OutputFormat};
    use crate::config::Profile;
    use unifly_api::{AuthCredentials, TlsVerification};

    fn base_global() -> GlobalOpts {
        GlobalOpts {
            profile: None,
            controller: None,
            site: None,
            api_key: Some("flag-api-key".into()),
            host_id: Some("host-from-flag".into()),
            output: OutputFormat::Table,
            color: ColorMode::Auto,
            theme: None,
            verbose: 0,
            quiet: false,
            yes: false,
            totp: None,
            no_cache: false,
            demo: false,
            insecure: true,
            timeout: 30,
            no_effects: false,
        }
    }

    fn cloud_profile() -> Profile {
        Profile {
            controller: String::new(),
            site: "default".into(),
            auth_mode: "cloud".into(),
            api_key: Some("profile-api-key".into()),
            api_key_env: None,
            host_id: Some("host-from-profile".into()),
            host_id_env: None,
            username: None,
            password: None,
            totp_env: None,
            ca_cert: None,
            insecure: Some(true),
            timeout: None,
        }
    }

    #[test]
    fn resolve_profile_supports_cloud_defaults_and_flag_override() {
        let profile = cloud_profile();
        let global = base_global();

        let resolved =
            resolve_profile(&profile, "cloud", &global).expect("cloud profile should resolve");

        assert_eq!(resolved.url.as_str(), "https://api.ui.com/");
        assert!(matches!(resolved.tls, TlsVerification::SystemDefaults));
        assert!(resolved.no_session_cache);

        match resolved.auth {
            AuthCredentials::Cloud { host_id, .. } => {
                assert_eq!(host_id, "host-from-flag");
            }
            other => panic!("expected cloud auth, got {other:?}"),
        }
    }

    #[test]
    fn resolve_profile_honors_insecure_flag_over_unset_profile() {
        // The TUI launch path resolves profiles through this function; the
        // -k/--insecure flag must win when the profile leaves TLS unset
        // (issue #25: flag was dropped, TLS failed, TUI sat disconnected).
        let mut profile = cloud_profile();
        profile.auth_mode = "integration".into();
        profile.controller = "https://10.0.1.1".into();
        profile.insecure = None;

        let global = base_global();
        assert!(global.insecure);

        let resolved =
            resolve_profile(&profile, "default", &global).expect("profile should resolve");

        assert!(matches!(resolved.tls, TlsVerification::DangerAcceptInvalid));
    }

    #[test]
    fn resolve_profile_cloud_requires_host_id() {
        let profile = cloud_profile();
        let mut global = base_global();
        global.host_id = None;

        let resolved = resolve_profile(&profile, "cloud", &global)
            .expect("profile fallback host id should still resolve");

        match resolved.auth {
            AuthCredentials::Cloud { host_id, .. } => {
                assert_eq!(host_id, "host-from-profile");
            }
            other => panic!("expected cloud auth, got {other:?}"),
        }
    }
}
