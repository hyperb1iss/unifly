#![cfg(feature = "e2e")]
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;
use std::process::Output;

use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

struct E2eConfig {
    controller_url: String,
    username: String,
    password: String,
    site: String,
}

impl E2eConfig {
    fn from_env() -> Self {
        Self {
            controller_url: std::env::var("UNIFLY_E2E_URL")
                .unwrap_or_else(|_| "https://localhost:8443".into()),
            username: std::env::var("UNIFLY_E2E_USERNAME").unwrap_or_else(|_| "admin".into()),
            password: std::env::var("UNIFLY_E2E_PASSWORD").unwrap_or_else(|_| "admin".into()),
            site: std::env::var("UNIFLY_E2E_SITE").unwrap_or_else(|_| "default".into()),
        }
    }

    fn with_password(&self, password: &str) -> Self {
        Self {
            controller_url: self.controller_url.clone(),
            username: self.username.clone(),
            password: password.into(),
            site: self.site.clone(),
        }
    }
}

struct E2eContext {
    config: E2eConfig,
    tempdir: TempDir,
}

impl E2eContext {
    fn session() -> Self {
        Self::with_config(E2eConfig::from_env(), "session")
    }

    fn session_with_password(password: &str) -> Self {
        let config = E2eConfig::from_env().with_password(password);
        Self::with_config(config, "session")
    }

    fn with_config(config: E2eConfig, auth_mode: &str) -> Self {
        let tempdir = tempfile::tempdir().unwrap();
        write_profile(tempdir.path(), &config, auth_mode);
        Self { config, tempdir }
    }

    fn cmd(&self) -> assert_cmd::Command {
        let mut cmd = cargo_bin_cmd!("unifly");
        cmd.env("HOME", self.tempdir.path())
            .env("XDG_CONFIG_HOME", self.tempdir.path())
            .env("APPDATA", self.tempdir.path())
            .env("LOCALAPPDATA", self.tempdir.path())
            .env("USERPROFILE", self.tempdir.path())
            .env("UNIFI_USERNAME", &self.config.username)
            .env("UNIFI_PASSWORD", &self.config.password)
            .env("UNIFI_TIMEOUT", "60")
            .env_remove("UNIFI_PROFILE")
            .env_remove("UNIFI_API_KEY");
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.cmd().args(args).output().unwrap()
    }
}

fn write_profile(root: &Path, config: &E2eConfig, auth_mode: &str) {
    let config_dir = root.join("unifly");
    fs::create_dir_all(&config_dir).unwrap();

    let body = format!(
        r#"
default_profile = "default"

[profiles.default]
controller = "{controller}"
site = "{site}"
auth_mode = "{auth_mode}"
insecure = true
"#,
        controller = config.controller_url,
        site = config.site,
        auth_mode = auth_mode,
    );

    fs::write(config_dir.join("config.toml"), body.trim_start()).unwrap();
}

fn stdout_json(output: &std::process::Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed, stderr:\n{}",
        stderr_text(output)
    );
}

fn assert_exit_code(output: &Output, expected: i32, context: &str) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "{context} exited unexpectedly, stderr:\n{}",
        stderr_text(output)
    );
}

fn json_array<'a>(payload: &'a serde_json::Value, context: &str) -> &'a [serde_json::Value] {
    payload
        .as_array()
        .unwrap_or_else(|| panic!("expected {context} to render a JSON array, got {payload}"))
}

fn json_bool(payload: &serde_json::Value, key: &str) -> bool {
    payload
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or_else(|| panic!("expected boolean field '{key}' in {payload}"))
}

fn bool_arg(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn json_str_field<'a>(payload: &'a serde_json::Value, key: &str) -> &'a str {
    payload
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("expected string field '{key}' in {payload}"))
}

fn list_firewall_groups(ctx: &E2eContext) -> Vec<serde_json::Value> {
    let output = ctx.run(&["firewall", "groups", "list", "--all", "-o", "json"]);
    assert_success(&output, "firewall groups list");
    let payload = stdout_json(&output);
    json_array(&payload, "firewall groups list").to_vec()
}

fn find_firewall_group<'a>(
    groups: &'a [serde_json::Value],
    name: &str,
) -> Option<&'a serde_json::Value> {
    groups.iter().find(|group| {
        group
            .get("name")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| value == name)
    })
}

fn list_nat_rules_raw(ctx: &E2eContext) -> Vec<serde_json::Value> {
    let path = format!("v2/api/site/{}/nat", ctx.config.site);
    let output = ctx.run(&["api", &path, "-o", "json"]);
    assert_success(&output, "raw api v2 nat list");
    let payload = stdout_json(&output);
    json_array(&payload, "raw api v2 nat list").to_vec()
}

fn strip_jsonc_comments(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

struct RestoreCommand<'a> {
    ctx: &'a E2eContext,
    args: Vec<String>,
}

impl<'a> RestoreCommand<'a> {
    fn new(ctx: &'a E2eContext, args: &[&str]) -> Self {
        Self {
            ctx,
            args: args.iter().map(ToString::to_string).collect(),
        }
    }
}

impl Drop for RestoreCommand<'_> {
    fn drop(&mut self) {
        let args = self.args.iter().map(String::as_str).collect::<Vec<_>>();
        let _ = self.ctx.run(&args);
    }
}

#[test]
fn session_profile_smoke_test_reads_sysinfo() {
    let ctx = E2eContext::session();

    let output = ctx.run(&["system", "sysinfo", "-o", "json"]);

    assert_success(&output, "system sysinfo");

    let payload = stdout_json(&output);
    assert!(payload.is_object(), "expected JSON object, got {payload}");
}

#[test]
fn session_profile_lists_simulated_devices() {
    let ctx = E2eContext::session();
    let output = ctx.run(&["devices", "list", "--all", "-o", "json"]);

    assert_success(&output, "devices list");

    let payload = stdout_json(&output);
    let devices = json_array(&payload, "devices list");

    assert!(
        !devices.is_empty(),
        "expected simulation mode to expose demo devices"
    );
}

#[test]
fn session_profile_reads_first_device_detail() {
    let ctx = E2eContext::session();

    let list_output = ctx.run(&["devices", "list", "--all", "-o", "json"]);
    assert_success(&list_output, "devices list for detail lookup");
    let list_payload = stdout_json(&list_output);
    let devices = json_array(&list_payload, "devices list");
    let first_device = devices
        .first()
        .expect("expected at least one simulated device for detail lookup");
    let device_id = first_device
        .get("id")
        .or_else(|| first_device.get("mac"))
        .and_then(serde_json::Value::as_str)
        .expect("expected the simulated device to expose an id or mac");

    let detail_output = ctx.run(&["devices", "get", device_id, "-o", "json"]);
    assert_success(&detail_output, "devices get");
    let detail = stdout_json(&detail_output);

    assert_eq!(
        detail
            .get("id")
            .and_then(serde_json::Value::as_str)
            .expect("device detail should include an id"),
        device_id
    );
    assert!(
        detail
            .get("device_type")
            .and_then(serde_json::Value::as_str)
            .is_some(),
        "expected device detail to include a device_type, got {detail}"
    );

    let stats_output = ctx.run(&["devices", "stats", device_id, "-o", "json"]);
    assert_success(&stats_output, "devices stats");
    let stats = stdout_json(&stats_output);
    assert_eq!(
        stats
            .get("id")
            .and_then(serde_json::Value::as_str)
            .expect("device stats should include an id"),
        device_id
    );
    assert!(
        stats
            .get("stats")
            .and_then(serde_json::Value::as_object)
            .is_some(),
        "expected device stats to include a stats object, got {stats}"
    );
}

#[test]
fn session_profile_lists_default_site() {
    let ctx = E2eContext::session();
    let output = ctx.run(&["sites", "list", "--all", "-o", "json"]);

    assert_success(&output, "sites list");

    let payload = stdout_json(&output);
    let sites = json_array(&payload, "sites list");

    assert!(
        sites.iter().any(|site| {
            site.get("name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|name| name == "Default" || name == "default")
        }),
        "expected the default site to be present, got {}",
        stdout_text(&output)
    );
}

#[test]
fn session_profile_raw_api_sysinfo_returns_json() {
    let ctx = E2eContext::session();
    let path = format!("api/s/{}/stat/sysinfo", ctx.config.site);
    let output = ctx
        .cmd()
        .args(["api", &path, "-o", "json"])
        .output()
        .unwrap();

    assert_success(&output, "raw api sysinfo");

    let payload = stdout_json(&output);
    assert!(payload.is_object(), "expected JSON object, got {payload}");
}

#[test]
fn session_profile_output_formats_render_cleanly() {
    let ctx = E2eContext::session();

    let table = ctx.run(&["sites", "list", "--all"]);
    assert_success(&table, "sites list table output");
    assert!(stdout_text(&table).contains("default") || stdout_text(&table).contains("Default"));

    let yaml = ctx.run(&["sites", "list", "--all", "-o", "yaml"]);
    assert_success(&yaml, "sites list yaml output");
    assert!(stdout_text(&yaml).contains("name:"));

    let plain = ctx.run(&["sites", "list", "--all", "-o", "plain"]);
    assert_success(&plain, "sites list plain output");
    assert!(!stdout_text(&plain).trim().is_empty());
}

#[test]
fn session_profile_reports_system_health_subsystems() {
    let ctx = E2eContext::session();

    let output = ctx.run(&["system", "health", "-o", "json"]);
    assert_success(&output, "system health");

    let payload = stdout_json(&output);
    let health = json_array(&payload, "system health");
    assert!(
        !health.is_empty(),
        "expected system health to return subsystem entries"
    );

    for subsystem in ["lan", "wlan", "wan", "vpn", "www"] {
        assert!(
            health.iter().any(|entry| {
                entry
                    .get("subsystem")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|value| value == subsystem)
            }),
            "expected system health to include subsystem '{subsystem}', got {}",
            stdout_text(&output)
        );
    }
}

#[test]
fn session_profile_lists_pending_devices() {
    let ctx = E2eContext::session();

    let output = ctx.run(&["devices", "pending", "--all", "-o", "json"]);
    assert_success(&output, "devices pending");

    let payload = stdout_json(&output);
    let pending = json_array(&payload, "devices pending");
    assert!(
        !pending.is_empty(),
        "expected simulation mode to expose pending devices"
    );
    assert!(
        pending.iter().all(|device| {
            device
                .get("state")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|state| state == "PendingAdoption")
        }),
        "expected pending devices to remain in PendingAdoption, got {}",
        stdout_text(&output)
    );
}

#[test]
fn session_profile_lists_session_settings_and_export() {
    let ctx = E2eContext::session();

    let settings_output = ctx.run(&["settings", "list", "-o", "json"]);
    assert_success(&settings_output, "settings list");
    let settings_payload = stdout_json(&settings_output);
    let settings = json_array(&settings_payload, "settings list");

    for key in ["dpi", "teleport", "magic_site_to_site_vpn"] {
        assert!(
            settings.iter().any(|setting| {
                setting
                    .get("key")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|value| value == key)
            }),
            "expected settings list to include '{key}', got {}",
            stdout_text(&settings_output)
        );
    }

    let export = ctx.run(&["settings", "export", "-o", "json"]);
    assert_success(&export, "settings export");
    let export_payload = stdout_json(&export);
    let exported = json_array(&export_payload, "settings export");
    assert!(
        exported.iter().any(|setting| {
            setting
                .get("key")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| value == "dpi")
        }),
        "expected settings export to include dpi, got {}",
        stdout_text(&export)
    );
}

#[test]
fn session_profile_reads_dpi_setting() {
    let ctx = E2eContext::session();

    let output = ctx.run(&["settings", "get", "dpi", "-o", "json"]);
    assert_success(&output, "settings get dpi");

    let payload = stdout_json(&output);
    assert_eq!(
        payload
            .get("key")
            .and_then(serde_json::Value::as_str)
            .expect("dpi setting should include a key"),
        "dpi"
    );
    assert!(json_bool(&payload, "enabled"));
    assert!(json_bool(&payload, "fingerprintingEnabled"));
}

#[test]
fn session_profile_can_toggle_dpi_setting() {
    let ctx = E2eContext::session();

    let initial = ctx.run(&["settings", "get", "dpi", "-o", "json"]);
    assert_success(&initial, "settings get dpi before toggle");
    let initial_payload = stdout_json(&initial);
    let original_enabled = json_bool(&initial_payload, "enabled");
    let toggled_enabled = !original_enabled;
    let restore = RestoreCommand::new(
        &ctx,
        &[
            "settings",
            "set",
            "dpi",
            "enabled",
            bool_arg(original_enabled),
        ],
    );

    let update = ctx.run(&[
        "settings",
        "set",
        "dpi",
        "enabled",
        bool_arg(toggled_enabled),
    ]);
    assert_success(&update, "settings set dpi enabled");

    let after = ctx.run(&["settings", "get", "dpi", "-o", "json"]);
    assert_success(&after, "settings get dpi after toggle");
    let after_payload = stdout_json(&after);
    assert_eq!(json_bool(&after_payload, "enabled"), toggled_enabled);

    drop(restore);
}

#[test]
fn session_profile_lists_session_vpn_settings() {
    let ctx = E2eContext::session();

    let output = ctx.run(&["vpn", "settings", "list", "--all", "-o", "json"]);
    assert_success(&output, "vpn settings list");

    let payload = stdout_json(&output);
    let settings = json_array(&payload, "vpn settings list");

    for key in ["magic_site_to_site_vpn", "peer_to_peer", "teleport"] {
        assert!(
            settings.iter().any(|setting| {
                setting
                    .get("key")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|value| value == key)
            }),
            "expected vpn settings list to include '{key}', got {}",
            stdout_text(&output)
        );
    }
}

#[test]
fn session_profile_can_toggle_teleport_vpn_setting() {
    let ctx = E2eContext::session();

    let initial = ctx.run(&["vpn", "settings", "get", "teleport", "-o", "json"]);
    assert_success(&initial, "vpn settings get teleport before toggle");
    let initial_payload = stdout_json(&initial);
    let original_enabled = json_bool(&initial_payload, "enabled");
    let toggled_enabled = !original_enabled;
    let restore = RestoreCommand::new(
        &ctx,
        &[
            "vpn",
            "settings",
            "set",
            "teleport",
            "--enabled",
            bool_arg(original_enabled),
        ],
    );

    let update = ctx.run(&[
        "vpn",
        "settings",
        "set",
        "teleport",
        "--enabled",
        bool_arg(toggled_enabled),
    ]);
    assert_success(&update, "vpn settings set teleport");

    let after = ctx.run(&["vpn", "settings", "get", "teleport", "-o", "json"]);
    assert_success(&after, "vpn settings get teleport after toggle");
    let after_payload = stdout_json(&after);
    assert_eq!(json_bool(&after_payload, "enabled"), toggled_enabled);

    drop(restore);
}

#[test]
fn session_profile_lists_empty_nat_and_vpn_collections() {
    let ctx = E2eContext::session();

    for args in [
        ["nat", "policies", "list", "--all", "-o", "json"],
        ["vpn", "clients", "list", "--all", "-o", "json"],
        ["vpn", "connections", "list", "--all", "-o", "json"],
        ["vpn", "remote-access", "list", "--all", "-o", "json"],
        ["vpn", "site-to-site", "list", "--all", "-o", "json"],
    ] {
        let output = ctx.run(&args);
        let context = args.join(" ");
        assert_success(&output, &context);

        let payload = stdout_json(&output);
        let values = json_array(&payload, &context);
        assert!(
            values.is_empty(),
            "expected '{context}' to be empty in simulation mode, got {}",
            stdout_text(&output)
        );
    }
}

#[test]
fn session_profile_renders_session_observability_collections() {
    let ctx = E2eContext::session();

    for args in [
        &["events", "list", "-o", "json"][..],
        &["clients", "list", "--all", "-o", "json"][..],
        &["stats", "client", "-o", "json"][..],
        &["stats", "gateway", "-o", "json"][..],
        &["wifi", "neighbors", "--all", "-o", "json"][..],
    ] {
        let output = ctx.run(args);
        let context = args.join(" ");
        assert_success(&output, &context);

        let payload = stdout_json(&output);
        let values = json_array(&payload, &context);
        assert!(
            values.iter().all(serde_json::Value::is_object),
            "expected '{context}' to render an array of JSON objects, got {}",
            stdout_text(&output)
        );
    }
}

#[test]
fn session_profile_lists_empty_session_reference_collections() {
    let ctx = E2eContext::session();

    for args in [
        &["devices", "tags", "--all", "-o", "json"][..],
        &["clients", "reservations", "--all", "-o", "json"][..],
        &["system", "backup", "list", "-o", "json"][..],
    ] {
        let output = ctx.run(args);
        let context = args.join(" ");
        assert_success(&output, &context);

        let payload = stdout_json(&output);
        let values = json_array(&payload, &context);
        assert!(
            values.is_empty(),
            "expected '{context}' to be empty in simulation mode, got {}",
            stdout_text(&output)
        );
    }
}

#[test]
fn session_profile_renders_stats_and_wifi_channel_data() {
    let ctx = E2eContext::session();

    let site_stats = ctx.run(&["stats", "site", "-o", "json"]);
    assert_success(&site_stats, "stats site");
    let site_payload = stdout_json(&site_stats);
    let site_values = json_array(&site_payload, "stats site");
    let first_site = site_values
        .first()
        .expect("expected stats site to return at least one datapoint");
    assert_eq!(
        first_site
            .get("o")
            .and_then(serde_json::Value::as_str)
            .expect("site stats row should include object type"),
        "site"
    );
    assert!(
        first_site
            .get("time")
            .and_then(serde_json::Value::as_i64)
            .is_some(),
        "expected stats site rows to include a numeric timestamp, got {first_site}"
    );

    let device_stats = ctx.run(&["stats", "device", "-o", "json"]);
    assert_success(&device_stats, "stats device");
    let device_payload = stdout_json(&device_stats);
    let device_values = json_array(&device_payload, "stats device");
    let first_device = device_values
        .first()
        .expect("expected stats device to return at least one datapoint");
    assert!(
        first_device
            .get("oid")
            .and_then(serde_json::Value::as_str)
            .is_some(),
        "expected stats device rows to include an oid, got {first_device}"
    );

    for group_by in ["by-cat", "by-app"] {
        let output = ctx.run(&["stats", "dpi", "--group-by", group_by, "-o", "json"]);
        let context = format!("stats dpi --group-by {group_by}");
        assert_success(&output, &context);
        let payload = stdout_json(&output);
        let values = json_array(&payload, &context);
        assert!(
            !values.is_empty(),
            "expected '{context}' to return at least one JSON object"
        );
        assert!(
            values.iter().all(serde_json::Value::is_object),
            "expected '{context}' to return JSON objects, got {}",
            stdout_text(&output)
        );
    }

    let channels = ctx.run(&["wifi", "channels", "-o", "json"]);
    assert_success(&channels, "wifi channels");
    let channel_payload = stdout_json(&channels);
    let channel_rows = json_array(&channel_payload, "wifi channels");

    for band in ["2.4 GHz", "5 GHz", "6 GHz"] {
        assert!(
            channel_rows.iter().any(|row| {
                row.get("band")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|value| value == band)
            }),
            "expected wifi channels to include '{band}', got {}",
            stdout_text(&channels)
        );
    }
}

#[test]
fn session_profile_client_resolution_errors_include_list_guidance() {
    let ctx = E2eContext::session();

    for args in [
        ["clients", "find", "definitely-not-a-client", "-o", "json"],
        ["clients", "get", "aa:bb:cc:dd:ee:ff", "-o", "json"],
        ["clients", "roams", "definitely-not-a-client", "-o", "json"],
        ["clients", "wifi", "definitely-not-a-client", "-o", "json"],
    ] {
        let output = ctx.run(&args);
        let context = args.join(" ");
        assert_exit_code(&output, 4, &context);
        assert!(
            stderr_text(&output).contains("clients list"),
            "expected '{context}' to guide toward clients list, got:\n{}",
            stderr_text(&output)
        );
    }
}

#[test]
fn session_profile_rejects_invalid_stats_time_range() {
    let ctx = E2eContext::session();

    let output = ctx.run(&[
        "stats",
        "site",
        "--start",
        "2026-01-02T00:00:00Z",
        "--end",
        "2026-01-01T00:00:00Z",
        "-o",
        "json",
    ]);

    assert_exit_code(&output, 2, "stats site invalid time range");
    assert!(
        stderr_text(&output).contains("start must be <="),
        "expected validation guidance, got:\n{}",
        stderr_text(&output)
    );
}

#[test]
fn session_profile_destructive_commands_require_yes_in_noninteractive_mode() {
    let ctx = E2eContext::session();

    for args in [
        &["devices", "remove", "00:27:22:00:00:03", "-o", "json"][..],
        &["clients", "forget", "aa:bb:cc:dd:ee:ff", "-o", "json"][..],
        &["system", "backup", "delete", "fake.unf", "-o", "json"][..],
    ] {
        let output = ctx.run(args);
        let context = args.join(" ");
        assert_exit_code(&output, 2, &context);
        assert!(
            stderr_text(&output).contains("Use --yes (-y)"),
            "expected '{context}' to require explicit confirmation, got:\n{}",
            stderr_text(&output)
        );
    }
}

#[test]
fn session_profile_wrong_credentials_fail_with_auth_exit_code() {
    let ctx = E2eContext::session_with_password("not-the-right-password");
    let output = ctx.run(&["system", "sysinfo", "-o", "json"]);

    assert_eq!(
        output.status.code(),
        Some(3),
        "stderr:\n{}",
        stderr_text(&output)
    );
    assert!(
        stderr_text(&output).contains("Authentication failed"),
        "expected auth failure, got:\n{}",
        stderr_text(&output)
    );
}

#[test]
fn session_profile_firewall_group_crud_round_trip() {
    let ctx = E2eContext::session();
    let name = "e2e-fwg-crud";

    // A failed earlier run against a still-running container may have left
    // the group behind; remove it so the create below starts clean.
    if let Some(stale) = find_firewall_group(&list_firewall_groups(&ctx), name) {
        let stale_id = json_str_field(stale, "id").to_string();
        let _ = ctx.run(&["firewall", "groups", "delete", &stale_id, "-y"]);
    }

    let create = ctx.run(&[
        "firewall",
        "groups",
        "create",
        "--name",
        name,
        "--type",
        "port-group",
        "--members",
        "80,443",
    ]);
    assert_success(&create, "firewall groups create");
    assert!(
        create.stdout.is_empty(),
        "expected create to print nothing on stdout (confirmation goes to stderr), got {}",
        stdout_text(&create)
    );
    assert!(
        stderr_text(&create).contains("Firewall group created"),
        "expected create confirmation on stderr, got:\n{}",
        stderr_text(&create)
    );

    let groups = list_firewall_groups(&ctx);
    let created = find_firewall_group(&groups, name)
        .unwrap_or_else(|| panic!("expected created group '{name}' in list, got {groups:?}"));
    let id = json_str_field(created, "id").to_string();
    let restore = RestoreCommand::new(&ctx, &["firewall", "groups", "delete", &id, "-y"]);

    assert_eq!(json_str_field(created, "group_type"), "port-group");
    assert!(
        !json_str_field(created, "external_id").is_empty(),
        "expected the controller to assign an external_id UUID, got {created}"
    );
    assert_eq!(
        created.get("group_members"),
        Some(&serde_json::json!(["80", "443"])),
        "expected the created group to carry its members"
    );

    let update = ctx.run(&[
        "firewall",
        "groups",
        "update",
        &id,
        "--members",
        "8080,8443,9000",
    ]);
    assert_success(&update, "firewall groups update");

    let get = ctx.run(&["firewall", "groups", "get", &id, "-o", "json"]);
    assert_success(&get, "firewall groups get after update");
    let detail = stdout_json(&get);
    assert_eq!(json_str_field(&detail, "name"), name);
    assert_eq!(
        detail.get("group_members"),
        Some(&serde_json::json!(["8080", "8443", "9000"])),
        "expected update to replace the member list, got {detail}"
    );

    let delete = ctx.run(&["firewall", "groups", "delete", &id, "-y"]);
    assert_success(&delete, "firewall groups delete");

    let after = list_firewall_groups(&ctx);
    assert!(
        find_firewall_group(&after, name).is_none(),
        "expected group '{name}' to be gone after delete, got {after:?}"
    );

    drop(restore);
}

// NAT create/update/delete are session-backed and the simulated controller
// accepts them even though its only gateway sits in PendingAdoption, so
// this exercises the full lifecycle rather than a rejection contract.
// `nat policies list` cannot observe the result in session auth mode: the
// session-only refresh path (`refresh_session_snapshot` in
// `unifly-api/src/controller/refresh.rs`) fills the NAT collection with an
// empty vec, so persistence is asserted through the raw v2 endpoint via
// `unifly api`. Create/update/delete resolve against the live endpoint and
// are unaffected.
#[test]
fn session_profile_nat_policy_lifecycle_persists_via_raw_api() {
    let ctx = E2eContext::session();
    let name = "e2e-nat-lifecycle";

    // Remove a stale rule from a failed earlier run against a still-running
    // container so the create below starts clean.
    for rule in &list_nat_rules_raw(&ctx) {
        if rule.get("description").and_then(serde_json::Value::as_str) == Some(name) {
            let stale_id = json_str_field(rule, "_id").to_string();
            let _ = ctx.run(&["nat", "policies", "delete", &stale_id, "-y"]);
        }
    }

    let create = ctx.run(&[
        "nat",
        "policies",
        "create",
        "--name",
        name,
        "--type",
        "masquerade",
        "--protocol",
        "all",
        "--src-address",
        "203.0.113.0/24",
    ]);
    assert_success(&create, "nat policies create");
    assert!(
        create.stdout.is_empty(),
        "expected create to print nothing on stdout (confirmation goes to stderr), got {}",
        stdout_text(&create)
    );
    assert!(
        stderr_text(&create).contains("NAT policy created"),
        "expected create confirmation on stderr, got:\n{}",
        stderr_text(&create)
    );

    let rules = list_nat_rules_raw(&ctx);
    let created = rules
        .iter()
        .find(|rule| rule.get("description").and_then(serde_json::Value::as_str) == Some(name))
        .unwrap_or_else(|| panic!("expected created NAT rule '{name}' on the wire, got {rules:?}"));
    assert_eq!(json_str_field(created, "type"), "MASQUERADE");
    assert!(json_bool(created, "enabled"));
    let id = json_str_field(created, "_id").to_string();
    let restore = RestoreCommand::new(&ctx, &["nat", "policies", "delete", &id, "-y"]);

    let update = ctx.run(&["nat", "policies", "update", &id, "--enabled", "false"]);
    assert_success(&update, "nat policies update");

    let after_update = list_nat_rules_raw(&ctx);
    let updated = after_update
        .iter()
        .find(|rule| rule.get("_id").and_then(serde_json::Value::as_str) == Some(id.as_str()))
        .unwrap_or_else(|| {
            panic!("expected NAT rule {id} to survive the update, got {after_update:?}")
        });
    assert!(
        !json_bool(updated, "enabled"),
        "expected update to disable the rule, got {updated}"
    );

    let delete = ctx.run(&["nat", "policies", "delete", &id, "-y"]);
    assert_success(&delete, "nat policies delete");

    let after_delete = list_nat_rules_raw(&ctx);
    assert!(
        after_delete
            .iter()
            .all(|rule| rule.get("_id").and_then(serde_json::Value::as_str) != Some(id.as_str())),
        "expected NAT rule {id} to be gone after delete, got {after_delete:?}"
    );

    drop(restore);
}

// The simulated switches stay in PendingAdoption and report no port_table,
// so the port read path yields well-formed empty collections rather than
// rows. The shape assertions below hold for both the observed empty state
// and an adopted switch with real port rows.
#[test]
fn session_profile_switch_ports_read_path_is_well_formed() {
    let ctx = E2eContext::session();

    let list_output = ctx.run(&["devices", "list", "--all", "-o", "json"]);
    assert_success(&list_output, "devices list for switch lookup");
    let list_payload = stdout_json(&list_output);
    let devices = json_array(&list_payload, "devices list");
    let switch = devices
        .iter()
        .find(|device| {
            device
                .get("device_type")
                .and_then(serde_json::Value::as_str)
                == Some("Switch")
        })
        .expect("expected simulation mode to expose at least one switch");
    let mac = json_str_field(switch, "mac").to_string();

    let ports = ctx.run(&["devices", "ports", &mac, "-o", "json"]);
    assert_success(&ports, "devices ports");
    let ports_payload = stdout_json(&ports);
    let port_rows = json_array(&ports_payload, "devices ports");
    assert!(
        port_rows.iter().all(|row| {
            row.get("index")
                .and_then(serde_json::Value::as_u64)
                .is_some()
        }),
        "expected every port row to carry a numeric index, got {}",
        stdout_text(&ports)
    );

    let export = ctx.run(&["devices", "ports-export", &mac, "--all"]);
    assert_success(&export, "devices ports-export");
    let stripped = strip_jsonc_comments(&stdout_text(&export));
    let export_payload: serde_json::Value = serde_json::from_str(&stripped).unwrap_or_else(|err| {
        panic!(
            "expected ports-export to parse as JSON after comment stripping ({err}), got:\n{}",
            stdout_text(&export)
        )
    });
    let exported_ports = export_payload
        .get("ports")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("expected a top-level ports array, got {export_payload}"));
    assert!(
        exported_ports.iter().all(|entry| {
            entry
                .get("index")
                .and_then(serde_json::Value::as_u64)
                .is_some()
        }),
        "expected every exported port entry to carry a numeric index, got {}",
        stdout_text(&export)
    );
}

#[test]
fn session_profile_integration_commands_surface_unsupported() {
    let ctx = E2eContext::session();
    let output = ctx.run(&["networks", "list", "--all", "-o", "json"]);

    assert_eq!(
        output.status.code(),
        Some(5),
        "stderr:\n{}",
        stderr_text(&output)
    );
    assert!(
        stderr_text(&output).contains("Integration API"),
        "expected integration guidance, got:\n{}",
        stderr_text(&output)
    );
}
