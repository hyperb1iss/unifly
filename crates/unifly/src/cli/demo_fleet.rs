//! `--demo-fleet` for the CLI, and the guard that keeps `--demo` to the TUI.
//!
//! A curated set of read-only commands answers from the synthetic demo
//! fleet, so CLI screenshots need no controller. Everything else is refused
//! up front, before any config file, keyring, or network access.
//!
//! Demo mode (`--demo`, `UNIFI_DEMO`, or `[demo] enabled`) sanitizes what
//! the TUI draws. CLI output serializes whole records, which that sanitizer
//! does not cover, so controller and cloud commands refuse demo mode rather
//! than print real data under a setting that promises otherwise. The demo
//! fleet is synthetic already and needs no sanitizing.

use chrono::{DateTime, Utc};
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

/// 2026-03-18T15:30Z, the reference instant the demo fleet's own tests pin.
const OBSERVED_AT_SECS: i64 = 1_773_847_800;

/// When the demo fleet says it observed what a point-in-time annotation
/// records, such as the `// last-seen` markers of `ports-export
/// --with-clients`. The fleet's own clock starts at launch, so a fixed
/// stamp is what lets the same seed export the same bytes on every run.
pub fn observed_at() -> DateTime<Utc> {
    DateTime::from_timestamp(OBSERVED_AT_SECS, 0).unwrap_or_default()
}

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

/// Where demo mode was switched on.
pub enum DemoSwitch {
    /// `--demo` or `UNIFI_DEMO`.
    Flag,
    /// `enabled = true` under `[demo]` in the config file at this path.
    ConfigFile(std::path::PathBuf),
}

/// Whether `path` may run in demo mode: the TUI sanitizes, the local
/// commands print no network data, the demo fleet has none to hide, and
/// `config` stays usable so a config-file switch can be managed from the CLI.
pub fn honors_demo(path: &str, demo_fleet: bool) -> bool {
    let top = path.split(' ').next().unwrap_or_default();
    demo_fleet || top == "config" || LOCAL.contains(&top)
}

/// The error for a command that cannot sanitize its output for demo mode,
/// with a hint that turns off whichever switch is on.
pub fn refuse_demo(path: &str, switch: &DemoSwitch) -> CliError {
    let help = match switch {
        DemoSwitch::Flag => "--demo (or UNIFI_DEMO) redacts names, addresses, and SSIDs in the \
             TUI only: record a real network with `unifly tui --demo`. For CLI recordings use \
             --demo-fleet, which serves a synthetic network, or drop --demo."
            .to_owned(),
        DemoSwitch::ConfigFile(file) => format!(
            "Demo mode is on because `enabled = true` sits under [demo] in\n  {}\n\
             It redacts only the TUI. Set `enabled = false` there to run CLI commands \
             against the controller, or use --demo-fleet for a synthetic network.",
            file.display()
        ),
    };
    CliError::DemoNotSanitized {
        command: path.to_owned(),
        help,
    }
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
    fn demo_sanitizing_is_limited_to_the_tui() {
        assert!(honors_demo("tui", false));
        assert!(honors_demo("skill", false));
        assert!(honors_demo("completions", false));
        assert!(honors_demo("clients list", true));
        assert!(!honors_demo("clients list", false));
        assert!(honors_demo("config show", false));
        assert!(honors_demo("config set", false));
        assert!(!honors_demo("cloud hosts", false));
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
    fn observed_at_is_the_fixed_reference_instant() {
        assert_eq!(
            observed_at().format("%Y-%m-%dT%H:%MZ").to_string(),
            "2026-03-18T15:30Z"
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
