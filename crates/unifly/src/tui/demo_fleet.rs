//! Controller selection for the synthetic demo fleet.

use tracing::{info, warn};
use unifly_api::{Controller, DEFAULT_DEMO_SEED, DemoFleetOptions};

use crate::cli::args::{GlobalOpts, TuiArgs};

/// Pick this session's controller and report whether it is the demo fleet.
///
/// `--demo-fleet` always gets the synthetic fleet and never touches config
/// or credentials. Otherwise the real controller is resolved, and plain
/// `--demo` falls back to the fleet when nothing could be resolved instead
/// of opening onboarding.
pub(super) fn select_controller(
    global: &GlobalOpts,
    args: &TuiArgs,
    resolve: impl FnOnce() -> Option<Controller>,
) -> (Option<Controller>, bool) {
    let options = DemoFleetOptions::with_seed(args.demo_seed.unwrap_or(DEFAULT_DEMO_SEED));
    if args.demo_fleet {
        info!(
            seed = options.seed,
            "launching against the synthetic demo fleet"
        );
        return (Some(Controller::demo(options)), true);
    }

    match resolve() {
        Some(controller) => {
            if args.demo_seed.is_some() {
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

    fn parse(argv: &[&str]) -> (GlobalOpts, TuiArgs) {
        let cli = Cli::try_parse_from(argv).expect("valid arguments");
        let Command::Tui(tui) = cli.command else {
            panic!("expected the tui subcommand");
        };
        (cli.global, tui)
    }

    fn real() -> Controller {
        Controller::new(ControllerConfig::default())
    }

    #[test]
    fn demo_fleet_flag_never_resolves_a_controller() {
        let (global, args) = parse(&["unifly", "tui", "--demo-fleet", "--demo-seed", "7"]);
        let (controller, fleet) = select_controller(&global, &args, || {
            panic!("--demo-fleet must not touch config or credentials")
        });
        assert!(fleet);
        assert!(controller.is_some_and(|controller| controller.is_demo()));
    }

    #[test]
    fn plain_demo_keeps_a_real_controller() {
        let (global, args) = parse(&["unifly", "--demo", "tui"]);
        let (controller, fleet) = select_controller(&global, &args, || Some(real()));
        assert!(!fleet);
        assert!(controller.is_some_and(|controller| !controller.is_demo()));
    }

    #[test]
    fn plain_demo_falls_back_to_the_fleet_without_a_controller() {
        let (global, args) = parse(&["unifly", "--demo", "tui"]);
        let (controller, fleet) = select_controller(&global, &args, || None);
        assert!(fleet);
        assert!(controller.is_some_and(|controller| controller.is_demo()));
    }

    #[test]
    fn no_controller_and_no_demo_opens_onboarding() {
        let (global, args) = parse(&["unifly", "tui"]);
        let (controller, fleet) = select_controller(&global, &args, || None);
        assert!(!fleet);
        assert!(controller.is_none());
    }
}
