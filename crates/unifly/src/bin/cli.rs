//! `unifly` — kubectl-style CLI for managing UniFi Network controllers.

use clap::Parser;
use tracing_subscriber::EnvFilter;

use unifly::cli::args::{Cli, Command, EventsCommand, GlobalOpts};
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
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter)),
        )
        .with_target(false)
        .init();
}

#[allow(clippy::future_not_send)]
async fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Config(args) => commands::config_cmd::handle(args, &cli.global),

        Command::Completions(args) => {
            use clap::CommandFactory;
            use clap_complete::generate;

            let mut cmd = Cli::command();
            generate(args.shell, &mut cmd, "unifi", &mut std::io::stdout());
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
            let mut controller_config = build_controller_config(&cli.global)?;
            controller_config.websocket_enabled = command_uses_websocket(&cmd);
            let controller = Controller::new(controller_config);
            controller.connect().await.map_err(CliError::from)?;
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

fn build_controller_config(global: &GlobalOpts) -> Result<unifly_api::ControllerConfig, CliError> {
    let cfg = resolve::load_config_or_default();
    let profile_name = resolve::active_profile_name(global, &cfg);

    if let Some(profile) = cfg.profiles.get(&profile_name) {
        return resolve::resolve_profile(profile, &profile_name, global);
    }

    let url_str = global
        .controller
        .as_deref()
        .ok_or_else(|| CliError::NoConfig {
            path: unifly::config::config_path().display().to_string(),
        })?;

    let url: url::Url = url_str.parse().map_err(|_| CliError::Validation {
        field: "controller".into(),
        reason: format!("invalid URL: {url_str}"),
    })?;

    let auth = if let Some(ref key) = global.api_key {
        unifly_api::AuthCredentials::ApiKey(secrecy::SecretString::from(key.clone()))
    } else {
        return Err(CliError::NoCredentials {
            profile: profile_name,
        });
    };

    let tls = if global.insecure {
        unifly_api::TlsVerification::DangerAcceptInvalid
    } else {
        unifly_api::TlsVerification::SystemDefaults
    };

    Ok(unifly_api::ControllerConfig {
        url,
        auth,
        site: global.site.clone().unwrap_or_else(|| "default".into()),
        tls,
        timeout: std::time::Duration::from_secs(global.timeout),
        refresh_interval_secs: 0,
        websocket_enabled: false,
        polling_interval_secs: 30,
        totp_token: global.totp.as_ref().map(|t| secrecy::SecretString::from(t.clone())),
    })
}

#[cfg(test)]
mod tests {
    use super::command_uses_websocket;
    use unifly::cli::args::{Command, EventsArgs, EventsCommand};

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
}
