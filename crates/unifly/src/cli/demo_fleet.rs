//! `--demo-fleet` for the CLI.
//!
//! A curated set of read-only commands answers from the synthetic demo
//! fleet, so CLI screenshots need no controller. Everything else is refused
//! up front, before any config file, keyring, or network access.

use clap::ArgMatches;
use unifly_api::{Controller, DEFAULT_DEMO_SEED, DemoFleetOptions};

use crate::cli::args::GlobalOpts;
use crate::cli::error::CliError;

/// Command paths that work with `--demo-fleet`. Each reads the controller's
/// store after connect, or a query the demo fleet answers itself.
pub const SUPPORTED: &[&str] = &[
    "system health",
    "system info",
    "alarms list",
    "wans list",
    "devices list",
    "devices get",
    "devices stats",
    "devices ports",
    "devices ports-export",
    "clients list",
    "clients get",
    "clients find",
    "clients roams",
    "clients wifi",
    "networks list",
    "networks get",
    "wifi list",
    "wifi get",
    "wifi neighbors",
    "wifi channels",
    "firewall policies list",
    "firewall policies get",
    "firewall zones list",
    "firewall zones get",
    "nat policies list",
    "nat policies get",
    "acl list",
    "acl get",
    "events list",
    "stats site",
    "stats gateway",
    "stats dpi",
    "topology",
    "sites list",
];

/// Commands that never need a controller and run normally.
const LOCAL: &[&str] = &["completions", "skill", "tui"];

/// The subcommand path a parse resolved to, with aliases canonicalized
/// (`unifly d ls` becomes `devices list`).
pub fn command_path(matches: &ArgMatches) -> String {
    let mut parts = Vec::new();
    let mut current = matches;
    while let Some((name, sub)) = current.subcommand() {
        parts.push(name.to_owned());
        current = sub;
    }
    parts.join(" ")
}

/// Whether `path` may run with `--demo-fleet`.
pub fn supports(path: &str) -> bool {
    let top = path.split(' ').next().unwrap_or_default();
    SUPPORTED.contains(&path) || LOCAL.contains(&top)
}

/// The error for a command the demo fleet cannot answer.
pub fn refuse(path: &str) -> CliError {
    CliError::DemoUnavailable {
        command: path.to_owned(),
        supported: SUPPORTED.join(", "),
    }
}

/// Options for the fleet `global` asks for.
pub fn options(global: &GlobalOpts) -> DemoFleetOptions {
    DemoFleetOptions::with_seed(global.demo_seed.unwrap_or(DEFAULT_DEMO_SEED))
}

/// A controller serving the demo fleet. Builds no HTTP clients and reads no
/// config.
pub fn controller(global: &GlobalOpts) -> Controller {
    Controller::demo(options(global))
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;
    use crate::cli::args::Cli;

    fn path_of(argv: &[&str]) -> String {
        let matches = Cli::command()
            .try_get_matches_from(argv)
            .expect("valid arguments");
        command_path(&matches)
    }

    #[test]
    fn every_supported_path_is_a_real_command() {
        let root = Cli::command();
        for path in SUPPORTED {
            let mut command = &root;
            for part in path.split(' ') {
                command = command
                    .find_subcommand(part)
                    .unwrap_or_else(|| panic!("`{path}` is not a command"));
            }
        }
    }

    #[test]
    fn aliases_resolve_to_canonical_paths() {
        assert_eq!(
            path_of(&["unifly", "--demo-fleet", "devices", "list"]),
            "devices list"
        );
        assert_eq!(
            path_of(&["unifly", "firewall", "policies", "list", "--demo-fleet"]),
            "firewall policies list"
        );
    }

    #[test]
    fn writes_and_cloud_are_refused() {
        assert!(supports("devices list"));
        assert!(supports("stats gateway"));
        assert!(supports("system health"));
        assert!(supports("alarms list"));
        assert!(!supports("alarms archive"));
        assert!(!supports("system reboot"));
        assert!(supports("completions"));
        assert!(!supports("devices restart"));
        assert!(supports("devices ports"));
        assert!(supports("devices ports-export"));
        assert!(!supports("devices port-set"));
        assert!(!supports("networks create"));
        assert!(!supports("cloud hosts"));
        assert!(!supports("config show"));
        assert!(!supports("api"));
        let message = refuse("devices restart").to_string();
        assert!(message.contains("demo mode"), "{message}");
    }
}
