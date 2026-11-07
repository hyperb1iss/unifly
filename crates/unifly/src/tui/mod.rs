pub mod action;
pub mod app;
pub mod component;
pub mod data_bridge;
mod demo_fleet;
pub mod effects;
pub mod event;
pub(crate) mod forms;
#[cfg(feature = "tui-graphics")]
pub mod graphics;
pub mod render_caps;
mod render_scheduler;
pub mod screen;
pub mod screens;
pub mod table;
pub mod terminal;
#[allow(dead_code)]
pub mod theme;
pub mod widgets;

use std::sync::Arc;

use color_eyre::eyre::Result;
use tracing::info;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use unifly_api::Controller;

use crate::cli::args::{GlobalOpts, TuiArgs};
use crate::config;
use crate::sanitizer::Sanitizer;

/// Launch the real-time terminal dashboard.
///
/// Sets up file-based tracing, installs panic hooks, initializes the theme,
/// builds a controller (with graceful fallback), and runs the TUI app loop.
#[allow(clippy::future_not_send)]
pub async fn launch(global: &GlobalOpts, args: TuiArgs) -> Result<()> {
    terminal::install_hooks()?;

    let _log_guard = setup_tracing(global.verbose, &args.log_file);

    // A config that fails to load is reported, not taken for a first run:
    // the setup wizard would only refuse to save over it.
    let mut load_error = None;
    let loaded_config = demo_fleet::startup_config(global, || match config::load_config() {
        Ok(config) => Some(config),
        Err(error) => {
            load_error = Some(error);
            None
        }
    });
    if let Some(error) = load_error {
        return Err(error.into());
    }
    let theme_name = global.theme.as_deref().or_else(|| {
        loaded_config
            .as_ref()
            .and_then(|config| config.defaults.theme.as_deref())
    });
    theme::initialize(theme_name);
    render_caps::initialize(
        loaded_config
            .as_ref()
            .and_then(|config| config.defaults.chart_quality.as_deref()),
    );

    let url = global.controller.as_deref().map_or_else(
        || "(not set)".to_owned(),
        crate::config::redact_url_userinfo,
    );
    info!(
        url = url.as_str(),
        site = global.site.as_deref().unwrap_or("default"),
        "starting unifly tui"
    );

    let cfg = loaded_config.as_ref();
    let (controller, demo_fleet) =
        demo_fleet::select_controller(global, || build_controller_from_config(global, cfg));

    // The demo fleet is fictional already; sanitizing it would only scramble it.
    let sanitizer = (!demo_fleet)
        .then(|| resolve_sanitizer(global, cfg))
        .flatten();
    let effects_enabled = resolve_effects_enabled(global, cfg);
    let show_donate = cfg.is_none_or(|c| c.defaults.show_donate);

    let session_profile = session_profile_name(global, cfg);
    screens::settings::set_session_profile(&session_profile);
    let mut app = app::App::new(controller, sanitizer, effects_enabled, show_donate)
        .with_setup_profile(&session_profile, cfg);
    app.run().await?;

    Ok(())
}

/// The profile this session runs on (`--profile`, else the config's default,
/// else `default`). Onboarding sets it up and Settings edits it, so neither
/// writes over another profile.
fn session_profile_name(global: &GlobalOpts, cfg: Option<&config::Config>) -> String {
    global
        .profile
        .clone()
        .or_else(|| cfg.and_then(|cfg| cfg.default_profile.clone()))
        .unwrap_or_else(|| "default".into())
}

/// Resolve whether TUI effects should run this session.
///
/// Resolution order (first "off" wins):
///   `--no-effects` flag → `NO_EFFECTS` env var → `[defaults].effects` config → default on.
fn resolve_effects_enabled(global: &GlobalOpts, cfg: Option<&config::Config>) -> bool {
    if global.no_effects {
        return false;
    }
    if std::env::var_os("NO_EFFECTS").is_some() {
        return false;
    }
    cfg.is_none_or(|c| c.defaults.effects)
}

fn setup_tracing(verbosity: u8, log_file: &std::path::Path) -> WorkerGuard {
    let log_level = match verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("unifly={log_level},unifly_api={log_level}")));

    let log_dir = log_file.parent().unwrap_or(std::path::Path::new("/tmp"));
    let log_filename = log_file
        .file_name()
        .unwrap_or(std::ffi::OsStr::new("unifly-tui.log"));

    let file_appender = tracing_appender::rolling::never(log_dir, log_filename);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_target(true)
                .with_thread_ids(true),
        )
        .init();

    guard
}

/// Build the dashboard with the same profile selection and overrides as the CLI.
fn build_controller_from_config(
    global: &GlobalOpts,
    cfg: Option<&config::Config>,
) -> Option<Controller> {
    let defaults = config::Config::default();
    let cfg = cfg.unwrap_or(&defaults);
    let result = config::resolve::connection_profile(cfg, global).and_then(|(profile, name)| {
        config::resolve::resolve_connection(
            &profile,
            name.as_deref(),
            global,
            &cfg.defaults,
            config::ConnectionUsage::Interactive,
        )
    });
    match result {
        Ok(config) => Some(Controller::new(config)),
        Err(error) => {
            tracing::warn!(%error, "failed to resolve controller configuration");
            None
        }
    }
}

/// Resolve demo-mode PII sanitization from the `--demo` flag and config,
/// with the flag forcing it on. Returns `None` when demo mode is off.
fn resolve_sanitizer(global: &GlobalOpts, cfg: Option<&config::Config>) -> Option<Arc<Sanitizer>> {
    let mut demo_config = cfg.map(|c| c.demo.clone()).unwrap_or_default();

    if global.demo {
        demo_config.enabled = true;
    }

    if demo_config.enabled {
        info!("demo mode active — PII will be sanitized");
        Some(Arc::new(Sanitizer::new(&demo_config)))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::session_profile_name;
    use crate::cli::args::{Cli, GlobalOpts};
    use crate::config::Config;

    fn global(argv: &[&str]) -> GlobalOpts {
        Cli::try_parse_from(argv).expect("valid arguments").global
    }

    #[test]
    fn the_session_runs_on_the_profile_the_launch_selected() {
        let cfg = Config {
            default_profile: Some("home".into()),
            ..Config::default()
        };

        let work = global(&["unifly", "tui", "-p", "work"]);
        assert_eq!(session_profile_name(&work, Some(&cfg)), "work");
        let plain = global(&["unifly", "tui"]);
        assert_eq!(session_profile_name(&plain, Some(&cfg)), "home");
        assert_eq!(session_profile_name(&plain, None), "default");
    }

    #[test]
    fn direct_flags_keep_saved_profile_defaults_in_the_dashboard() {
        let mut cfg = toml::from_str::<Config>(
            "[defaults]\ntimeout = 79\ninsecure = true\n\
             [profiles.default]\ncontroller = 'https://saved.local'\n\
             site = 'saved-site'\napi_key = 'fixture-key'\nca_cert = '/fixture/ca.pem'\n",
        )
        .expect("fixture");
        let flags = global(&[
            "unifly",
            "tui",
            "--controller",
            "https://override.local",
            "--api-key",
            "override-key",
        ]);
        for mode in ["integration", "session", "legacy", "hybrid", "cloud"] {
            let profile = cfg.profiles.get_mut("default").expect("fixture profile");
            profile.auth_mode = mode.into();
            profile.username = Some("saved-user".into());
            profile.password = Some("saved-password".into());
            let controller =
                super::build_controller_from_config(&flags, Some(&cfg)).expect("controller");
            assert_eq!(controller.config().timeout.as_secs(), 79);
            assert_eq!(controller.config().site, "saved-site");
            assert_eq!(controller.config().url.as_str(), "https://override.local/");
            assert!(matches!(
                controller.config().tls,
                unifly_api::TlsVerification::CustomCa(_)
            ));
            assert!(controller.config().websocket_enabled);
            assert_eq!(controller.config().refresh_interval_secs, 10);
            assert!(
                matches!(
                    controller.config().auth,
                    unifly_api::AuthCredentials::ApiKey(_)
                ),
                "an explicit direct key must not use saved {mode} credentials"
            );
        }
    }

    #[tokio::test]
    async fn cloud_flags_do_not_send_a_key_to_the_saved_lan_controller() {
        let lan = wiremock::MockServer::start().await;
        let mut cfg = toml::from_str::<Config>(&format!(
            "[profiles.default]\ncontroller = '{}'\nauth_mode = 'session'\n\
             username = 'saved-user'\npassword = 'saved-password'\n",
            lan.uri()
        ))
        .expect("fixture");
        for mode in ["integration", "session", "legacy", "hybrid", "cloud"] {
            let profile = cfg.profiles.get_mut("default").expect("profile");
            profile.auth_mode = mode.into();
            profile.controller = if mode == "cloud" {
                "https://saved-cloud.invalid".into()
            } else {
                lan.uri()
            };
            for custom in [false, true] {
                let mut argv = vec![
                    "unifly",
                    "tui",
                    "--api-key",
                    "one-shot-key",
                    "--host-id",
                    "console",
                ];
                if custom {
                    argv.extend(["--controller", "https://custom-cloud.invalid"]);
                }
                let flags = global(&argv);
                let controller =
                    super::build_controller_from_config(&flags, Some(&cfg)).expect("cloud");
                let expected = if custom {
                    "https://custom-cloud.invalid/"
                } else if mode == "cloud" {
                    "https://saved-cloud.invalid/"
                } else {
                    "https://api.ui.com/"
                };
                assert_eq!(controller.config().url.as_str(), expected, "saved {mode}");
                assert!(matches!(
                    controller.config().auth,
                    unifly_api::AuthCredentials::Cloud { .. }
                ));
            }
        }
        assert!(lan.received_requests().await.expect("requests").is_empty());
    }
}
