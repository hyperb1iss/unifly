//! Controller selection for the synthetic demo fleet.

use tracing::{info, warn};
use unifly_api::Controller;

use crate::cli::args::GlobalOpts;
use crate::cli::demo_fleet;
use crate::config::Config;

/// The config file for this session, if any. The demo fleet never opens it
/// (it holds real profiles and possibly plaintext secrets): theme comes from
/// `--theme` / `UNIFLY_THEME` or the default, everything else from defaults.
pub(super) fn startup_config(
    global: &GlobalOpts,
    load: impl FnOnce() -> Option<Config>,
) -> Option<Config> {
    if global.demo_fleet { None } else { load() }
}

/// Pick this session's controller and report whether it is the demo fleet.
///
/// `--demo-fleet` always gets the synthetic fleet and never touches config
/// or credentials. Otherwise the real controller is resolved, and plain
/// `--demo` falls back to the fleet when nothing could be resolved instead
/// of opening onboarding.
pub(super) fn select_controller(
    global: &GlobalOpts,
    resolve: impl FnOnce() -> Option<Controller>,
) -> (Option<Controller>, bool) {
    let options = demo_fleet::options(global);
    if global.demo_fleet {
        info!(
            seed = options.seed,
            "launching against the synthetic demo fleet"
        );
        return (Some(Controller::demo(options)), true);
    }

    match resolve() {
        Some(controller) => {
            if global.demo_seed.is_some() {
                warn!("--demo-seed ignored: a real controller is configured");
            }
            (Some(controller), false)
        }
        None if global.demo => {
            warn!(
                seed = options.seed,
                "--demo with no controller configured; falling back to the synthetic demo fleet"
            );
            (Some(Controller::demo(options)), true)
        }
        None => (None, false),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use unifly_api::ControllerConfig;

    use super::*;
    use crate::cli::args::{Cli, Command};

    fn parse(argv: &[&str]) -> GlobalOpts {
        let cli = Cli::try_parse_from(argv).expect("valid arguments");
        assert!(matches!(cli.command, Command::Tui(_)), "expected tui");
        cli.global
    }

    fn real() -> Controller {
        Controller::new(ControllerConfig::default())
    }

    #[test]
    fn demo_fleet_flag_never_resolves_a_controller() {
        let global = parse(&["unifly", "tui", "--demo-fleet", "--demo-seed", "7"]);
        let (controller, fleet) = select_controller(&global, || {
            panic!("--demo-fleet must not touch config or credentials")
        });
        assert!(fleet);
        assert!(controller.is_some_and(|controller| controller.is_demo()));
    }

    #[test]
    fn plain_demo_keeps_a_real_controller() {
        let global = parse(&["unifly", "--demo", "tui"]);
        let (controller, fleet) = select_controller(&global, || Some(real()));
        assert!(!fleet);
        assert!(controller.is_some_and(|controller| !controller.is_demo()));
    }

    #[test]
    fn plain_demo_falls_back_to_the_fleet_without_a_controller() {
        let global = parse(&["unifly", "--demo", "tui"]);
        let (controller, fleet) = select_controller(&global, || None);
        assert!(fleet);
        assert!(controller.is_some_and(|controller| controller.is_demo()));
    }

    #[test]
    fn demo_fleet_never_loads_the_config_file() {
        let global = parse(&["unifly", "tui", "--demo-fleet"]);
        let config = startup_config(&global, || panic!("--demo-fleet must not read config"));
        assert!(config.is_none());

        let global = parse(&["unifly", "--demo", "tui"]);
        let config = startup_config(&global, || Some(Config::default()));
        assert!(config.is_some(), "plain --demo still reads config");
    }

    #[test]
    fn no_controller_and_no_demo_opens_onboarding() {
        let global = parse(&["unifly", "tui"]);
        let (controller, fleet) = select_controller(&global, || None);
        assert!(!fleet);
        assert!(controller.is_none());
    }
}
