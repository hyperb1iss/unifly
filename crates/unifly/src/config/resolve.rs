//! CLI-specific configuration resolution.
//!
//! Adds `GlobalOpts` flag overrides on top of the shared config module.

use secrecy::SecretString;

use unifly_api::ControllerConfig;

use crate::cli::args::GlobalOpts;
use crate::cli::error::CliError;
use crate::config::{self, Config, ConfigError, Profile};

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
    resolve_connection(
        profile,
        Some(profile_name),
        global,
        defaults,
        config::ConnectionUsage::Command,
    )
}

/// Resolve a launch connection, discovering a missing Cloud console once.
/// Saved and explicit console IDs bypass discovery in both user interfaces.
pub async fn resolve_startup(
    cfg: &Config,
    global: &GlobalOpts,
    usage: config::ConnectionUsage,
) -> Result<ControllerConfig, CliError> {
    let (mut profile, name) = connection_profile(cfg, global)?;
    if profile.auth_mode == "cloud"
        && global.host_id.is_none()
        && config::resolve_host_id(&profile).is_err()
    {
        profile.host_id = Some(
            crate::cli::commands::cloud::discover_profile_host(
                &profile,
                name.as_deref().unwrap_or("default"),
                global,
                &cfg.defaults,
            )
            .await?,
        );
        profile.host_id_env = None;
    }
    resolve_connection(&profile, name.as_deref(), global, &cfg.defaults, usage)
}

/// Apply flag overrides before the shared credential and transport resolution.
pub fn resolve_connection(
    profile: &Profile,
    profile_name: Option<&str>,
    global: &GlobalOpts,
    defaults: &Defaults,
    usage: config::ConnectionUsage,
) -> Result<ControllerConfig, CliError> {
    let effective = normalized_profile(profile, global);
    let mut resolved = config::controller::build(&effective, profile_name, defaults, usage)?;
    if let Some(token) = &global.totp {
        resolved.totp_token = Some(SecretString::from(token.clone()));
    }
    resolved.no_session_cache |= global.no_cache;
    Ok(resolved)
}

/// Select a saved profile, or construct an ad-hoc profile from explicit flags.
/// The returned optional name controls persistent session caching.
pub fn connection_profile(
    cfg: &Config,
    global: &GlobalOpts,
) -> Result<(Profile, Option<String>), CliError> {
    let name = active_profile_name(global, cfg);
    if let Some(profile) = cfg.profiles.get(&name) {
        return Ok((normalized_profile(profile, global), Some(name)));
    }
    let cloud =
        global.host_id.is_some() || controller_points_to_cloud(global.controller.as_deref());
    let controller = global
        .controller
        .clone()
        .or_else(|| cloud.then(|| config::DEFAULT_CLOUD_CONTROLLER_URL.into()))
        .ok_or_else(|| missing_profile(cfg, &name))?;
    let api_key = global.api_key.clone().ok_or_else(CliError::no_api_key)?;
    let profile = Profile {
        controller,
        site: "default".into(),
        auth_mode: if cloud { "cloud" } else { "integration" }.into(),
        api_key: Some(api_key),
        api_key_env: None,
        host_id: global.host_id.clone(),
        host_id_env: None,
        username: None,
        password: None,
        totp_env: None,
        totp_command: None,
        ca_cert: None,
        insecure: None,
        timeout: None,
    };
    Ok((normalized_profile(&profile, global), None))
}

pub fn controller_points_to_cloud(controller: Option<&str>) -> bool {
    controller
        .and_then(|value| url::Url::parse(value).ok())
        .is_some_and(|url| url.host_str() == Some("api.ui.com"))
}

/// Normalize auth and its target together before discovery or connection setup.
/// Applying the same flags again leaves the selected connection unchanged.
fn normalized_profile(profile: &Profile, global: &GlobalOpts) -> Profile {
    let mut effective = profile.clone();
    if global.api_key.is_some() {
        if global.host_id.is_some() || controller_points_to_cloud(global.controller.as_deref()) {
            if profile.auth_mode != "cloud" && global.controller.is_none() {
                effective.controller = config::DEFAULT_CLOUD_CONTROLLER_URL.into();
            }
            effective.auth_mode = "cloud".into();
        } else if global.controller.is_some()
            || matches!(profile.auth_mode.as_str(), "session" | "legacy")
        {
            effective.auth_mode = "integration".into();
        }
    }
    if let Some(url) = &global.controller {
        effective.controller.clone_from(url);
    }
    if let Some(site) = &global.site {
        effective.site.clone_from(site);
    }
    if let Some(key) = &global.api_key {
        effective.api_key = Some(key.clone());
        effective.api_key_env = None;
    }
    if let Some(host_id) = &global.host_id {
        effective.host_id = Some(host_id.clone());
        effective.host_id_env = None;
    }
    effective.insecure = global.insecure.or(profile.insecure);
    effective.timeout = global.timeout.or(profile.timeout);
    effective
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
            host_id: None,
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

    #[tokio::test]
    async fn startup_discovery_reports_missing_and_ambiguous_consoles_for_both_interfaces() {
        use crate::config::{Config, ConnectionUsage};
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        for hosts in [
            serde_json::json!([]),
            serde_json::json!([{"id":"a"},{"id":"b"}]),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/v1/hosts"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({"data":hosts})),
                )
                .expect(2)
                .mount(&server)
                .await;
            let mut profile = cloud_profile();
            profile.controller = server.uri();
            profile.host_id = None;
            let mut cfg = Config::default();
            cfg.profiles.insert("default".into(), profile);
            for usage in [ConnectionUsage::Command, ConnectionUsage::Interactive] {
                let error = super::resolve_startup(&cfg, &base_global(), usage)
                    .await
                    .expect_err("missing console");
                assert!(
                    matches!(error, crate::cli::error::CliError::Validation { field, .. } if field == "host_id")
                );
            }
            server.verify().await;
        }
    }

    #[test]
    fn direct_key_and_controller_never_use_a_saved_password() {
        use super::resolve_connection;
        use crate::config::ConnectionUsage;
        for mode in ["session", "legacy", "hybrid", "cloud"] {
            let mut profile = direct_profile();
            profile.auth_mode = mode.into();
            profile.username = Some("saved-user".into());
            profile.password = Some("saved-password".into());
            let mut flags = base_global();
            flags.host_id = None;
            flags.controller = Some("https://other.invalid".into());
            for usage in [ConnectionUsage::Command, ConnectionUsage::Interactive] {
                let resolved = resolve_connection(
                    &profile,
                    Some("fixture"),
                    &flags,
                    &Defaults::default(),
                    usage,
                )
                .expect("direct key");
                assert!(
                    matches!(resolved.auth, AuthCredentials::ApiKey(_)),
                    "saved {mode} credentials must not accompany an explicit direct key"
                );
            }
        }
    }

    #[test]
    fn cloud_override_replaces_the_saved_lan_target_before_and_after_selection() {
        use super::{connection_profile, resolve_connection};
        use crate::config::{Config, ConnectionUsage};
        let mut cfg = Config::default();
        let mut flags = base_global();
        flags.host_id = Some("explicit-console".into());
        for mode in ["integration", "session", "legacy", "hybrid"] {
            let mut saved = direct_profile();
            saved.auth_mode = mode.into();
            cfg.profiles.insert("default".into(), saved.clone());
            let (selected, name) = connection_profile(&cfg, &flags).expect("selection");
            for profile in [&saved, &selected] {
                let resolved = resolve_connection(
                    profile,
                    name.as_deref(),
                    &flags,
                    &cfg.defaults,
                    ConnectionUsage::Command,
                )
                .expect("cloud connection");
                assert_eq!(resolved.url.as_str(), "https://api.ui.com/", "saved {mode}");
                assert!(matches!(resolved.auth, AuthCredentials::Cloud { .. }));
            }
        }
    }

    #[test]
    fn one_shot_key_and_cloud_intent_select_auth_before_discovery() {
        use super::{connection_profile, normalized_profile};
        use crate::config::Config;
        let mut flags = base_global();
        for (saved, expected) in [
            ("session", "integration"),
            ("legacy", "integration"),
            ("hybrid", "hybrid"),
            ("cloud", "cloud"),
        ] {
            let mut profile = direct_profile();
            profile.auth_mode = saved.into();
            assert_eq!(normalized_profile(&profile, &flags).auth_mode, expected);
            flags.api_key = None;
            flags.controller = Some("https://other.invalid".into());
            assert_eq!(normalized_profile(&profile, &flags).auth_mode, saved);
            flags = base_global();
        }
        let mut cfg = Config::default();
        cfg.profiles.insert("default".into(), cloud_profile());
        flags.controller = Some("https://other.invalid".into());
        let (profile, _) = connection_profile(&cfg, &flags).expect("direct selection");
        assert_eq!(
            profile.auth_mode, "integration",
            "must skip saved cloud discovery"
        );
        flags.host_id = Some("explicit-console".into());
        assert_eq!(normalized_profile(&profile, &flags).auth_mode, "cloud");
        flags.host_id = None;
        flags.controller = Some("https://api.ui.com".into());
        assert_eq!(normalized_profile(&profile, &flags).auth_mode, "cloud");
    }

    #[test]
    fn commands_and_dashboards_share_transport_precedence() {
        use super::{connection_profile, resolve_connection};
        use crate::config::{Config, ConnectionUsage};
        use secrecy::ExposeSecret;

        let mut config = Config::default();
        config.defaults.timeout = 71;
        config.defaults.insecure = true;
        let mut profile = direct_profile();
        profile.site = "saved-site".into();
        profile.ca_cert = Some("/fixture/ca.pem".into());
        profile.auth_mode = "hybrid".into();
        profile.username = Some("fixture-user".into());
        profile.password = Some("fixture-password".into());
        profile.totp_command = Some(vec!["fixture-totp".into()]);
        config.profiles.insert("default".into(), profile);
        let mut flags = base_global();
        flags.insecure = None;
        flags.host_id = None;
        flags.totp = Some("654321".into());
        flags.no_cache = true;
        let (profile, name) = connection_profile(&config, &flags).expect("select");
        for usage in [ConnectionUsage::Command, ConnectionUsage::Interactive] {
            let resolved =
                resolve_connection(&profile, name.as_deref(), &flags, &config.defaults, usage)
                    .expect("resolve");
            assert_eq!(resolved.url.as_str(), "https://10.0.1.1/");
            assert_eq!(resolved.site, "saved-site");
            assert_eq!(resolved.timeout, Duration::from_secs(71));
            assert!(matches!(resolved.tls, TlsVerification::CustomCa(_)));
            let AuthCredentials::Hybrid {
                api_key,
                username,
                password,
            } = resolved.auth
            else {
                panic!("hybrid auth lost")
            };
            assert_eq!(api_key.expose_secret(), "flag-api-key");
            assert_eq!(username, "fixture-user");
            assert_eq!(password.expose_secret(), "fixture-password");
            assert_eq!(
                resolved.totp_token.expect("token").expose_secret(),
                "654321"
            );
            assert!(resolved.totp_provider.is_some());
            assert!(resolved.no_session_cache);
        }
    }

    #[test]
    fn ad_hoc_connections_honor_defaults_without_a_cache_profile() {
        use super::{connection_profile, resolve_connection};
        use crate::config::{Config, ConnectionUsage};
        let mut config = Config::default();
        config.defaults.timeout = 91;
        config.defaults.insecure = true;
        let mut flags = base_global();
        flags.controller = Some("https://adhoc.local".into());
        flags.host_id = None;
        flags.insecure = None;
        let (profile, name) = connection_profile(&config, &flags).expect("select");
        assert!(name.is_none());
        for usage in [ConnectionUsage::Command, ConnectionUsage::Interactive] {
            let resolved =
                resolve_connection(&profile, name.as_deref(), &flags, &config.defaults, usage)
                    .expect("resolve");
            assert!(resolved.profile_name.is_none());
            assert_eq!(resolved.timeout, Duration::from_secs(91));
            assert!(matches!(resolved.tls, TlsVerification::DangerAcceptInvalid));
        }
        flags.controller = Some("https://api.ui.com".into());
        let (profile, _) = connection_profile(&config, &flags).expect("cloud select");
        assert_eq!(profile.auth_mode, "cloud");
    }

    /// A controller URL can carry a password (`https://admin:pw@host`).
    /// When it does not parse, the error names it without that part.
    #[test]
    fn an_unparseable_controller_url_is_reported_without_its_password() {
        let mut profile = direct_profile();
        profile.controller = "https://admin:hunter2@udm.lan:99999".into();

        let message = resolve_profile(&profile, "default", &base_global(), &Defaults::default())
            .expect_err("the port is out of range")
            .to_string();

        assert!(!message.contains("hunter2"), "{message}");
        assert!(message.contains("https://****@udm.lan:99999"), "{message}");
    }

    #[test]
    fn resolve_profile_supports_cloud_defaults_and_flag_override() {
        let profile = cloud_profile();
        let mut global = base_global();
        global.host_id = Some("host-from-flag".into());

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
