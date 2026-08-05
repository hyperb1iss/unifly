//! `unifly` — kubectl-style CLI for managing UniFi Network controllers.

use clap::Parser;
use tracing_subscriber::EnvFilter;

use unifly::cli::args::{Cli, Command, ConfigCommand, EventsCommand, GlobalOpts};
use unifly::cli::commands;
use unifly::cli::error::CliError;
use unifly::config::resolve;

use unifly_api::Controller;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if let Err(err) = run(cli).await {
        let code = err.exit_code();
        eprintln!("{:?}", miette::Report::new(err));
        std::process::exit(code);
    }
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
async fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Cloud(args) => {
            init_tracing(cli.global.verbose);
            commands::cloud::handle(args, &cli.global).await
        }

        Command::Config(args) => {
            if matches!(&args.command, ConfigCommand::CloudSetup) {
                init_tracing(cli.global.verbose);
            }
            commands::config_cmd::handle(args, &cli.global).await
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
            unifly::tui::launch(&cli.global, args)
                .await
                .map_err(|e| CliError::ApiError {
                    code: "tui".into(),
                    message: e.to_string(),
                    request_id: None,
                })
        }

        cmd => {
            init_tracing(cli.global.verbose);
            let mut controller_config = build_controller_config(&cli.global).await?;
            controller_config.websocket_enabled = command_uses_websocket(&cmd);
            let controller = Controller::new(controller_config);
            if command_needs_initial_refresh(&cmd) {
                controller.connect().await.map_err(CliError::from)?;
            } else {
                controller
                    .connect_lightweight()
                    .await
                    .map_err(CliError::from)?;
            }
            for warning in controller.take_warnings().await {
                if !cli.global.quiet {
                    eprintln!("warning: {warning}");
                }
            }

            tracing::debug!(command = ?cmd, "dispatching command");
            let result = commands::dispatch(cmd, &controller, &cli.global).await;

            controller.disconnect().await;
            result
        }
    }
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
        reason: format!("invalid URL: {url_str}"),
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
    } else if global.insecure {
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
