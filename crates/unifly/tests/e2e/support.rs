use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Output;
use std::sync::OnceLock;

use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

use super::assert_success;

/// Env var holding a pre-provisioned Integration API key.
///
/// A standalone UniFi Network Application in simulation mode cannot mint
/// an Integration key over the API (key auth is backed by the UniFi OS
/// account subsystem, absent on the bare controller image), so the
/// Integration lane runs only when a maintainer supplies a key for a real
/// controller. See the e2e section in `CONTRIBUTING.md` for the full
/// provisioning findings.
pub(super) const E2E_API_KEY_ENV: &str = "UNIFLY_E2E_API_KEY";

pub(super) struct E2eConfig {
    controller_url: String,
    username: String,
    password: String,
    pub(super) site: String,
    api_key: Option<String>,
}

impl E2eConfig {
    fn from_env() -> Self {
        Self {
            controller_url: std::env::var("UNIFLY_E2E_URL")
                .unwrap_or_else(|_| "https://localhost:8443".into()),
            username: std::env::var("UNIFLY_E2E_USERNAME").unwrap_or_else(|_| "admin".into()),
            password: std::env::var("UNIFLY_E2E_PASSWORD").unwrap_or_else(|_| "admin".into()),
            site: std::env::var("UNIFLY_E2E_SITE").unwrap_or_else(|_| "default".into()),
            api_key: std::env::var(E2E_API_KEY_ENV)
                .ok()
                .filter(|key| !key.trim().is_empty()),
        }
    }

    fn with_password(&self, password: &str) -> Self {
        Self {
            controller_url: self.controller_url.clone(),
            username: self.username.clone(),
            password: password.into(),
            site: self.site.clone(),
            api_key: self.api_key.clone(),
        }
    }
}

pub(super) struct E2eContext {
    pub(super) config: E2eConfig,
    tempdir: TempDir,
    no_cache: bool,
}

impl E2eContext {
    pub(super) fn session() -> Self {
        let context = Self::with_config(E2eConfig::from_env(), "session");
        seed_session_cache(&context);
        context
    }

    pub(super) fn session_with_password(password: &str) -> Self {
        let config = E2eConfig::from_env().with_password(password);
        let mut context = Self::with_config(config, "session");
        context.no_cache = true;
        context
    }

    /// Build an Integration-auth context, or `None` when no key is set.
    ///
    /// Returns `None` (so the caller skips) unless `UNIFLY_E2E_API_KEY`
    /// carries a key. The key reaches the CLI through the profile's
    /// `api_key_env`, so it never lands on disk in the temp config.
    pub(super) fn integration() -> Option<Self> {
        let mut config = E2eConfig::from_env();
        config.api_key.as_ref()?;
        if let Ok(site) = std::env::var("UNIFLY_E2E_INTEGRATION_SITE") {
            config.site = site;
        }
        Some(Self::with_config(config, "integration"))
    }

    fn with_config(config: E2eConfig, auth_mode: &str) -> Self {
        let tempdir = tempfile::tempdir().expect("isolated E2E config directory");
        write_profile(tempdir.path(), &config, auth_mode);
        Self {
            config,
            tempdir,
            no_cache: false,
        }
    }

    pub(super) fn cmd(&self) -> assert_cmd::Command {
        let mut cmd = cargo_bin_cmd!("unifly");
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("UNIFI_")
                || key.to_string_lossy().starts_with("UNIFLY_")
            {
                cmd.env_remove(key);
            }
        }
        if let Some(key) = &self.config.api_key {
            cmd.env(E2E_API_KEY_ENV, key);
        }
        cmd.env("HOME", self.tempdir.path())
            .env("XDG_CONFIG_HOME", self.tempdir.path())
            .env("XDG_CACHE_HOME", self.tempdir.path().join("cache"))
            .env("APPDATA", self.tempdir.path())
            .env("LOCALAPPDATA", self.tempdir.path())
            .env("USERPROFILE", self.tempdir.path())
            .env("UNIFI_USERNAME", &self.config.username)
            .env("UNIFI_PASSWORD", &self.config.password)
            .env("UNIFI_TIMEOUT", "60");
        if self.no_cache {
            cmd.arg("--no-cache");
        }
        cmd
    }

    pub(super) fn run(&self, args: &[&str]) -> Output {
        self.cmd().args(args).output().expect("execute E2E command")
    }

    pub(super) fn modern_stats(&self) -> bool {
        let output = self.run(&["api", "v2/api/stats/source", "-o", "json"]);
        if !output.status.success() {
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("HTTP 404"),
                "statistics source discovery failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            return false;
        }
        let source: String = serde_json::from_slice(&output.stdout).expect("raw backend string");
        match source.as_str() {
            "MODERN" => true,
            "LEGACY" => false,
            other => panic!("unknown statistics backend: {other}"),
        }
    }
}

fn write_profile(root: &Path, config: &E2eConfig, auth_mode: &str) {
    let config_dir = root.join("unifly");
    fs::create_dir_all(&config_dir).expect("create E2E config directory");

    // Integration auth pulls the key from an env var rather than config so
    // the secret never touches disk; session auth reads UNIFI_USERNAME /
    // UNIFI_PASSWORD from the child env (see `cmd`).
    let api_key_line = if auth_mode == "integration" {
        format!("api_key_env = \"{E2E_API_KEY_ENV}\"\n")
    } else {
        String::new()
    };

    let body = format!(
        r#"
default_profile = "default"

[profiles.default]
controller = "{controller}"
site = "{site}"
auth_mode = "{auth_mode}"
insecure = true
{api_key_line}"#,
        controller = config.controller_url,
        site = config.site,
        auth_mode = auth_mode,
    );

    fs::write(config_dir.join("config.toml"), body.trim_start()).expect("write E2E profile");
}

/// Bootstrap one login, then isolate each test's mutable cache copy.
fn seed_session_cache(context: &E2eContext) {
    static SEED: OnceLock<Vec<(OsString, Vec<u8>)>> = OnceLock::new();
    let seed = SEED.get_or_init(|| {
        let bootstrap = E2eContext::with_config(E2eConfig::from_env(), "session");
        let output = bootstrap.run(&["system", "sysinfo", "-o", "json"]);
        assert_success(&output, "bootstrap E2E session");
        let directory = bootstrap.tempdir.path().join("cache/unifly");
        let files = fs::read_dir(directory)
            .expect("bootstrap creates a session cache")
            .map(|entry| {
                let entry = entry.expect("read bootstrap cache entry");
                (
                    entry.file_name(),
                    fs::read(entry.path()).expect("read cached session bytes"),
                )
            })
            .collect::<Vec<_>>();
        assert!(
            !files.is_empty(),
            "bootstrap must cache its authenticated session"
        );
        files
    });
    let directory = context.tempdir.path().join("cache/unifly");
    fs::create_dir_all(&directory).expect("create isolated session cache");
    for (name, bytes) in seed {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options
            .open(directory.join(name))
            .expect("create private session cache")
            .write_all(bytes)
            .expect("write isolated session cache");
    }
}
