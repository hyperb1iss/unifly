//! Config edits and auth overrides exercised through the real CLI binary.

use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

fn command(home: &Path) -> assert_cmd::Command {
    let mut command = cargo_bin_cmd!("unifly");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("UNIFI_")
            || key.to_string_lossy().starts_with("UNIFLY_")
        {
            command.env_remove(key);
        }
    }
    for key in [
        "HOME",
        "XDG_CONFIG_HOME",
        "APPDATA",
        "LOCALAPPDATA",
        "USERPROFILE",
    ] {
        command.env(key, home);
    }
    command
}

fn fixture(source: &str) -> (TempDir, PathBuf) {
    let home = tempfile::tempdir().expect("isolated config");
    let path = home.path().join("unifly/config.toml");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("config directory");
    std::fs::write(&path, source).expect("config fixture");
    (home, path)
}

#[test]
fn config_edits_preserve_comments_above_changed_keys() {
    let (home, path) = fixture(
        "# controller selection\n\n  default_profile = 'home' # active\n\
         future = { keep = true }\n[profiles.home]\ncontroller = 'https://home.invalid'\n\
         [profiles.work]\n# endpoint comment\n  controller = 'https://work.invalid' # target\nfuture_option = 7\n",
    );
    command(home.path())
        .args(["config", "use", "work"])
        .assert()
        .success();
    command(home.path())
        .args(["config", "set", "controller", "https://new.invalid"])
        .assert()
        .success();
    let saved = std::fs::read_to_string(path).expect("saved config");
    for expected in [
        "# controller selection\n\n  default_profile = \"work\" # active",
        "# endpoint comment\n  controller = \"https://new.invalid\" # target",
        "future = { keep = true }",
        "future_option = 7",
    ] {
        assert!(
            saved.contains(expected),
            "missing preserved text: {expected}"
        );
    }
}

#[test]
fn malformed_userinfo_is_hidden_in_config_output_and_errors() {
    for url in [
        "https:///admin:fixturesecret@host:99999",
        "///admin:fixturesecret@host:99999",
        "https:///fixturesecret@host:99999",
        "////fixturesecret@host:99999",
        "https://\\fixturesecret@host:99999",
    ] {
        let (home, _) = fixture(&format!(
            "[profiles.default]\ncontroller = '{url}'\napi_key = 'fixture-key'\n"
        ));
        let shown = command(home.path())
            .args(["config", "show", "-o", "json"])
            .output()
            .expect("config show");
        assert!(shown.status.success());
        let shown = String::from_utf8_lossy(&shown.stdout);
        assert!(!shown.contains("fixturesecret"));
        assert!(shown.contains("****@host:99999"));
        let failed = command(home.path())
            .args(["system", "info"])
            .output()
            .expect("invalid URL");
        assert!(!failed.status.success());
        let error: String = String::from_utf8_lossy(&failed.stderr)
            .chars()
            .filter(|character| !character.is_whitespace() && *character != '│')
            .collect();
        assert!(!error.contains("fixturesecret"));
        assert!(error.contains("****@host:99999"));
    }
}

#[tokio::test]
async fn explicit_direct_keys_never_post_saved_passwords_or_discover_cloud_hosts() {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/auth/login"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path("/proxy/network/integration/v1/sites"))
        .and(header("X-API-KEY", "one-shot-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "offset":0,"limit":50,"count":1,"totalCount":1,
            "data":[{"id":"550e8400-e29b-41d4-a716-446655440000","internalReference":"default","name":"Fixture"}]
        }))).mount(&server).await;
    Mock::given(method("GET"))
        .and(path("/proxy/network/integration/v1/info"))
        .and(header("X-API-KEY", "one-shot-key"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"applicationVersion":"11.0.99"})),
        )
        .expect(5)
        .mount(&server)
        .await;
    for (mode, override_url) in [
        ("session", true),
        ("legacy", true),
        ("hybrid", true),
        ("cloud", true),
        ("session", false),
    ] {
        let saved_url = if override_url {
            "https://saved.invalid".into()
        } else {
            server.uri()
        };
        let (home, _) = fixture(&format!(
            "[profiles.default]\ncontroller = '{saved_url}'\nauth_mode = '{mode}'\n\
             username = 'saved-user'\npassword = 'never-send-this-password'\napi_key = 'saved-key'\n"
        ));
        let mut cli = command(home.path());
        cli.args(["--api-key", "one-shot-key"]);
        if override_url {
            cli.args(["--controller", &server.uri()]);
        }
        cli.args(["system", "info", "-o", "json"])
            .assert()
            .success();
    }
    let requests = server.received_requests().await.expect("requests");
    assert!(
        requests
            .iter()
            .all(|request| request.method == "GET" && request.body.is_empty())
    );
    assert!(
        !requests
            .iter()
            .any(|request| request.url.path().contains("hosts"))
    );
    server.verify().await;
}
