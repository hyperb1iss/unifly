//! The installed skill must be usable offline, including outside a source checkout.

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

fn offline_command(directory: &TempDir) -> assert_cmd::Command {
    let mut command = cargo_bin_cmd!("unifly");
    command
        .env_clear()
        .current_dir(directory.path())
        .env("XDG_CONFIG_HOME", directory.path())
        .env("APPDATA", directory.path())
        .env("LOCALAPPDATA", directory.path())
        .env("UNIFI_URL", "http://127.0.0.1:1")
        .env("UNIFI_PROFILE", "missing-profile")
        .env("UNIFI_TIMEOUT", "1");
    command
}

#[test]
fn every_guide_works_without_configuration_or_network() {
    let directory = TempDir::new().expect("temporary directory");
    for topic in ["core", "commands", "concepts", "workflows", "examples"] {
        offline_command(&directory)
            .args(["skill", topic, "--output", "json", "--color", "always"])
            .assert()
            .success()
            .stderr(predicate::str::is_empty())
            .stdout(predicate::str::starts_with(concat!(
                "# unifly ",
                env!("CARGO_PKG_VERSION"),
                " agent guide\n"
            )))
            .stdout(predicate::str::contains("\u{1b}").not());
    }
    offline_command(&directory)
        .arg("skill")
        .assert()
        .success()
        .stdout(predicate::str::contains("unifly skill commands"));
    assert_eq!(
        std::fs::read_dir(directory.path())
            .expect("read directory")
            .count(),
        0
    );
}

#[test]
fn example_output_is_a_verbatim_payload() {
    let directory = TempDir::new().expect("temporary directory");
    offline_command(&directory)
        .args(["skill", "examples", "network-iot-vlan.json"])
        .assert()
        .success()
        .stderr(predicate::str::is_empty())
        .stdout(include_str!("../skill/examples/network-iot-vlan.json"));
}

#[test]
fn invalid_topic_or_example_fails_without_partial_output() {
    let directory = TempDir::new().expect("temporary directory");
    for args in [
        vec!["skill", "missing"],
        vec!["skill", "examples", "../../config.toml"],
        vec!["skill", "core", "config.toml"],
    ] {
        offline_command(&directory)
            .args(args)
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty());
    }
}

#[test]
fn inventory_includes_nested_commands_and_respects_build_features() {
    let directory = TempDir::new().expect("temporary directory");
    let mut command = offline_command(&directory);
    let assertion = command.args(["skill", "commands"]).assert().success();
    let output = String::from_utf8_lossy(&assertion.get_output().stdout);
    for path in [
        "unifly devices port-set",
        "unifly vpn peers subnets",
        "unifly settings export",
        "unifly config cloud-setup",
        "unifly skill",
    ] {
        assert!(output.contains(&format!("`{path}`")), "missing {path}");
    }
    assert_eq!(output.contains("`unifly tui`"), cfg!(feature = "tui"));
}
