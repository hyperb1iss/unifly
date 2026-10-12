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

// ── skill --install ─────────────────────────────────────────────────

const DISCOVERY_SKILL: &str = include_str!("../skill/discovery-skill.md");

fn installed_path(skills_root: &std::path::Path) -> std::path::PathBuf {
    skills_root.join("unifly").join("SKILL.md")
}

fn stdout_line(path: &std::path::Path) -> String {
    format!("{}\n", path.display())
}

#[test]
fn install_writes_the_skill_without_configuration_or_network() {
    let directory = TempDir::new().expect("temporary directory");
    let skills_root = directory.path().join("skills");
    let path = installed_path(&skills_root);
    offline_command(&directory)
        .args(["skill", "--install", "--dir"])
        .arg(&skills_root)
        .assert()
        .success()
        .stdout(stdout_line(&path))
        .stderr(predicate::str::starts_with(
            "Installed the unifly agent skill at ",
        ));
    assert_eq!(
        std::fs::read_to_string(&path).expect("installed skill"),
        DISCOVERY_SKILL
    );
    let entries: Vec<_> = std::fs::read_dir(directory.path())
        .expect("read directory")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(entries, ["skills"], "only the skill should be written");
}

#[test]
fn install_is_idempotent() {
    let directory = TempDir::new().expect("temporary directory");
    let skills_root = directory.path().join("skills");
    let path = installed_path(&skills_root);
    let install = || {
        offline_command(&directory)
            .args(["skill", "--install", "--dir"])
            .arg(&skills_root)
            .assert()
            .success()
            .stdout(stdout_line(&path))
    };
    install();
    let first = std::fs::metadata(&path).expect("metadata").modified();
    install().stderr(predicate::str::ends_with("is up to date\n"));
    let second = std::fs::metadata(&path).expect("metadata").modified();
    assert_eq!(
        first.ok(),
        second.ok(),
        "an up-to-date skill is not rewritten"
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("installed skill"),
        DISCOVERY_SKILL
    );
}

#[test]
fn install_refuses_an_unrecognized_skill_until_forced() {
    let directory = TempDir::new().expect("temporary directory");
    let skills_root = directory.path().join("skills");
    let path = installed_path(&skills_root);
    std::fs::create_dir_all(path.parent().expect("skill dir")).expect("create skill dir");
    std::fs::write(&path, "my own notes\n").expect("write local skill");

    offline_command(&directory)
        .args(["skill", "--install", "--dir"])
        .arg(&skills_root)
        .assert()
        .code(6)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("--force"));
    assert_eq!(
        std::fs::read_to_string(&path).expect("local skill"),
        "my own notes\n"
    );

    offline_command(&directory)
        .args(["skill", "--install", "--force", "--dir"])
        .arg(&skills_root)
        .assert()
        .success()
        .stdout(stdout_line(&path))
        .stderr(predicate::str::starts_with("Replaced "));
    assert_eq!(
        std::fs::read_to_string(&path).expect("installed skill"),
        DISCOVERY_SKILL
    );
}

#[test]
fn install_updates_a_published_skill_and_notes_leftover_files() {
    let directory = TempDir::new().expect("temporary directory");
    let skills_root = directory.path().join("skills");
    let path = installed_path(&skills_root);
    let skill_dir = path.parent().expect("skill dir");
    std::fs::create_dir_all(skill_dir.join("references")).expect("create references");
    std::fs::write(&path, DISCOVERY_SKILL.replace('\n', "\r\n")).expect("write CRLF copy");

    offline_command(&directory)
        .args(["skill", "--install", "--dir"])
        .arg(&skills_root)
        .assert()
        .success()
        .stdout(stdout_line(&path))
        .stderr(
            predicate::str::starts_with("Updated the unifly agent skill at ")
                .and(predicate::str::contains("also contains references")),
        );
    assert_eq!(
        std::fs::read_to_string(&path).expect("installed skill"),
        DISCOVERY_SKILL
    );
    assert!(skill_dir.join("references").is_dir(), "leftovers are kept");
}

#[test]
fn quiet_install_prints_only_the_path() {
    let directory = TempDir::new().expect("temporary directory");
    let skills_root = directory.path().join("skills");
    offline_command(&directory)
        .args(["skill", "--install", "--quiet", "--dir"])
        .arg(&skills_root)
        .assert()
        .success()
        .stdout(stdout_line(&installed_path(&skills_root)))
        .stderr(predicate::str::is_empty());
}

/// `HOME` decides the default target on Unix only; Windows resolves the
/// profile directory through the shell API, so these tests would write
/// into the real profile there.
#[cfg(unix)]
#[test]
fn install_defaults_to_agents_and_claude_skill_directories() {
    let directory = TempDir::new().expect("temporary directory");
    let home = directory.path().join("home");
    let agents = installed_path(&home.join(".agents").join("skills"));
    offline_command(&directory)
        .env("HOME", &home)
        .args(["skill", "--install"])
        .assert()
        .success()
        .stdout(stdout_line(&agents))
        .stderr(predicate::str::contains("--claude").not());

    std::fs::create_dir_all(home.join(".claude")).expect("create claude home");
    std::fs::remove_file(&agents).expect("remove agents skill");
    offline_command(&directory)
        .env("HOME", &home)
        .args(["skill", "--install"])
        .assert()
        .success()
        .stderr(predicate::str::contains("unifly skill --install --claude"));

    let claude = installed_path(&home.join(".claude").join("skills"));
    offline_command(&directory)
        .env("HOME", &home)
        .args(["skill", "--install", "--claude"])
        .assert()
        .success()
        .stdout(stdout_line(&claude));

    let config_dir = directory.path().join("claude-config");
    offline_command(&directory)
        .env("HOME", &home)
        .env("CLAUDE_CONFIG_DIR", &config_dir)
        .args(["skill", "--install", "--claude"])
        .assert()
        .success()
        .stdout(stdout_line(&installed_path(&config_dir.join("skills"))));
}

#[test]
fn install_flags_are_rejected_outside_install() {
    let directory = TempDir::new().expect("temporary directory");
    for args in [
        vec!["skill", "--dir", "skills"],
        vec!["skill", "--claude"],
        vec!["skill", "--force"],
        vec!["skill", "commands", "--install"],
        vec!["skill", "examples", "wifi-iot.json", "--install"],
        vec!["skill", "--install", "--claude", "--dir", "skills"],
    ] {
        offline_command(&directory)
            .args(&args)
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty());
    }
    assert_eq!(
        std::fs::read_dir(directory.path())
            .expect("read directory")
            .count(),
        0
    );
}
