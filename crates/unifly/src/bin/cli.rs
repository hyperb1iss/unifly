//! `unifly` — kubectl-style CLI for managing UniFi Network controllers.

use clap::{CommandFactory, FromArgMatches};
use tracing_subscriber::EnvFilter;

use unifly::cli::args::{Cli, Command, ConfigCommand, EventsCommand, GlobalOpts};
use unifly::cli::commands;
use unifly::cli::demo_fleet::{self, DemoSwitch};
use unifly::cli::error::{CliError, redact_userinfo};
use unifly::config::resolve;

use unifly_api::Controller;

/// Parses the CLI, then builds the runtime by hand rather than using
/// `#[tokio::main]`, so the worker-thread count can depend on the command.
fn main() {
    let matches = Cli::command().get_matches();
    let command_path = demo_fleet::command_path(&matches);
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|err| err.exit());

    let runtime = match build_runtime(worker_threads_for(&cli.command)) {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("failed to start async runtime: {err}");
            std::process::exit(1);
        }
    };

    if let Err(err) = runtime.block_on(run(cli, &command_path)) {
        let code = err.exit_code();
        eprintln!("{:?}", miette::Report::new(err));
        std::process::exit(code);
    }
}

/// Pick the tokio worker-thread count for a command.
///
/// Tokio's default is one worker per core, which on a many-core machine
/// means dozens of idle threads and, with glibc malloc, one heap arena per
/// thread that touched the allocator. Every path in unifly is I/O bound and
/// nothing blocks the runtime, so the TUI runs on a single worker and the
/// CLI on two.
fn worker_threads_for(command: &Command) -> usize {
    match command {
        #[cfg(feature = "tui")]
        Command::Tui(_) => 1,
        _ => 2,
    }
}

/// Build the tokio runtime with an explicit worker count; see
/// [`worker_threads_for`] for why the default is not one-per-core.
fn build_runtime(worker_threads: usize) -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .thread_name("unifly-rt")
        .enable_all()
        .build()
}

fn init_tracing(verbosity: u8) {
    let filter = match verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter)),
        )
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();
}

#[allow(clippy::future_not_send)]
async fn run(cli: Cli, command_path: &str) -> Result<(), CliError> {
    // Refuse before anything reads config, the keyring, or the network.
    if cli.global.demo_fleet && !demo_fleet::supports(command_path) {
        return Err(demo_fleet::refuse(command_path));
    }
    // Demo mode comes from the flag, UNIFI_DEMO, or `[demo] enabled` in the
    // config; the config is read only for commands that would refuse it.
    if !demo_fleet::honors_demo(command_path, cli.global.demo_fleet) {
        if cli.global.demo {
            return Err(demo_fleet::refuse_demo(command_path, &DemoSwitch::Flag));
        }
        if resolve::load_config_or_default().demo.enabled {
            let file = unifly::config::config_path();
            return Err(demo_fleet::refuse_demo(
                command_path,
                &DemoSwitch::ConfigFile(file),
            ));
        }
    }

    let Cli { global, command } = cli;
    let result = run_command(command, &global).await;
    result.map_err(|err| name_profile(err, &global))
}

#[allow(clippy::future_not_send)]
async fn run_command(command: Command, global: &GlobalOpts) -> Result<(), CliError> {
    match command {
        Command::Skill(args) => unifly::cli::skill::handle(&args, global.quiet),

        Command::Cloud(args) => {
            init_tracing(global.verbose);
            commands::cloud::handle(args, global).await
        }

        Command::Config(args) => {
            if matches!(&args.command, ConfigCommand::CloudSetup) {
                init_tracing(global.verbose);
            }
            commands::config_cmd::handle(args, global).await
        }

        Command::Completions(args) => {
            use clap::CommandFactory;
            use clap_complete::generate;

            let mut cmd = Cli::command();
            generate(args.shell, &mut cmd, "unifly", &mut std::io::stdout());
            Ok(())
        }

        #[cfg(feature = "tui")]
        Command::Tui(args) => {
            unifly::tui::launch(global, args)
                .await
                .map_err(|e| CliError::ApiError {
                    code: "tui".into(),
                    message: e.to_string(),
                    request_id: None,
                })
        }

        cmd => {
            init_tracing(global.verbose);
            let controller = if global.demo_fleet {
                demo_fleet::controller(global)
            } else {
                let mut controller_config = build_controller_config(global).await?;
                commands::util::require_http_url(controller_config.url.as_str())?;
                controller_config.websocket_enabled = command_uses_websocket(&cmd);
                Controller::new(controller_config)
            };
            if command_needs_initial_refresh(&cmd) {
                controller.connect().await.map_err(CliError::from)?;
            } else {
                controller
                    .connect_lightweight()
                    .await
                    .map_err(CliError::from)?;
            }
            for warning in controller.take_warnings().await {
                if !global.quiet {
                    eprintln!("warning: {warning}");
                }
            }

            tracing::debug!(command = ?cmd, "dispatching command");
            let result = commands::dispatch(cmd, &controller, global).await;

            controller.disconnect().await;
            result
        }
    }
}

/// Point an authentication failure's hint at the profile the command used.
/// The config is only read on this failure path, and never for the demo
/// fleet, which keeps its promise not to open the config file.
fn name_profile(err: CliError, global: &GlobalOpts) -> CliError {
    if global.demo_fleet || !matches!(err, CliError::AuthFailed { .. }) {
        return err;
    }
    let cfg = resolve::load_config_or_default();
    let name = resolve::active_profile_name(global, &cfg);
    let profile = cfg.profiles.contains_key(&name).then_some(name);
    err.for_profile(profile.as_deref())
}

fn command_uses_websocket(command: &Command) -> bool {
    matches!(
        command,
        Command::Events(args) if matches!(args.command, EventsCommand::Watch { .. })
    )
}

fn command_needs_initial_refresh(command: &Command) -> bool {
    !matches!(command, Command::System(_))
}

fn controller_points_to_cloud(controller: Option<&str>) -> bool {
    controller
        .and_then(|controller| url::Url::parse(controller).ok())
        .is_some_and(|url| url.host_str() == Some("api.ui.com"))
}

async fn build_controller_config(
    global: &GlobalOpts,
) -> Result<unifly_api::ControllerConfig, CliError> {
    let cfg = resolve::load_config_or_default();
    let profile_name = resolve::active_profile_name(global, &cfg);

    if let Some(profile) = cfg.profiles.get(&profile_name) {
        if profile.auth_mode == "cloud"
            && global.host_id.is_none()
            && unifly::config::resolve_host_id(profile).is_err()
        {
            let mut resolved_profile = profile.clone();
            resolved_profile.host_id = Some(commands::cloud::auto_resolve_host_id(global).await?);
            resolved_profile.host_id_env = None;
            return resolve::resolve_profile(
                &resolved_profile,
                &profile_name,
                global,
                &cfg.defaults,
            );
        }

        return resolve::resolve_profile(profile, &profile_name, global, &cfg.defaults);
    }

    let is_cloud =
        global.host_id.is_some() || controller_points_to_cloud(global.controller.as_deref());
    let url_str = global.controller.as_deref().or({
        if is_cloud {
            Some(unifly::config::DEFAULT_CLOUD_CONTROLLER_URL)
        } else {
            None
        }
    });
    let url_str = url_str.ok_or_else(|| CliError::NoConfig {
        path: unifly::config::config_path().display().to_string(),
    })?;

    let url: url::Url = url_str.parse().map_err(|_| CliError::Validation {
        field: "controller".into(),
        reason: format!("invalid URL: {}", redact_userinfo(url_str)),
    })?;

    let auth = if let Some(ref key) = global.api_key {
        let api_key = secrecy::SecretString::from(key.clone());

        if is_cloud {
            let host_id = if let Some(host_id) = &global.host_id {
                host_id.clone()
            } else {
                commands::cloud::auto_resolve_host_id(global).await?
            };

            unifly_api::AuthCredentials::Cloud { api_key, host_id }
        } else {
            unifly_api::AuthCredentials::ApiKey(api_key)
        }
    } else {
        return Err(CliError::NoCredentials {
            profile: profile_name,
        });
    };

    let tls = if is_cloud {
        unifly_api::TlsVerification::SystemDefaults
    } else if global.insecure.unwrap_or(cfg.defaults.insecure) {
        unifly_api::TlsVerification::DangerAcceptInvalid
    } else {
        unifly_api::TlsVerification::SystemDefaults
    };

    let totp_token = global
        .totp
        .as_ref()
        .map(|t| secrecy::SecretString::from(t.clone()));

    Ok(unifly_api::ControllerConfig {
        url,
        auth,
        site: global.site.clone().unwrap_or_else(|| "default".into()),
        tls,
        timeout: std::time::Duration::from_secs(
            global.timeout_secs(None, Some(cfg.defaults.timeout)),
        ),
        refresh_interval_secs: 0,
        websocket_enabled: false,
        polling_interval_secs: 30,
        totp_token,
        totp_provider: None,
        profile_name: None, // ad-hoc flags — no profile, no caching
        no_session_cache: global.no_cache || is_cloud,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        command_needs_initial_refresh, command_uses_websocket, controller_points_to_cloud,
    };
    use unifly::cli::args::{
        BackupArgs, BackupCommand, Command, EventsArgs, EventsCommand, SystemArgs, SystemCommand,
    };

    #[test]
    fn only_events_watch_enables_websocket() {
        let watch = Command::Events(EventsArgs {
            command: EventsCommand::Watch { types: None },
        });
        let list = Command::Events(EventsArgs {
            command: EventsCommand::List {
                limit: 100,
                within: 24,
            },
        });

        assert!(command_uses_websocket(&watch));
        assert!(!command_uses_websocket(&list));
    }

    #[test]
    fn system_commands_skip_initial_refresh() {
        let info = Command::System(SystemArgs {
            command: SystemCommand::Info,
        });
        let sysinfo = Command::System(SystemArgs {
            command: SystemCommand::Sysinfo,
        });
        let backup = Command::System(SystemArgs {
            command: SystemCommand::Backup(BackupArgs {
                command: BackupCommand::List,
            }),
        });

        assert!(!command_needs_initial_refresh(&info));
        assert!(!command_needs_initial_refresh(&sysinfo));
        assert!(!command_needs_initial_refresh(&backup));
    }

    #[test]
    fn api_ui_url_is_detected_as_cloud() {
        assert!(controller_points_to_cloud(Some("https://api.ui.com")));
        assert!(!controller_points_to_cloud(Some("https://192.168.1.1")));
        assert!(!controller_points_to_cloud(None));
    }
}
