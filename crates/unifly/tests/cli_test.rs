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
        .env_remove("UNIFI_HOST_ID")
        .env_remove("UNIFI_OUTPUT")
        .env_remove("UNIFI_INSECURE")
        .env_remove("UNIFI_TIMEOUT")
        .env_remove("UNIFI_USERNAME")
        .env_remove("UNIFI_PASSWORD")
        .env_remove("UNIFI_DEMO")
        .env_remove("UNIFI_DEMO_FLEET")
        .env_remove("UNIFI_DEMO_SEED");
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
        .env_remove("UNIFI_HOST_ID")
        .env_remove("UNIFI_OUTPUT")
        .env_remove("UNIFI_INSECURE")
        .env_remove("UNIFI_TIMEOUT")
        .env_remove("UNIFI_USERNAME")
        .env_remove("UNIFI_PASSWORD")
        .env_remove("UNIFI_DEMO")
        .env_remove("UNIFI_DEMO_FLEET")
        .env_remove("UNIFI_DEMO_SEED");
    cmd
}

fn written_config(tempdir: &TempDir) -> String {
    std::fs::read_to_string(config_file(tempdir)).unwrap()
}

fn config_file(tempdir: &TempDir) -> std::path::PathBuf {
    let candidates = [
        tempdir.path().join("unifly").join("config.toml"),
        tempdir
            .path()
            .join("AppData")
            .join("Roaming")
            .join("unifly")
            .join("config.toml"),
    ];

    candidates
        .into_iter()
        .find(|candidate| candidate.exists())
        .expect("config file should be written to one of the platform config directories")
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
fn test_settings_read_commands_offer_with_secrets() {
    for subcommand in ["list", "get", "export"] {
        unifly_cmd()
            .args(["settings", subcommand, "--help"])
            .assert()
            .success()
            .stdout(predicate::str::contains("--with-secrets"));
    }
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
fn test_cloud_help_mentions_fleet_subcommands() {
    unifly_cmd()
        .args(["cloud", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("hosts")
                .and(predicate::str::contains("sites"))
                .and(predicate::str::contains("switch"))
                .and(predicate::str::contains("devices"))
                .and(predicate::str::contains("isp"))
                .and(predicate::str::contains("sdwan")),
        );
}

#[test]
fn test_cloud_hosts_help_mentions_get() {
    unifly_cmd()
        .args(["cloud", "hosts", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("get"));
}

#[test]
fn test_cloud_isp_query_help_mentions_sites_flag() {
    unifly_cmd()
        .args(["cloud", "isp", "query", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--sites <SITES>"));
}

#[test]
fn test_cloud_switch_help_mentions_site_identifier() {
    unifly_cmd()
        .args(["cloud", "switch", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("<SITE>")
                .and(predicate::str::contains("internal reference"))
                .or(predicate::str::contains("UUID")),
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
        .stderr(
            predicate::str::contains("unifly config init")
                .and(predicate::str::contains("unifly config cloud-setup")),
        );
}

#[test]
fn test_config_help_mentions_cloud_setup() {
    unifly_cmd()
        .args(["config", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("cloud-setup"));
}

#[test]
fn test_config_cloud_setup_help_mentions_site_manager() {
    unifly_cmd()
        .args(["config", "cloud-setup", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Site Manager")
                .or(predicate::str::contains("cloud Site Manager")),
        );
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

/// Write a session-auth profile named `name` into an isolated config dir.
fn write_session_profile(config_home: &std::path::Path, name: &str, controller: &str) {
    for (key, value) in [("controller", controller), ("auth_mode", "session")] {
        unifly_cmd_in(config_home)
            .args(["config", "set", &format!("profiles.{name}.{key}"), value])
            .assert()
            .success();
    }
}

/// stderr with whitespace collapsed, since miette wraps help text.
fn squeezed_stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn test_missing_credentials_get_the_hint_that_fixes_them() {
    // Ad-hoc flags without a key: the fix is a key, not a profile secret.
    let output = unifly_cmd()
        .args(["-c", "https://127.0.0.1:9", "devices", "list"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        combined_output(&output)
    );
    let stderr = squeezed_stderr(&output);
    assert!(stderr.contains("--api-key"), "{stderr}");
    assert!(stderr.contains("UNIFI_API_KEY"), "{stderr}");
    assert!(!stderr.contains("set-password"), "{stderr}");

    // A session profile without a username needs one, then a password.
    // Session auth without a username fails before any keyring lookup.
    for name in ["lab", "my lab"] {
        let tempdir = tempfile::tempdir().unwrap();
        write_session_profile(tempdir.path(), name, "https://127.0.0.1:9");
        let output = unifly_cmd_in(tempdir.path())
            .args(["-p", name, "devices", "list"])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(3),
            "{}",
            combined_output(&output)
        );
        let stderr = squeezed_stderr(&output);
        let quoted = if name.contains(' ') {
            format!("'{name}'")
        } else {
            name.to_owned()
        };
        assert!(
            stderr.contains(&format!("No credentials configured for profile '{name}'")),
            "{stderr}"
        );
        assert!(
            stderr.contains(&format!("unifly -p {quoted} config set username")),
            "{stderr}"
        );
        assert!(
            stderr.contains(&format!("unifly config set-password {quoted}")),
            "{stderr}"
        );
        assert!(!stderr.contains("UNIFI_API_KEY"), "{stderr}");
        assert!(!stderr.contains("cloud-setup"), "{stderr}");
    }
}

#[test]
fn test_profile_validation_errors_are_usage_errors() {
    let tempdir = tempfile::tempdir().unwrap();
    write_session_profile(tempdir.path(), "lab", "https://127.0.0.1:9");
    let path = config_file(&tempdir);
    let cfg = std::fs::read_to_string(&path).unwrap();
    let cfg = cfg.replace(
        "[profiles.lab]\n",
        "[profiles.lab]\ntotp_command = [\"\"]\n",
    );
    std::fs::write(&path, cfg).unwrap();

    unifly_cmd_in(tempdir.path())
        .env("UNIFI_USERNAME", "admin")
        .env("UNIFI_PASSWORD", "test-password")
        .args(["-p", "lab", "devices", "list"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("Invalid value for totp_command"));
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
fn test_host_id_flag_parses_as_global_option() {
    unifly_cmd()
        .args(["--host-id", "console-123", "config", "show"])
        .assert()
        .success();
}

#[test]
fn test_config_set_accepts_cloud_auth_mode_and_host_fields() {
    let tempdir = tempfile::tempdir().unwrap();

    unifly_cmd_in(tempdir.path())
        .args(["config", "set", "auth_mode", "cloud"])
        .assert()
        .success();
    unifly_cmd_in(tempdir.path())
        .args(["config", "set", "host_id", "console-123"])
        .assert()
        .success();
    unifly_cmd_in(tempdir.path())
        .args(["config", "set", "host_id_env", "UNIFI_HOST_ID"])
        .assert()
        .success();

    let cfg = written_config(&tempdir);
    assert!(
        cfg.contains("auth_mode = \"cloud\""),
        "Expected cloud auth:\n{cfg}"
    );
    assert!(
        cfg.contains("host_id = \"console-123\""),
        "Expected host_id:\n{cfg}"
    );
    assert!(
        cfg.contains("host_id_env = \"UNIFI_HOST_ID\""),
        "Expected host_id_env:\n{cfg}"
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
            "devices",
            "pending",
            "--limit",
            "1",
            "--offset",
            "1",
            "--filter",
            "state.eq('pending')",
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
fn test_unparseable_filter_is_a_usage_error() {
    for filter in [
        "garbage((",
        "name.starts_with('x')",
        "name.eq('a','b')",
        "name",
    ] {
        unifly_cmd()
            .args([
                "--demo-fleet",
                "clients",
                "list",
                "-o",
                "json",
                "--filter",
                filter,
            ])
            .assert()
            .failure()
            .code(2)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("--filter"));
    }
}

#[test]
fn test_quiet_silences_the_truncation_hint() {
    unifly_cmd()
        .args(["--demo-fleet", "clients", "list"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Showing 25 of"));
    unifly_cmd()
        .args(["--demo-fleet", "-q", "clients", "list"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::is_empty());
}

#[test]
fn test_valid_filter_still_matches_on_the_demo_fleet() {
    let devices = demo_json(&["devices", "list", "--filter", "name.eq('AP-ROOF')"]);
    let names: Vec<&str> = devices
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|device| device["name"].as_str())
        .collect();
    assert_eq!(names, ["ap-roof"]);
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

// ── Demo fleet ──────────────────────────────────────────────────────

fn demo_json(args: &[&str]) -> serde_json::Value {
    let output = unifly_cmd()
        .arg("--demo-fleet")
        .args(args)
        .args(["-o", "json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", combined_output(&output));
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn test_demo_fleet_devices_list_table_runs_offline() {
    unifly_cmd()
        .args(["--demo-fleet", "devices", "list", "--color", "never"])
        .assert()
        .success()
        .stdout(predicate::str::contains("gw-harbor"))
        .stdout(predicate::str::contains("USW Pro Max 24 PoE"))
        .stdout(predicate::str::contains("203.0.113.42"));
}

#[test]
fn test_demo_fleet_clients_list_json_holds_the_whole_fleet() {
    let clients = demo_json(&["clients", "list", "--all"]);
    let count = clients.as_array().map_or(0, Vec::len);
    assert!(
        (120..=180).contains(&count),
        "unexpected client count {count}"
    );
}

#[test]
fn test_demo_fleet_networks_and_stats_answer_offline() {
    let networks = demo_json(&["networks", "list"]);
    let names: Vec<&str> = networks
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|network| network["name"].as_str())
        .collect();
    assert!(names.contains(&"Render"), "{names:?}");

    let rows = demo_json(&["stats", "gateway"]);
    let first = &rows.as_array().unwrap()[0];
    assert_eq!(first["o"], "gw");
    assert!(first["wan-rx_bytes"].as_u64().unwrap() > 0);
}

#[test]
fn test_demo_fleet_seed_reproduces_the_fleet() {
    let ids = |seed: &str| {
        let mut ids: Vec<String> = demo_json(&["--demo-seed", seed, "devices", "list"])
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|device| device["id"].as_str().map(str::to_owned))
            .collect();
        ids.sort();
        ids
    };
    assert_eq!(ids("7"), ids("7"));
    assert_ne!(ids("7"), ids("8"));
}

#[test]
fn test_demo_fleet_refuses_commands_it_cannot_answer() {
    for args in [
        &["devices", "restart", "00:00:00:00:00:01"][..],
        &["devices", "port-set", "sw-core", "12", "--poe", "off"][..],
        &["networks", "delete", "x"][..],
        &["config", "show"][..],
    ] {
        unifly_cmd()
            .arg("--demo-fleet")
            .args(args)
            .assert()
            .failure()
            .code(5)
            .stderr(predicate::str::contains("not available in demo mode"));
    }
}

#[test]
fn test_demo_fleet_devices_get_accepts_name_ip_and_any_mac_spelling() {
    let roof = demo_json(&["devices", "get", "ap-roof"]);
    assert_eq!(roof["name"], "ap-roof");
    let id = roof["id"].as_str().unwrap();
    let ip = roof["ip"].as_str().unwrap();
    let mac = roof["mac"].as_str().unwrap();
    let dashed_upper = mac.replace(':', "-").to_uppercase();
    let bare = mac.replace(':', "");
    let dotted = format!("{}.{}.{}", &bare[..4], &bare[4..8], &bare[8..]);
    for needle in [id, "AP-Roof", ip, dashed_upper.as_str(), dotted.as_str()] {
        let found = demo_json(&["devices", "get", needle]);
        assert_eq!(found["id"], roof["id"], "{needle} resolved elsewhere");
    }
}

#[test]
fn test_demo_fleet_devices_get_miss_points_at_devices_list() {
    unifly_cmd()
        .args([
            "--demo-fleet",
            "devices",
            "get",
            "ap-attic",
            "--color",
            "never",
        ])
        .assert()
        .failure()
        .code(4)
        .stderr(predicate::str::contains("device 'ap-attic' not found"))
        .stderr(predicate::str::contains("unifly devices list"));
}

/// `::76ce:f4ac:26ee` is an IPv6 address whose twelve hex digits spell a
/// client's MAC. `clients roams` once fetched that client's history from
/// it; every lookup must treat it as an IP that matches nothing.
#[test]
fn test_demo_fleet_ip_shaped_identifiers_never_pass_as_macs() {
    let as_ipv6 = |mac: &str| {
        let hex = mac.replace(':', "");
        format!("::{}:{}:{}", &hex[..4], &hex[4..8], &hex[8..])
    };
    let clients = demo_json(&["clients", "list", "--all"]);
    let wireless = clients
        .as_array()
        .unwrap()
        .iter()
        .find(|client| client["client_type"] == "Wireless")
        .unwrap();
    let client_ip = as_ipv6(wireless["mac"].as_str().unwrap());
    let device_ip = as_ipv6(
        demo_json(&["devices", "get", "ap-roof"])["mac"]
            .as_str()
            .unwrap(),
    );
    for args in [
        vec!["clients", "roams", client_ip.as_str()],
        vec!["clients", "get", client_ip.as_str()],
        vec!["clients", "wifi", client_ip.as_str()],
        vec!["devices", "get", device_ip.as_str()],
    ] {
        unifly_cmd()
            .arg("--demo-fleet")
            .args(&args)
            .assert()
            .failure()
            .code(4)
            .stderr(predicate::str::contains("not found"));
    }
}

#[test]
fn test_demo_fleet_get_commands_accept_display_names() {
    for command in [
        &["networks"][..],
        &["firewall", "policies"][..],
        &["firewall", "zones"][..],
        &["nat", "policies"][..],
        &["acl"][..],
    ] {
        let listed = demo_json(&[command, &["list", "--all"]].concat());
        for record in listed.as_array().unwrap().iter().take(3) {
            let name = record["name"].as_str().unwrap().to_uppercase();
            let found = demo_json(&[command, &["get", name.as_str()]].concat());
            assert_eq!(found["id"], record["id"], "{command:?} get {name}");
        }
    }
}

#[test]
fn test_demo_fleet_clients_get_accepts_hostname_ip_and_mac() {
    let clients = demo_json(&["clients", "list", "--all"]);
    let client = clients
        .as_array()
        .unwrap()
        .iter()
        .find(|client| client["hostname"].is_string() && client["ip"].is_string())
        .unwrap();
    let dashed = client["mac"].as_str().unwrap().replace(':', "-");
    for needle in [
        client["hostname"].as_str().unwrap(),
        client["ip"].as_str().unwrap(),
        dashed.as_str(),
    ] {
        let found = demo_json(&["clients", "get", needle]);
        assert_eq!(found["id"], client["id"], "clients get {needle}");
    }
}

#[test]
fn test_demo_fleet_answers_the_first_diagnosis_reads() {
    let health = demo_json(&["system", "health"]);
    let subsystems: Vec<&str> = health
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["subsystem"].as_str())
        .collect();
    assert_eq!(subsystems, ["wan", "www", "wlan", "lan", "vpn"]);
    assert_eq!(health[0]["wan_ip"], "203.0.113.42");

    let wans = demo_json(&["wans", "list"]);
    assert_eq!(wans[0]["name"], "WAN");
    assert_eq!(wans[0]["ip"], "203.0.113.42");
    assert_eq!(wans[1]["name"], "WAN2");
    assert!(wans[1]["ip"].is_null(), "WAN2 has no link: {}", wans[1]);

    let alarms = demo_json(&["alarms", "list"]);
    let messages: Vec<&str> = alarms
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|alarm| alarm["message"].as_str())
        .collect();
    for needle in ["ap-dock", "sw-core port 12", "ap-roof", "WAN2"] {
        assert!(
            messages.iter().any(|message| message.contains(needle)),
            "no alarm mentions {needle}: {messages:?}"
        );
    }

    let info = demo_json(&["system", "info"]);
    assert_eq!(info["hostname"], "gw-harbor");
}

#[test]
fn test_demo_fleet_answers_wifi_get_and_device_stats_by_name() {
    let wifi = demo_json(&["wifi", "list", "--all"]);
    let ssid = wifi[0]["name"].as_str().unwrap().to_lowercase();
    assert_eq!(
        demo_json(&["wifi", "get", ssid.as_str()])["id"],
        wifi[0]["id"]
    );

    let stats = demo_json(&["devices", "stats", "ap-roof"]);
    assert!(stats["stats"]["cpu_utilization_pct"].is_number(), "{stats}");
}

fn demo_stdout(args: &[&str]) -> String {
    let output = unifly_cmd()
        .arg("--demo-fleet")
        .args(args)
        .args(["--color", "never"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", combined_output(&output));
    String::from_utf8(output.stdout).unwrap()
}

/// Parse `ports-export` output the way `port-set --from-file` does, into
/// the request type that rejects unknown fields.
fn parse_port_export(jsonc: &str) -> unifly_api::ApplyPortsRequest {
    let mut json = String::new();
    std::io::Read::read_to_string(
        &mut json_comments::StripComments::new(jsonc.as_bytes()),
        &mut json,
    )
    .unwrap();
    serde_json::from_str(&json).unwrap()
}

#[test]
fn test_demo_fleet_devices_ports_shows_the_core_switch() {
    let table = demo_stdout(&["devices", "ports", "sw-core"]);
    let row = |name: &str| {
        table
            .lines()
            .find(|line| line.contains(&format!(" {name} ")))
            .unwrap_or_else(|| panic!("no row for {name}:\n{table}"))
            .to_owned()
    };
    let dark = row("cam-dock");
    assert!(
        dark.contains("DOWN") && dark.contains("Cameras (50)"),
        "{dark}"
    );
    assert!(row("printer-ops").contains("IoT (40)"));
    assert!(row("badge-front").contains("auto (100)"));
    assert!(row("ap-dock").contains("DOWN"));
    assert!(row("sw-render").contains("Studio,Render,Guest,IoT"));
    assert!(row("Uplink").contains("all"));
}

#[test]
fn test_demo_fleet_devices_ports_with_clients_matches_clients_list() {
    let render = demo_json(&["devices", "get", "sw-render"]);
    let mac = render["mac"].as_str().unwrap();
    let ports = demo_json(&["devices", "ports", "sw-render", "--with-clients"]);
    let ports = ports.as_array().unwrap();
    assert_eq!(ports.len(), 18);
    let connections = |index: u64| {
        ports
            .iter()
            .find(|port| port["index"] == index)
            .unwrap_or_else(|| panic!("no port {index}"))["connections"]
            .as_array()
            .unwrap()
            .clone()
    };

    let clients = demo_json(&["clients", "list", "--all"]);
    let wired: Vec<&serde_json::Value> = clients
        .as_array()
        .unwrap()
        .iter()
        .filter(|client| client["uplink_device_mac"] == mac)
        .collect();
    assert_eq!(wired.len(), 13, "render nodes, dailies, license, NAS");
    for client in wired {
        let port = client["switch_port"].as_u64().unwrap();
        let seen = connections(port);
        assert_eq!(seen.len(), 1, "port {port}: {seen:?}");
        assert_eq!(seen[0]["kind"], "client");
        assert_eq!(seen[0]["mac"], client["mac"]);
        assert_eq!(seen[0]["name"], client["name"]);
        let profile = ports.iter().find(|p| p["index"] == port).unwrap();
        assert_eq!(profile["name"], client["name"]);
        assert_eq!(profile["native_vlan_id"], client["vlan"]);
    }

    let ap = connections(13);
    assert_eq!(
        (&ap[0]["kind"], &ap[0]["name"]),
        (&"device".into(), &"ap-render".into())
    );
    let nas = ports.iter().find(|port| port["index"] == 17).unwrap();
    assert_eq!(nas["name"], "nas-vault");
    assert_eq!(nas["link_speed_mbps"], 10_000);
}

/// Compares raw bytes, `// last-seen` markers included: each run starts the
/// fleet's clock at launch, so a marker stamped from any clock would flip
/// whenever two runs straddle a minute.
#[test]
fn test_demo_fleet_annotated_export_is_byte_stable_across_runs() {
    let export = || {
        demo_stdout(&[
            "--demo-seed",
            "7",
            "devices",
            "ports-export",
            "sw-core",
            "--with-clients",
        ])
    };
    let first = export();
    let markers: Vec<&str> = first
        .lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with("// last-seen "))
        .collect();
    assert!(!markers.is_empty(), "{first}");
    assert!(
        markers
            .iter()
            .all(|line| line.starts_with("// last-seen 2026-03-18T15:30Z: ")),
        "{markers:?}"
    );
    assert_eq!(export(), first);
}

#[test]
fn test_demo_fleet_ports_export_round_trips_as_jsonc() {
    let annotated = demo_stdout(&["devices", "ports-export", "sw-core", "--with-clients"]);
    let markers: Vec<&str> = annotated
        .lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with("// last-seen "))
        .collect();
    assert!(
        markers
            .iter()
            .any(|line| line.ends_with("(ws-color-01, client)")),
        "{annotated}"
    );
    assert!(
        markers
            .iter()
            .any(|line| line.ends_with("(sw-render, device)"))
    );
    assert!(!markers.iter().any(|line| line.contains("cam-dock")));

    let request = parse_port_export(&annotated);
    let plain = demo_stdout(&["devices", "ports-export", "sw-core"]);
    assert!(!plain.contains("//"), "{plain}");
    assert_eq!(
        serde_json::to_value(parse_port_export(&plain)).unwrap(),
        serde_json::to_value(&request).unwrap(),
        "the markers are the only difference"
    );

    let profiles = demo_json(&["devices", "ports", "sw-core"]);
    let profiles = profiles.as_array().unwrap();
    let configured: Vec<u32> = request.ports.iter().map(|entry| entry.index).collect();
    assert!(
        configured.contains(&12),
        "the dark camera port stays configured"
    );
    assert!(!configured.contains(&15) && !configured.contains(&16));
    for entry in &request.ports {
        let profile = profiles.iter().find(|p| p["index"] == entry.index).unwrap();
        assert_eq!(profile["name"].as_str(), entry.name.as_deref());
        assert_eq!(profile["mode"].as_str(), entry.mode.as_deref());
        assert_eq!(
            profile["native_network_id"].as_str(),
            entry.native_network_id.as_deref()
        );
        assert_eq!(profile["poe_mode"].as_str(), entry.poe.as_deref());
    }
    let printer = request
        .ports
        .iter()
        .find(|entry| entry.name.as_deref() == Some("printer-ops"))
        .unwrap();
    assert_eq!(printer.speed.as_deref(), Some("100"));

    let full = parse_port_export(&demo_stdout(&[
        "devices",
        "ports-export",
        "sw-core",
        "--all",
    ]));
    assert_eq!(full.ports.len(), profiles.len());
}

/// The `path: keys` lines of the JSON field block in `unifly skill
/// concepts`, in order. Indented lines continue the previous path.
fn documented_json_keys() -> Vec<(String, Vec<String>)> {
    let guide = include_str!("../skill/concepts.md");
    let section = &guide[guide.find("## JSON field names").unwrap()..];
    let block = section.split("```text\n").nth(1).unwrap();
    let block = &block[..block.find("```").unwrap()];
    let mut records: Vec<(String, Vec<String>)> = Vec::new();
    for line in block.lines() {
        let words = if line.starts_with(' ') {
            line
        } else {
            let (path, keys) = line.split_once(": ").unwrap();
            records.push((path.to_owned(), Vec::new()));
            keys
        };
        let (_, keys) = records.last_mut().unwrap();
        keys.extend(words.split_whitespace().map(str::to_owned));
    }
    records
}

/// Every key the field guide lists must exist at its own record path in
/// real output, so a renamed (`speed_gbps`) or misplaced key fails, and
/// every record path must be one this test knows how to fetch.
#[test]
fn test_demo_fleet_json_keys_match_the_skill_guide() {
    let device = demo_json(&["devices", "get", "ap-roof"]);
    let clients = demo_json(&["clients", "list", "--all"]);
    let wireless = clients
        .as_array()
        .unwrap()
        .iter()
        .find(|client| client["client_type"] == "Wireless")
        .unwrap()
        .clone();
    let fetched = [
        ("device", device.clone()),
        ("device.ports[]", device["ports"][0].clone()),
        ("device.radios[]", device["radios"][0].clone()),
        ("device.stats", device["stats"].clone()),
        (
            "device.stats.uplink_bandwidth",
            device["stats"]["uplink_bandwidth"].clone(),
        ),
        ("client", wireless.clone()),
        ("client.bandwidth", wireless["bandwidth"].clone()),
        ("client.wireless", wireless["wireless"].clone()),
        ("event", demo_json(&["events", "list"])[0].clone()),
        ("alarm", demo_json(&["alarms", "list"])[0].clone()),
        ("health[]", demo_json(&["system", "health"])[0].clone()),
        ("wan", demo_json(&["wans", "list"])[0].clone()),
        ("report.site[]", demo_json(&["stats", "site"])[0].clone()),
        (
            "report.gateway[]",
            demo_json(&["stats", "gateway"])[0].clone(),
        ),
    ];

    let documented = documented_json_keys();
    let paths: Vec<&str> = documented.iter().map(|(path, _)| path.as_str()).collect();
    let known: Vec<&str> = fetched.iter().map(|(path, _)| *path).collect();
    assert_eq!(
        paths, known,
        "guide record paths and fetched records differ"
    );
    for ((path, keys), (_, record)) in documented.iter().zip(&fetched) {
        assert!(record.is_object(), "{path} is not an object: {record}");
        assert!(!keys.is_empty(), "{path} lists no keys");
        for key in keys {
            assert!(
                record.get(key).is_some(),
                "guide lists `{key}` under {path}, output lacks it: {record}"
            );
        }
    }
}

/// The config file holds profiles and possibly plaintext secrets, so the
/// demo fleet must never read it. A poisoned config sets a theme; if fleet
/// mode read it, colored table output would change.
#[test]
fn test_demo_fleet_never_reads_the_config_file() {
    let clean = TempDir::new().unwrap();
    let poisoned = TempDir::new().unwrap();
    for dir in [
        poisoned.path().join("unifly"),
        poisoned
            .path()
            .join("AppData")
            .join("Roaming")
            .join("unifly"),
    ] {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            "default_profile = \"home\"\n\
             [defaults]\ntheme = \"nord\"\n\
             [profiles.home]\ncontroller = \"https://192.0.2.1\"\n\
             api_key = \"must-never-be-read\"\n",
        )
        .unwrap();
    }

    let table = |home: &std::path::Path, extra: &[&str]| {
        let output = unifly_cmd_in(home)
            .env_remove("UNIFLY_THEME")
            .args(["--demo-fleet", "--demo-seed", "3"])
            .args(extra)
            .args(["devices", "list", "--color", "always"])
            .args(["--filter", "name.eq('gw-harbor')"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", combined_output(&output));
        output.stdout
    };
    let baseline = table(clean.path(), &[]);
    assert_eq!(
        table(poisoned.path(), &[]),
        baseline,
        "fleet mode read config"
    );
    assert_ne!(
        table(clean.path(), &["--theme", "nord"]),
        baseline,
        "the probe must be sensitive to the configured theme"
    );

    for (args, code) in [
        (&["clients", "list", "-o", "json"][..], 0),
        (&["networks", "list", "-o", "yaml"][..], 0),
        (&["devices", "restart", "00:00:00:00:00:01"][..], 5),
    ] {
        let output = unifly_cmd_in(poisoned.path())
            .arg("--demo-fleet")
            .args(args)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{}",
            combined_output(&output)
        );
        assert!(!combined_output(&output).contains("must-never-be-read"));
    }
}

// ── List ordering ───────────────────────────────────────────────────

fn demo_ids(args: &[&str]) -> Vec<String> {
    demo_json(args)
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn test_demo_fleet_lists_come_out_in_the_same_order_every_run() {
    for command in [
        &["devices", "list", "--all"][..],
        &["clients", "list", "--all"][..],
        &["networks", "list", "--all"][..],
        &["wifi", "list", "--all"][..],
        &["firewall", "policies", "list", "--all"][..],
        &["firewall", "zones", "list", "--all"][..],
        &["nat", "policies", "list", "--all"][..],
        &["acl", "list", "--all"][..],
    ] {
        let first = demo_ids(command);
        assert!(!first.is_empty(), "{command:?} returned no rows");
        for _ in 0..3 {
            assert_eq!(demo_ids(command), first, "{command:?} reordered");
        }
    }
}

#[test]
fn test_demo_fleet_topology_machine_formats_are_structured() {
    let clients = demo_json(&["clients", "list", "--all"]);
    let client_count = clients.as_array().map_or(0, Vec::len);

    for format in ["json", "json-compact", "yaml"] {
        let output = unifly_cmd()
            .args(["--demo-fleet", "topology", "-o", format])
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", combined_output(&output));
        let topology: serde_json::Value = if format == "yaml" {
            serde_yaml_ng::from_slice(&output.stdout)
                .unwrap_or_else(|err| panic!("{format}: invalid YAML ({err})"))
        } else {
            serde_json::from_slice(&output.stdout)
                .unwrap_or_else(|err| panic!("{format}: invalid JSON ({err})"))
        };
        assert_eq!(topology["gateway"]["name"], "gw-harbor", "{format}");
        let devices = topology["devices"].as_array().unwrap();
        assert!(!devices.is_empty(), "{format}");

        // Every client sits exactly once: under its uplink or unlinked.
        let placed = std::iter::once(&topology["gateway"])
            .chain(devices)
            .map(|node| node["clients"].as_array().map_or(0, Vec::len))
            .sum::<usize>()
            + topology["unlinked_clients"].as_array().map_or(0, Vec::len);
        assert_eq!(placed, client_count, "{format}");
    }
}

#[test]
fn test_demo_fleet_topology_reads_the_same_every_run() {
    let run = || {
        let output = unifly_cmd()
            .args(["--demo-fleet", "topology", "--color", "never"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", combined_output(&output));
        String::from_utf8(output.stdout).unwrap()
    };
    let first = run();
    assert!(first.contains("gw-harbor"), "{first}");
    for _ in 0..3 {
        assert_eq!(run(), first, "topology reordered between runs");
    }
}

#[test]
fn test_demo_fleet_pages_are_stable_and_disjoint() {
    let all = demo_ids(&["clients", "list", "--all"]);
    assert!(all.len() > 50, "fleet too small to page: {}", all.len());

    let page = |offset: &str| demo_ids(&["clients", "list", "--limit", "25", "--offset", offset]);
    let first = page("0");
    let second = page("25");
    assert_eq!(first.len(), 25);
    assert_eq!(second.len(), 25);
    assert!(first.iter().all(|id| !second.contains(id)), "pages overlap");
    assert_eq!(
        [first, second].concat(),
        all[..50],
        "pages must tile the full listing in order"
    );
}

#[test]
fn test_demo_fleet_firewall_policies_list_in_evaluation_order() {
    let zones = demo_json(&["firewall", "zones", "list", "--all"]);
    let zone_name = |id: &serde_json::Value| -> String {
        zones
            .as_array()
            .unwrap()
            .iter()
            .find(|zone| zone["id"] == *id)
            .and_then(|zone| zone["name"].as_str())
            .unwrap_or_else(|| panic!("policy references unknown zone {id}"))
            .to_lowercase()
    };

    let policies = demo_json(&["firewall", "policies", "list", "--all"]);
    let keys: Vec<(String, String, i64)> = policies
        .as_array()
        .unwrap()
        .iter()
        .map(|policy| {
            (
                zone_name(&policy["source"]["zone_id"]),
                zone_name(&policy["destination"]["zone_id"]),
                policy["index"].as_i64().unwrap(),
            )
        })
        .collect();
    assert!(keys.len() > 5, "too few policies: {keys:?}");

    let mut expected = keys.clone();
    expected.sort();
    assert_eq!(
        keys, expected,
        "policies must group by zone pair and run by ascending index"
    );
}

// ── Mock controller ─────────────────────────────────────────────────

const MOCK_SITE_ID: &str = "550e8400-e29b-41d4-a716-446655440000";
const MOCK_API_KEY: &str = "cli-test-key";

/// A UniFi OS controller that answers an API-key connect with one empty
/// site. Tests mount the routes they exercise at a higher priority.
async fn mock_api_key_controller() -> wiremock::MockServer {
    use wiremock::matchers::{method, path, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    let empty_page = serde_json::json!({
        "offset": 0, "limit": 200, "count": 0, "totalCount": 0, "data": [],
    });

    Mock::given(method("GET"))
        .and(path("/api/auth/login"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/proxy/network/integration/v1/sites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "offset": 0, "limit": 50, "count": 1, "totalCount": 1,
            "data": [{ "id": MOCK_SITE_ID, "internalReference": "default", "name": "Default" }],
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex("^/proxy/network/integration/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_page))
        .with_priority(10)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex("^/proxy/network/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "meta": { "rc": "ok" }, "data": [],
        })))
        .with_priority(10)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex("^/proxy/network/v2/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .with_priority(10)
        .mount(&server)
        .await;
    server
}

fn mock_cmd(server: &wiremock::MockServer) -> assert_cmd::Command {
    let mut cmd = unifly_cmd();
    cmd.args([
        "-c",
        &server.uri(),
        "--api-key",
        MOCK_API_KEY,
        "--timeout",
        "5",
    ]);
    cmd
}

const MOCK_WIFI_ID: &str = "6b0e5f4e-1c2d-4e5f-8a9b-0c1d2e3f4a5b";

async fn mock_wifi_detail(server: &wiremock::MockServer, response: wiremock::ResponseTemplate) {
    use wiremock::matchers::{method, path};
    wiremock::Mock::given(method("GET"))
        .and(path(format!(
            "/proxy/network/integration/v1/sites/{MOCK_SITE_ID}/wifi/broadcasts/{MOCK_WIFI_ID}"
        )))
        .respond_with(response)
        .with_priority(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn test_wifi_get_reports_failures_by_kind() {
    use wiremock::ResponseTemplate;

    let rejected = mock_api_key_controller().await;
    mock_wifi_detail(&rejected, ResponseTemplate::new(401)).await;
    mock_cmd(&rejected)
        .args(["wifi", "get", MOCK_WIFI_ID])
        .assert()
        .failure()
        .code(3)
        .stderr(predicate::str::contains("Authentication failed"))
        .stderr(predicate::str::contains("not found").not());

    let broken = mock_api_key_controller().await;
    mock_wifi_detail(
        &broken,
        ResponseTemplate::new(500).set_body_json(serde_json::json!({ "message": "boom" })),
    )
    .await;
    mock_cmd(&broken)
        .args(["wifi", "get", MOCK_WIFI_ID])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("boom"))
        .stderr(predicate::str::contains("not found").not());

    let missing = mock_api_key_controller().await;
    mock_wifi_detail(&missing, ResponseTemplate::new(404)).await;
    for identifier in [MOCK_WIFI_ID, "No Such SSID"] {
        mock_cmd(&missing)
            .args(["wifi", "get", identifier])
            .assert()
            .failure()
            .code(4)
            .stderr(predicate::str::contains(format!(
                "WiFi broadcast '{identifier}' not found"
            )));
    }
}

#[test]
fn test_not_found_hints_name_real_commands_spelled_right() {
    for (args, list) in [
        (
            &["firewall", "policies", "get", "nope"][..],
            "firewall policies list",
        ),
        (&["nat", "policies", "get", "nope"][..], "nat policies list"),
        (&["wifi", "get", "nope"][..], "wifi list"),
    ] {
        let output = unifly_cmd()
            .arg("--demo-fleet")
            .args(args)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(4),
            "{}",
            combined_output(&output)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(&format!("unifly {list}")), "{stderr}");
        assert!(!stderr.contains("policys"), "{stderr}");
    }
}

#[test]
fn test_unusable_controller_url_is_not_a_missing_config_file() {
    for url in ["foo:bar", "mailto:admin@example.com"] {
        unifly_cmd()
            .args(["-c", url, "--api-key", "k", "devices", "list"])
            .assert()
            .failure()
            .code(2)
            .stderr(predicate::str::contains("Configuration file not found").not())
            .stderr(predicate::str::contains("controller URL"));
    }
}

/// A controller whose API key check fails during connect.
async fn mock_rejecting_controller() -> wiremock::MockServer {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};

    let server = mock_api_key_controller().await;
    Mock::given(method("GET"))
        .and(path("/proxy/network/integration/v1/sites"))
        .respond_with(ResponseTemplate::new(401))
        .with_priority(1)
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn test_auth_failure_help_names_the_profile_in_use() {
    let server = mock_rejecting_controller().await;

    // Ad-hoc flags: the rejected key is the flag's, nothing is stored.
    let output = mock_cmd(&server)
        .args(["devices", "list"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        combined_output(&output)
    );
    let stderr = squeezed_stderr(&output);
    assert!(stderr.contains("Authentication failed"), "{stderr}");
    assert!(stderr.contains("--api-key"), "{stderr}");
    assert!(!stderr.contains("current"), "{stderr}");
    assert!(!stderr.contains("set-password"), "{stderr}");

    let tempdir = tempfile::tempdir().unwrap();
    for (key, value) in [
        ("controller", server.uri()),
        ("auth_mode", "integration".into()),
        ("api_key", MOCK_API_KEY.into()),
    ] {
        unifly_cmd_in(tempdir.path())
            .args(["config", "set", &format!("profiles.my lab.{key}"), &value])
            .assert()
            .success();
    }

    // The profile's own key was rejected: replace the stored secret.
    let output = unifly_cmd_in(tempdir.path())
        .args(["-p", "my lab", "devices", "list"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        combined_output(&output)
    );
    let stderr = squeezed_stderr(&output);
    assert!(
        stderr.contains("unifly config set-password 'my lab'"),
        "{stderr}"
    );
    assert!(!stderr.contains("current"), "{stderr}");

    // An explicit --api-key overrides the profile, so the profile's stored
    // secret is not what failed.
    let output = unifly_cmd_in(tempdir.path())
        .args(["-p", "my lab", "--api-key", "other-key", "devices", "list"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        combined_output(&output)
    );
    let stderr = squeezed_stderr(&output);
    assert!(stderr.contains("--api-key"), "{stderr}");
    assert!(!stderr.contains("set-password"), "{stderr}");
}

#[tokio::test]
async fn test_demo_flag_never_prints_unsanitized_cli_output() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};

    let server = mock_api_key_controller().await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/proxy/network/integration/v1/sites/{MOCK_SITE_ID}/clients"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "offset": 0, "limit": 200, "count": 1, "totalCount": 1,
            "data": [{
                "id": "c56a4180-65aa-42ec-a945-5fd21dec0538",
                "name": "Bob's iPhone",
                "type": "WIRELESS",
                "ipAddress": "10.0.0.50",
                "connectedAt": null,
                "macAddress": "aa:bb:cc:dd:ee:ff",
                "access": {},
            }],
        })))
        .with_priority(1)
        .mount(&server)
        .await;

    // Without --demo the client shows up as is.
    mock_cmd(&server)
        .args(["clients", "list", "-o", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Bob's iPhone"));

    for demo in [["--demo"].as_slice(), [].as_slice()] {
        let mut cmd = mock_cmd(&server);
        if demo.is_empty() {
            cmd.env("UNIFI_DEMO", "1");
        }
        cmd.args(demo)
            .args(["clients", "list", "-o", "json"])
            .assert()
            .failure()
            .code(5)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("demo mode"))
            .stderr(predicate::str::contains("Bob").not());
    }
}

#[test]
fn test_demo_flag_still_runs_offline_and_synthetic_commands() {
    unifly_cmd()
        .args(["--demo", "--demo-fleet", "clients", "list", "-o", "json"])
        .assert()
        .success();
    let tempdir = tempfile::tempdir().unwrap();
    unifly_cmd_in(tempdir.path())
        .args(["--demo", "config", "profiles"])
        .assert()
        .success();
    unifly_cmd()
        .args(["--demo", "completions", "bash"])
        .assert()
        .success();
    unifly_cmd().args(["--demo", "skill"]).assert().success();
}

#[tokio::test]
async fn test_demo_mode_from_the_config_file_is_refused_too() {
    let server = mock_api_key_controller().await;
    let tempdir = tempfile::tempdir().unwrap();
    for (key, value) in [
        ("controller", server.uri()),
        ("auth_mode", "integration".into()),
    ] {
        unifly_cmd_in(tempdir.path())
            .args(["config", "set", &format!("profiles.lab.{key}"), &value])
            .assert()
            .success();
    }
    let path = config_file(&tempdir);
    let cfg = std::fs::read_to_string(&path).unwrap();
    assert!(cfg.contains("[demo]\nenabled = false\n"), "{cfg}");
    std::fs::write(
        &path,
        cfg.replace("[demo]\nenabled = false\n", "[demo]\nenabled = true\n"),
    )
    .unwrap();

    // The refusal names the file and key that turn demo mode off.
    let output = unifly_cmd_in(tempdir.path())
        .args(["-p", "lab", "--api-key", MOCK_API_KEY, "clients", "list"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(5),
        "{}",
        combined_output(&output)
    );
    assert!(output.stdout.is_empty(), "{}", combined_output(&output));
    // miette wraps help text to the terminal width, even inside a path, so
    // compare without whitespace.
    let squeezed = |text: &str| text.split_whitespace().collect::<String>();
    let stderr = squeezed(&String::from_utf8_lossy(&output.stderr));
    assert!(stderr.contains("enabled=false"), "{stderr}");
    assert!(
        stderr.contains(&squeezed(&path.display().to_string())),
        "{stderr}"
    );

    // Config commands stay usable, so the switch can be inspected and
    // profiles managed while it is on.
    for args in [&["config", "show"][..], &["config", "profiles"][..]] {
        unifly_cmd_in(tempdir.path()).args(args).assert().success();
    }

    // The synthetic fleet never reads the config, so it still answers.
    unifly_cmd_in(tempdir.path())
        .args(["--demo-fleet", "clients", "list"])
        .assert()
        .success();
}

#[tokio::test]
async fn test_wifi_channels_renders_every_country_as_one_document() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};

    let server = mock_api_key_controller().await;
    Mock::given(method("GET"))
        .and(path("/proxy/network/api/s/default/stat/current-channel"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "meta": { "rc": "ok" },
            "data": [
                { "code": "840", "key": "US", "name": "United States",
                  "channels_ng": [1, 6, 11], "channels_na": [36, 40] },
                { "code": "124", "key": "CA", "name": "Canada",
                  "channels_ng": [1, 6, 11] },
            ],
        })))
        .with_priority(1)
        .mount(&server)
        .await;

    let json_rows = |format: &str| -> Vec<serde_json::Value> {
        let output = mock_cmd(&server)
            .args(["wifi", "channels", "-o", format])
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", combined_output(&output));
        let rows: serde_json::Value = if format == "yaml" {
            serde_yaml_ng::from_slice(&output.stdout)
                .unwrap_or_else(|err| panic!("invalid YAML ({err}): {}", combined_output(&output)))
        } else {
            serde_json::from_slice(&output.stdout)
                .unwrap_or_else(|err| panic!("invalid JSON ({err}): {}", combined_output(&output)))
        };
        rows.as_array().cloned().unwrap_or_default()
    };

    for format in ["json", "json-compact", "yaml"] {
        let rows = json_rows(format);
        let countries: Vec<&str> = rows
            .iter()
            .filter_map(|row| row["country"].as_str())
            .collect();
        assert_eq!(
            countries,
            ["United States", "United States", "Canada"],
            "{format}: {rows:?}"
        );
    }

    let plain = mock_cmd(&server)
        .args(["wifi", "channels", "-o", "plain"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&plain.stdout).lines().count(),
        3,
        "{}",
        combined_output(&plain)
    );
}

#[test]
fn test_unsupported_controller_scheme_is_an_invalid_configuration() {
    for command in [
        &["devices", "list"][..],
        &["system", "info"][..],
        &["cloud", "hosts"][..],
    ] {
        let output = unifly_cmd()
            .args(["-c", "ftp://192.0.2.1", "--api-key", "k"])
            .args(command)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "{}",
            combined_output(&output)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("'ftp'"), "{stderr}");
        assert!(!stderr.contains("builder error"), "{stderr}");
    }

    let tempdir = tempfile::tempdir().unwrap();
    unifly_cmd_in(tempdir.path())
        .args([
            "config",
            "set",
            "profiles.lab.controller",
            "gopher://192.0.2.1",
        ])
        .assert()
        .success();
    unifly_cmd_in(tempdir.path())
        .args(["-p", "lab", "--api-key", "k", "devices", "list"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("'gopher'"));
}

#[tokio::test]
async fn test_vpn_lists_render_yaml_and_json_without_duplicate_keys() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};

    let server = mock_api_key_controller().await;
    for (route, record) in [
        (
            "vpn/servers",
            serde_json::json!({
                "id": "11111111-1111-1111-1111-111111111111",
                "name": "Home WG",
                "type": "WIREGUARD",
                "enabled": true,
                "port": 51820,
                "mtu": 1420,
            }),
        ),
        (
            "vpn/site-to-site-tunnels",
            serde_json::json!({
                "id": "22222222-2222-2222-2222-222222222222",
                "name": "Branch",
                "type": "IPSEC",
                "enabled": false,
                "preSharedKey": "hunter2",
                "lifetime": 3600,
            }),
        ),
    ] {
        Mock::given(method("GET"))
            .and(path(format!(
                "/proxy/network/integration/v1/sites/{MOCK_SITE_ID}/{route}"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "offset": 0, "limit": 200, "count": 1, "totalCount": 1, "data": [record],
            })))
            .with_priority(1)
            .mount(&server)
            .await;
    }

    for (command, unmapped) in [("servers", "mtu"), ("tunnels", "lifetime")] {
        let yaml = mock_cmd(&server)
            .args(["vpn", command, "-o", "yaml"])
            .output()
            .unwrap();
        assert!(yaml.status.success(), "{}", combined_output(&yaml));
        let parsed: serde_json::Value = serde_yaml_ng::from_slice(&yaml.stdout)
            .unwrap_or_else(|err| panic!("invalid YAML ({err}): {}", combined_output(&yaml)));
        assert!(parsed[0][unmapped].as_i64().is_some(), "{parsed}");

        let json = mock_cmd(&server)
            .args(["vpn", command, "-o", "json-compact"])
            .output()
            .unwrap();
        assert!(json.status.success(), "{}", combined_output(&json));
        let text = String::from_utf8_lossy(&json.stdout);
        for key in ["\"id\":", "\"name\":", "\"enabled\":"] {
            assert_eq!(text.matches(key).count(), 1, "{key} repeated in {text}");
        }
        assert!(!text.contains("hunter2"), "{text}");
    }
}

#[test]
fn test_controller_url_errors_never_echo_a_password() {
    for url in [
        "ftp://admin:hunter2@192.0.2.1",
        "http://admin:hunter2@[::1",
        "gopher://admin:hunter2@192.0.2.1/x",
    ] {
        for command in [&["devices", "list"][..], &["cloud", "hosts"][..]] {
            let output = unifly_cmd()
                .args(["-c", url, "--api-key", "k"])
                .args(command)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(2),
                "{}",
                combined_output(&output)
            );
            let text = combined_output(&output);
            assert!(!text.contains("hunter2"), "{url} {command:?}: {text}");
            assert!(text.contains("***@"), "{url} {command:?}: {text}");
        }
    }
}
