//! CLI-specific configuration resolution.
//!
//! Adds `GlobalOpts` flag overrides on top of the shared config module.

use std::time::Duration;

use secrecy::SecretString;

use unifly_api::{AuthCredentials, ControllerConfig, TlsVerification, TotpProvider};

use crate::cli::args::GlobalOpts;
use crate::cli::error::CliError;
use crate::config::{
    self, Config, ConfigError, Profile, resolve_totp_provider, resolve_totp_token,
};

// ── Re-exports for convenience ──────────────────────────────────

pub use crate::config::{Defaults, config_path, load_config, save_config};

// ── CLI-specific helpers ────────────────────────────────────────

/// Fallback request timeout when neither `--timeout` nor the profile sets one.
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

// This inherent impl lives here rather than in `cli/args/common.rs`
// because build.rs includes args.rs standalone for man-page generation,
// and that compilation unit must stay pure clap definitions.
impl GlobalOpts {
    /// Resolve the effective timeout: flag/env, then profile, then
    /// `[defaults]`, then the builtin fallback.
    ///
    /// Callers without a loaded config pass `None` for `defaults_timeout`.
    #[must_use]
    pub fn timeout_secs(&self, profile_timeout: Option<u64>, defaults_timeout: Option<u64>) -> u64 {
        self.timeout
            .or(profile_timeout)
            .or(defaults_timeout)
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
    }
}

/// Why `cfg` has no profile called `profile_name`: there is no config file
/// yet, or the file there does not define that profile.
pub fn missing_profile(cfg: &Config, profile_name: &str) -> CliError {
    let path = config_path();
    if path.exists() {
        ConfigError::profile_not_defined(cfg, profile_name).into()
    } else {
        CliError::NoConfig {
            path: path.display().to_string(),
        }
    }
}

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
/// Precedence: CLI flags > env > profile > `[defaults]` > builtin.
pub fn resolve_profile(
    profile: &Profile,
    profile_name: &str,
    global: &GlobalOpts,
    defaults: &Defaults,
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

    // 3. TLS verification (flag/env > profile insecure > profile ca_cert
    //    > defaults insecure). An explicit `--insecure=false` re-enables
    //    verification, and a profile CA is still verification, so only an
    //    explicit insecure=true may skip it; [defaults].insecure applies
    //    when neither flag nor profile decided and no CA is configured.
    let explicit_insecure = global.insecure.or(profile.insecure);
    let tls = if is_cloud {
        TlsVerification::SystemDefaults
    } else if explicit_insecure.unwrap_or(false) {
        TlsVerification::DangerAcceptInvalid
    } else if let Some(ref ca_path) = profile.ca_cert {
        TlsVerification::CustomCa(ca_path.clone())
    } else if explicit_insecure.unwrap_or(defaults.insecure) {
        TlsVerification::DangerAcceptInvalid
    } else {
        TlsVerification::SystemDefaults
    };

    // 4. Site (flag > env > profile)
    let site = global.site.as_deref().unwrap_or(&profile.site).to_string();

    // 5. Timeout (flag/env > profile > defaults > builtin)
    let timeout = Duration::from_secs(global.timeout_secs(profile.timeout, Some(defaults.timeout)));

    // 6. TOTP: see `resolve_totp`.
    let (totp_token, totp_provider) = resolve_totp(profile, global)?;

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
        totp_provider,
        profile_name: Some(profile_name.to_owned()),
        no_session_cache: global.no_cache || is_cloud,
    })
}

/// Resolve where a login with `profile`'s session credentials gets its TOTP
/// codes, as `(code, provider)` for `ControllerConfig`.
///
/// The code is explicit and single-use: `--totp` / `UNIFI_TOTP`, else the
/// profile's `totp_env`. It serves the first login only. The provider comes
/// from the profile's `totp_command` and runs for every re-login, and for
/// the first login too when there is no explicit code.
pub fn resolve_totp(
    profile: &Profile,
    global: &GlobalOpts,
) -> Result<(Option<SecretString>, Option<TotpProvider>), ConfigError> {
    let code = global
        .totp
        .as_ref()
        .map(|totp| SecretString::from(totp.clone()))
        .or_else(|| resolve_totp_token(profile));
    Ok((code, resolve_totp_provider(profile)?))
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
    use std::time::Duration;

    use super::resolve_profile;
    use crate::cli::args::{ColorMode, GlobalOpts, OutputFormat};
    use crate::config::{Defaults, Profile};
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
            demo_fleet: false,
            demo_seed: None,
            insecure: Some(true),
            timeout: None,
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
            totp_command: None,
            ca_cert: None,
            insecure: Some(true),
            timeout: None,
        }
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
    fn resolve_profile_supports_cloud_defaults_and_flag_override() {
        let profile = cloud_profile();
        let global = base_global();

        let resolved = resolve_profile(&profile, "cloud", &global, &Defaults::default())
            .expect("cloud profile should resolve");

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
        let profile = direct_profile();

        let global = base_global();
        assert_eq!(global.insecure, Some(true));

        let resolved = resolve_profile(&profile, "default", &global, &Defaults::default())
            .expect("profile should resolve");

        assert!(matches!(resolved.tls, TlsVerification::DangerAcceptInvalid));
    }

    #[test]
    fn resolve_profile_applies_defaults_insecure_when_flag_and_profile_unset() {
        let profile = direct_profile();
        let mut global = base_global();
        global.insecure = None;

        let defaults = Defaults {
            insecure: true,
            ..Defaults::default()
        };

        let resolved = resolve_profile(&profile, "default", &global, &defaults)
            .expect("profile should resolve");

        assert!(matches!(resolved.tls, TlsVerification::DangerAcceptInvalid));
    }

    #[test]
    fn resolve_profile_profile_insecure_overrides_defaults() {
        let mut profile = direct_profile();
        profile.insecure = Some(false);

        let mut global = base_global();
        global.insecure = None;

        let defaults = Defaults {
            insecure: true,
            ..Defaults::default()
        };

        let resolved = resolve_profile(&profile, "default", &global, &defaults)
            .expect("profile should resolve");

        assert!(matches!(resolved.tls, TlsVerification::SystemDefaults));
    }

    #[test]
    fn resolve_profile_insecure_flag_overrides_profile_and_defaults() {
        let mut profile = direct_profile();
        profile.insecure = Some(false);

        let global = base_global();
        assert_eq!(global.insecure, Some(true));

        let resolved = resolve_profile(&profile, "default", &global, &Defaults::default())
            .expect("profile should resolve");

        assert!(matches!(resolved.tls, TlsVerification::DangerAcceptInvalid));
    }

    #[test]
    fn resolve_profile_insecure_flag_false_overrides_profile_and_defaults() {
        let mut profile = direct_profile();
        profile.insecure = Some(true);

        let mut global = base_global();
        global.insecure = Some(false);

        let defaults = Defaults {
            insecure: true,
            ..Defaults::default()
        };

        let resolved = resolve_profile(&profile, "default", &global, &defaults)
            .expect("profile should resolve");

        assert!(matches!(resolved.tls, TlsVerification::SystemDefaults));
    }

    #[test]
    fn resolve_profile_profile_ca_cert_beats_defaults_insecure() {
        let mut profile = direct_profile();
        profile.ca_cert = Some("/certs/controller-ca.pem".into());

        let mut global = base_global();
        global.insecure = None;

        let defaults = Defaults {
            insecure: true,
            ..Defaults::default()
        };

        let resolved = resolve_profile(&profile, "default", &global, &defaults)
            .expect("profile should resolve");

        assert!(matches!(resolved.tls, TlsVerification::CustomCa(_)));
    }

    #[test]
    fn resolve_profile_applies_defaults_timeout_when_flag_and_profile_unset() {
        let profile = direct_profile();
        let global = base_global();
        assert!(global.timeout.is_none());

        let defaults = Defaults {
            timeout: 77,
            ..Defaults::default()
        };

        let resolved = resolve_profile(&profile, "default", &global, &defaults)
            .expect("profile should resolve");

        assert_eq!(resolved.timeout, Duration::from_secs(77));
    }

    #[test]
    fn resolve_profile_profile_timeout_overrides_defaults() {
        let mut profile = direct_profile();
        profile.timeout = Some(45);

        let global = base_global();

        let defaults = Defaults {
            timeout: 77,
            ..Defaults::default()
        };

        let resolved = resolve_profile(&profile, "default", &global, &defaults)
            .expect("profile should resolve");

        assert_eq!(resolved.timeout, Duration::from_secs(45));
    }

    #[test]
    fn resolve_profile_timeout_flag_overrides_profile_and_defaults() {
        let mut profile = direct_profile();
        profile.timeout = Some(45);

        let mut global = base_global();
        global.timeout = Some(5);

        let defaults = Defaults {
            timeout: 77,
            ..Defaults::default()
        };

        let resolved = resolve_profile(&profile, "default", &global, &defaults)
            .expect("profile should resolve");

        assert_eq!(resolved.timeout, Duration::from_secs(5));
    }

    fn mfa_session_profile(controller: &str) -> Profile {
        let mut profile = direct_profile();
        profile.auth_mode = "session".into();
        profile.controller = controller.into();
        profile.api_key = None;
        profile.host_id = None;
        profile.username = Some("admin".into());
        profile.password = Some("pw".into());
        profile.totp_command = Some(vec!["sh".into(), "-c".into(), "echo 246810".into()]);
        profile
    }

    fn session_global() -> GlobalOpts {
        let mut global = base_global();
        global.api_key = None;
        global.host_id = None;
        // Keep tests away from the real session cache.
        global.no_cache = true;
        global
    }

    #[test]
    fn explicit_totp_code_serves_the_first_login_and_the_command_every_relogin() {
        use secrecy::ExposeSecret;

        let profile = mfa_session_profile("https://10.0.1.1");
        let mut global = session_global();
        global.totp = Some("111111".into());

        let resolved = resolve_profile(&profile, "default", &global, &Defaults::default())
            .expect("profile should resolve");

        assert_eq!(
            resolved
                .totp_token
                .as_ref()
                .map(ExposeSecret::expose_secret),
            Some("111111")
        );
        assert!(resolved.totp_provider.is_some());
    }

    #[test]
    fn totp_command_alone_feeds_every_login() {
        let profile = mfa_session_profile("https://10.0.1.1");

        let resolved =
            resolve_profile(&profile, "default", &session_global(), &Defaults::default())
                .expect("profile should resolve");

        assert!(resolved.totp_token.is_none());
        assert!(resolved.totp_provider.is_some());
    }

    #[test]
    fn totp_command_without_a_program_is_rejected() {
        for argv in [Vec::new(), vec![" ".to_owned()]] {
            let mut profile = mfa_session_profile("https://10.0.1.1");
            profile.totp_command = Some(argv);

            let error =
                resolve_profile(&profile, "default", &session_global(), &Defaults::default())
                    .expect_err("an empty totp_command must not resolve");

            assert!(error.to_string().contains("totp_command"), "{error}");
        }
    }

    /// The command's output must reach the MFA step of a real login: a
    /// profile resolved here connects to a controller that challenges.
    #[cfg(unix)]
    #[tokio::test]
    async fn totp_command_code_completes_the_mfa_login() {
        use wiremock::matchers::{body_partial_json, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/login"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/auth/login"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/login"))
            .and(body_partial_json(
                serde_json::json!({ "ubic_2fa_token": "246810" }),
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Set-Cookie", "TOKEN=mfa; Path=/")
                    .set_body_json(serde_json::json!({})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/login"))
            .respond_with(ResponseTemplate::new(499).set_body_json(serde_json::json!({
                "errors": ["ubic_2fa_token required"],
                "token": "UBIC_2FA=challenge",
            })))
            .expect(1)
            .mount(&server)
            .await;

        let profile = mfa_session_profile(&server.uri());
        let resolved =
            resolve_profile(&profile, "default", &session_global(), &Defaults::default())
                .expect("profile should resolve");
        let controller = unifly_api::Controller::new(resolved);
        controller
            .connect_lightweight()
            .await
            .expect("the command's code completes the MFA login");
        controller.disconnect().await;

        server.verify().await;
    }

    #[test]
    fn resolve_profile_cloud_requires_host_id() {
        let profile = cloud_profile();
        let mut global = base_global();
        global.host_id = None;

        let resolved = resolve_profile(&profile, "cloud", &global, &Defaults::default())
            .expect("profile fallback host id should still resolve");

        match resolved.auth {
            AuthCredentials::Cloud { host_id, .. } => {
                assert_eq!(host_id, "host-from-profile");
            }
            other => panic!("expected cloud auth, got {other:?}"),
        }
    }
}
