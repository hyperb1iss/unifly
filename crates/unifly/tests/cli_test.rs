//! Integration tests for the `unifly` CLI binary.
//!
//! These tests validate argument parsing, help output, shell completions,
//! and error handling — all without requiring a live UniFi controller.
#![allow(clippy::unwrap_used)]

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

// ── Helpers ─────────────────────────────────────────────────────────

/// Build a [`Command`] for the `unifly` binary with env isolation.
///
/// Clears all `UNIFI_*` env vars and points config directories at a
/// nonexistent path so tests never touch the user's real configuration.
fn unifly_cmd() -> assert_cmd::Command {
    let mut cmd = cargo_bin_cmd!("unifly");
    cmd.env("HOME", "/tmp/unifly-test-nonexistent")
        .env("XDG_CONFIG_HOME", "/tmp/unifly-test-nonexistent")
        .env("APPDATA", "/tmp/unifly-test-nonexistent")
        .env("LOCALAPPDATA", "/tmp/unifly-test-nonexistent")
        .env("USERPROFILE", "/tmp/unifly-test-nonexistent")
        .env_remove("UNIFI_PROFILE")
        .env_remove("UNIFI_URL")
        .env_remove("UNIFI_SITE")
        .env_remove("UNIFI_API_KEY")
        .env_remove("UNIFI_OUTPUT")
        .env_remove("UNIFI_INSECURE")
        .env_remove("UNIFI_TIMEOUT")
        .env_remove("UNIFI_USERNAME")
        .env_remove("UNIFI_PASSWORD");
    cmd
}

/// Build an isolated [`Command`] that can safely write config into a temp dir.
fn unifly_cmd_in(config_home: &std::path::Path) -> assert_cmd::Command {
    let mut cmd = cargo_bin_cmd!("unifly");
    cmd.env("HOME", config_home)
        .env("XDG_CONFIG_HOME", config_home)
        .env("APPDATA", config_home)
        .env("LOCALAPPDATA", config_home)
        .env("USERPROFILE", config_home)
        .env_remove("UNIFI_PROFILE")
        .env_remove("UNIFI_URL")
        .env_remove("UNIFI_SITE")
        .env_remove("UNIFI_API_KEY")
        .env_remove("UNIFI_OUTPUT")
        .env_remove("UNIFI_INSECURE")
        .env_remove("UNIFI_TIMEOUT")
        .env_remove("UNIFI_USERNAME")
        .env_remove("UNIFI_PASSWORD");
    cmd
}

fn written_config(tempdir: &TempDir) -> String {
    let candidates = [
        tempdir.path().join("unifly").join("config.toml"),
        tempdir
            .path()
            .join("Library")
            .join("Application Support")
            .join("unifly")
            .join("config.toml"),
        tempdir
            .path()
            .join("AppData")
            .join("Roaming")
            .join("unifly")
            .join("config.toml"),
    ];

    let path = candidates
        .into_iter()
        .find(|candidate| candidate.exists())
        .expect("config file should be written to one of the platform config directories");

    std::fs::read_to_string(path).unwrap()
}

/// Concatenate stdout + stderr from a command output for flexible matching.
fn combined_output(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!("{stdout}{stderr}")
}

// ── Basic invocation ────────────────────────────────────────────────

#[test]
fn test_no_args_shows_help() {
    let output = unifly_cmd().output().unwrap();
    assert_eq!(output.status.code(), Some(2), "Expected exit code 2");
    let text = combined_output(&output);
    assert!(
        text.contains("Usage"),
        "Expected 'Usage' in output:\n{text}"
    );
}

#[test]
fn test_help_flag() {
    unifly_cmd().arg("--help").assert().success().stdout(
        predicate::str::contains("UniFi network")
            .and(predicate::str::contains("devices"))
            .and(predicate::str::contains("clients"))
            .and(predicate::str::contains("networks")),
    );
}

#[test]
fn test_vpn_help_mentions_status_and_health() {
    unifly_cmd()
        .args(["vpn", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("servers")
                .and(predicate::str::contains("tunnels"))
                .and(predicate::str::contains("status"))
                .and(predicate::str::contains("health")),
        );
}

#[test]
fn test_version_flag() {
    unifly_cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("unifly"));
}

#[test]
fn test_api_help_mentions_all_http_methods() {
    unifly_cmd()
        .args(["api", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("get")
                .and(predicate::str::contains("post"))
                .and(predicate::str::contains("put"))
                .and(predicate::str::contains("patch"))
                .and(predicate::str::contains("delete")),
        );
}

// ── Shell completions ───────────────────────────────────────────────

#[test]
fn test_completions_bash() {
    unifly_cmd()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
fn test_completions_zsh() {
    unifly_cmd()
        .args(["completions", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#compdef"));
}

#[test]
fn test_completions_fish() {
    unifly_cmd()
        .args(["completions", "fish"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
fn test_completions_powershell_use_unifly_command_name() {
    unifly_cmd()
        .args(["completions", "powershell"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Register-ArgumentCompleter")
                .and(predicate::str::contains("unifly")),
        );
}

// ── Error cases ─────────────────────────────────────────────────────

#[test]
fn test_invalid_subcommand() {
    let output = unifly_cmd().arg("foobar").output().unwrap();
    assert!(
        !output.status.success(),
        "Expected failure for invalid subcommand"
    );
    let text = combined_output(&output);
    assert!(
        text.contains("invalid") || text.contains("unrecognized") || text.contains("foobar"),
        "Expected error mentioning invalid subcommand:\n{text}"
    );
}

#[test]
fn test_devices_list_no_controller() {
    unifly_cmd()
        .args(["devices", "list"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile")),
        );
}

#[test]
fn test_vpn_servers_get_parses_before_config_check() {
    let output = unifly_cmd()
        .args([
            "vpn",
            "servers",
            "get",
            "11111111-1111-1111-1111-111111111111",
        ])
        .output()
        .unwrap();
    let text = combined_output(&output);

    assert!(
        !text.contains("unexpected argument"),
        "expected clap to accept `vpn servers get <id>`:\n{text}"
    );
}

#[test]
fn test_config_show_no_config() {
    // `config show` uses load_config_or_default() so it succeeds even
    // when no config file exists — it just renders the default config.
    unifly_cmd().args(["config", "show"]).assert().success();
}

#[test]
fn test_config_profiles_no_config_mentions_unifly() {
    unifly_cmd()
        .args(["config", "profiles"])
        .assert()
        .success()
        .stderr(predicate::str::contains("unifly config init"));
}

#[test]
fn test_config_set_supports_profile_dot_path() {
    let tempdir = tempfile::tempdir().unwrap();

    unifly_cmd_in(tempdir.path())
        .args([
            "config",
            "set",
            "profiles.home.controller",
            "https://192.168.1.1",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Set controller on profile 'home'"));

    let cfg = written_config(&tempdir);
    assert!(
        cfg.contains("[profiles.home]"),
        "Expected home profile in config:\n{cfg}"
    );
    assert!(
        cfg.contains("controller = \"https://192.168.1.1\""),
        "Expected controller in config:\n{cfg}"
    );
}

#[test]
fn test_config_set_bare_key_still_targets_active_profile() {
    let tempdir = tempfile::tempdir().unwrap();

    unifly_cmd_in(tempdir.path())
        .args(["config", "set", "controller", "https://10.0.0.1"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Set controller on profile 'default'",
        ));

    let cfg = written_config(&tempdir);
    assert!(
        cfg.contains("[profiles.default]"),
        "Expected default profile in config:\n{cfg}"
    );
    assert!(
        cfg.contains("controller = \"https://10.0.0.1\""),
        "Expected controller in config:\n{cfg}"
    );
}

#[test]
fn test_config_set_help_mentions_profile_dot_path() {
    unifly_cmd()
        .args(["config", "set", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("profiles.home.controller"));
}

#[test]
fn test_config_set_password_accepts_positional_profile() {
    let output = unifly_cmd()
        .args(["config", "set-password", "home"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "Expected missing profile failure, not parse success"
    );
    let text = combined_output(&output);
    assert!(
        !text.contains("unexpected argument"),
        "Positional profile should parse:\n{text}"
    );
    assert!(
        text.contains("home") || text.contains("profile"),
        "Expected output to mention the missing profile:\n{text}"
    );
}

#[test]
fn test_config_set_password_help_mentions_positional_and_flag_profile() {
    unifly_cmd()
        .args(["config", "set-password", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("[PROFILE]")
                .and(predicate::str::contains("--profile <PROFILE>")),
        );
}

#[test]
fn test_invalid_output_format() {
    let output = unifly_cmd()
        .args(["--output", "invalid", "devices", "list"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "Expected failure for invalid output format"
    );
    let text = combined_output(&output);
    assert!(
        text.contains("invalid")
            || text.contains("possible values")
            || text.contains("valid value"),
        "Expected error about valid output formats:\n{text}"
    );
}

#[test]
fn test_global_flags_parsing() {
    // All flags should parse correctly — the failure should be about
    // missing controller config, not about argument parsing.
    unifly_cmd()
        .args([
            "--output",
            "json",
            "--verbose",
            "--insecure",
            "--timeout",
            "60",
            "devices",
            "list",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile")),
        );
}

#[test]
fn test_network_refs_command_parses() {
    unifly_cmd()
        .args(["networks", "refs", "00000000-0000-0000-0000-000000000000"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile")),
        );
}

#[test]
fn test_api_command_parses_with_valid_method() {
    unifly_cmd()
        .args(["api", "api/s/default/stat/health", "--method", "post"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile")),
        );
}

#[test]
fn test_api_command_rejects_invalid_method() {
    let output = unifly_cmd()
        .args(["api", "api/s/default/stat/health", "--method", "trace"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "Expected invalid API method to fail parsing"
    );
    let text = combined_output(&output);
    assert!(
        text.contains("invalid value") || text.contains("possible values"),
        "Expected invalid method parse error:\n{text}"
    );
}

#[test]
fn test_devices_pending_list_flags_parse() {
    unifly_cmd()
        .args([
            "devices", "pending", "--limit", "1", "--offset", "1", "--filter", "pending",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile")),
        );
}

#[test]
fn test_devices_list_supports_expression_filter_syntax() {
    unifly_cmd()
        .args(["devices", "list", "--filter", "name.eq('beta')"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile")),
        );
}

#[test]
fn test_devices_tags_all_flag_parse() {
    unifly_cmd()
        .args(["devices", "tags", "--all"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile")),
        );
}

#[test]
fn test_system_backup_list_command_parses() {
    unifly_cmd()
        .args(["system", "backup", "list"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile")),
        );
}

#[test]
fn test_system_backup_download_command_parses() {
    unifly_cmd()
        .args([
            "system",
            "backup",
            "download",
            "backup.unf",
            "--path",
            "/tmp/unifly-test-backup.unf",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile")),
        );
}

#[test]
fn test_acl_create_requires_zone_flags() {
    let output = unifly_cmd()
        .args(["acl", "create", "--name", "test", "--action", "allow"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "Expected failure when source/dest zone flags are missing"
    );
    let text = combined_output(&output);
    assert!(
        text.contains("--source-zone") || text.contains("--dest-zone"),
        "Expected error about missing zone flags:\n{text}"
    );
}

#[test]
fn test_acl_create_command_parses_with_required_flags() {
    unifly_cmd()
        .args([
            "acl",
            "create",
            "--name",
            "test",
            "--action",
            "allow",
            "--rule-type",
            "ipv4",
            "--source-zone",
            "00000000-0000-0000-0000-000000000001",
            "--dest-zone",
            "00000000-0000-0000-0000-000000000002",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile")),
        );
}

#[test]
fn test_firewall_policy_create_requires_zone_flags() {
    let output = unifly_cmd()
        .args([
            "firewall", "policies", "create", "--name", "test", "--action", "allow",
        ])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "Expected failure when source/dest zone flags are missing"
    );
    let text = combined_output(&output);
    assert!(
        text.contains("--source-zone") || text.contains("--dest-zone"),
        "Expected error about missing zone flags:\n{text}"
    );
}

#[test]
fn test_firewall_policy_create_command_parses_with_required_flags() {
    unifly_cmd()
        .args([
            "firewall",
            "policies",
            "create",
            "--name",
            "test",
            "--action",
            "allow",
            "--source-zone",
            "00000000-0000-0000-0000-000000000001",
            "--dest-zone",
            "00000000-0000-0000-0000-000000000002",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile")),
        );
}

// ── Subcommand help discovery ───────────────────────────────────────

#[test]
fn test_devices_subcommands_exist() {
    unifly_cmd()
        .args(["devices", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("list")
                .and(predicate::str::contains("get"))
                .and(predicate::str::contains("adopt"))
                .and(predicate::str::contains("remove"))
                .and(predicate::str::contains("restart")),
        );
}

#[test]
fn test_clients_subcommands_exist() {
    unifly_cmd()
        .args(["clients", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("list")
                .and(predicate::str::contains("get"))
                .and(predicate::str::contains("roams"))
                .and(predicate::str::contains("wifi"))
                .and(predicate::str::contains("block"))
                .and(predicate::str::contains("unblock")),
        );
}

#[test]
fn test_wifi_subcommands_exist() {
    unifly_cmd()
        .args(["wifi", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("list")
                .and(predicate::str::contains("get"))
                .and(predicate::str::contains("neighbors"))
                .and(predicate::str::contains("channels"))
                .and(predicate::str::contains("create")),
        );
}

#[test]
fn test_firewall_subcommands_exist() {
    unifly_cmd()
        .args(["firewall", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("policies").and(predicate::str::contains("zones")));
}

#[test]
fn test_vpn_settings_subcommands_exist() {
    unifly_cmd()
        .args(["vpn", "settings", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("list")
                .and(predicate::str::contains("get"))
                .and(predicate::str::contains("set"))
                .and(predicate::str::contains("patch")),
        );
}

#[test]
fn test_vpn_settings_set_command_parses() {
    unifly_cmd()
        .args(["vpn", "settings", "set", "teleport", "--enabled", "true"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile")),
        );
}

#[test]
fn test_vpn_site_to_site_subcommands_exist() {
    unifly_cmd()
        .args(["vpn", "site-to-site", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("list")
                .and(predicate::str::contains("get"))
                .and(predicate::str::contains("create"))
                .and(predicate::str::contains("update"))
                .and(predicate::str::contains("delete")),
        );
}

#[test]
fn test_vpn_site_to_site_create_command_parses() {
    unifly_cmd()
        .args(["vpn", "site-to-site", "create", "-F", "vpn.json"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile"))
                .or(predicate::str::contains("No such file")),
        );
}

#[test]
fn test_vpn_remote_access_subcommands_exist() {
    unifly_cmd()
        .args(["vpn", "remote-access", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("list")
                .and(predicate::str::contains("get"))
                .and(predicate::str::contains("create"))
                .and(predicate::str::contains("update"))
                .and(predicate::str::contains("suggest-port"))
                .and(predicate::str::contains("download-config"))
                .and(predicate::str::contains("delete")),
        );
}

#[test]
fn test_vpn_remote_access_create_command_parses() {
    unifly_cmd()
        .args(["vpn", "remote-access", "create", "-F", "vpn.json"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile"))
                .or(predicate::str::contains("No such file")),
        );
}

#[test]
fn test_vpn_remote_access_download_config_command_parses() {
    unifly_cmd()
        .args(["vpn", "remote-access", "download-config", "vpn1"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile")),
        );
}

#[test]
fn test_vpn_clients_subcommands_exist() {
    unifly_cmd()
        .args(["vpn", "clients", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("list")
                .and(predicate::str::contains("get"))
                .and(predicate::str::contains("create"))
                .and(predicate::str::contains("update"))
                .and(predicate::str::contains("delete")),
        );
}

#[test]
fn test_vpn_clients_create_command_parses() {
    unifly_cmd()
        .args(["vpn", "clients", "create", "-F", "vpn.json"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile"))
                .or(predicate::str::contains("No such file")),
        );
}

#[test]
fn test_vpn_connections_subcommands_exist() {
    unifly_cmd()
        .args(["vpn", "connections", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("list")
                .and(predicate::str::contains("get"))
                .and(predicate::str::contains("restart")),
        );
}

#[test]
fn test_vpn_connections_restart_command_parses() {
    unifly_cmd()
        .args(["vpn", "connections", "restart", "vpn1"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile")),
        );
}

#[test]
fn test_vpn_peers_subcommands_exist() {
    unifly_cmd()
        .args(["vpn", "peers", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("list")
                .and(predicate::str::contains("get"))
                .and(predicate::str::contains("create"))
                .and(predicate::str::contains("update"))
                .and(predicate::str::contains("delete"))
                .and(predicate::str::contains("subnets")),
        );
}

#[test]
fn test_vpn_peers_create_command_parses() {
    unifly_cmd()
        .args(["vpn", "peers", "create", "server123", "-F", "peer.json"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile"))
                .or(predicate::str::contains("No such file")),
        );
}

#[test]
fn test_vpn_magic_site_to_site_subcommands_exist() {
    unifly_cmd()
        .args(["vpn", "magic-site-to-site", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list").and(predicate::str::contains("get")));
}

// ── NAT ──────────────────────────────────────────────────────────────

#[test]
fn test_nat_policies_subcommands_exist() {
    unifly_cmd()
        .args(["nat", "policies", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("list")
                .and(predicate::str::contains("get"))
                .and(predicate::str::contains("create"))
                .and(predicate::str::contains("update"))
                .and(predicate::str::contains("delete")),
        );
}

#[test]
fn test_nat_policies_update_rejects_bare_id() {
    // With no update flags and no config, the CLI fails at config load
    // before reaching the "at least one flag" validation. This test
    // verifies the command parses but does not silently succeed.
    let output = unifly_cmd()
        .args(["nat", "policies", "update", "test-id"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("config")
            || stderr.contains("Configuration")
            || stderr.contains("at least one"),
        "Expected config-load or validation error, got: {stderr}"
    );
}

#[test]
fn test_nat_policies_update_name_conflicts_with_description() {
    unifly_cmd()
        .args([
            "nat",
            "policies",
            "update",
            "test-id",
            "--name",
            "foo",
            "--description",
            "bar",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("--name")
                .and(predicate::str::contains("--description"))
                .and(predicate::str::contains("cannot be used with")),
        );
}

#[test]
fn test_nat_policies_update_command_parses_with_name() {
    unifly_cmd()
        .args(["nat", "policies", "update", "test-id", "--name", "new-name"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile"))
                .or(predicate::str::contains("session")),
        );
}

#[test]
fn test_config_subcommands_exist() {
    unifly_cmd()
        .args(["config", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("init")
                .and(predicate::str::contains("show"))
                .and(predicate::str::contains("profiles")),
        );
}
