use crate::cli::error::CliError;
use crate::config::{Config, Profile};

/// Shown in place of a secret stored in config.toml.
const MASKED: &str = "****";

/// Mask every secret stored in `cfg` so no output format can print one.
/// Masking keeps the field, so the output still says a secret is set.
pub(super) fn redact_secrets(cfg: &mut Config) {
    for profile in cfg.profiles.values_mut() {
        for secret in [&mut profile.api_key, &mut profile.password] {
            if secret.is_some() {
                *secret = Some(MASKED.into());
            }
        }
    }
}

pub(super) fn format_config_redacted(cfg: &Config) -> String {
    use std::fmt::Write;

    let mut out = String::new();

    if let Some(ref default) = cfg.default_profile {
        let _ = writeln!(out, "default_profile = \"{default}\"");
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "[defaults]");
    let _ = writeln!(out, "output = \"{}\"", cfg.defaults.output);
    let _ = writeln!(out, "color = \"{}\"", cfg.defaults.color);
    let _ = writeln!(out, "insecure = {}", cfg.defaults.insecure);
    let _ = writeln!(out, "timeout = {}", cfg.defaults.timeout);

    let mut names: Vec<_> = cfg.profiles.keys().collect();
    names.sort();
    for name in names {
        let profile = &cfg.profiles[name];
        let _ = writeln!(out);
        let _ = writeln!(out, "[profiles.{name}]");
        let _ = writeln!(out, "controller = \"{}\"", profile.controller);
        let _ = writeln!(out, "site = \"{}\"", profile.site);
        let _ = writeln!(out, "auth_mode = \"{}\"", profile.auth_mode);
        if profile.api_key.is_some() {
            let _ = writeln!(out, "api_key = \"{MASKED}\"");
        }
        if let Some(ref env) = profile.api_key_env {
            let _ = writeln!(out, "api_key_env = \"{env}\"");
        }
        if let Some(ref host_id) = profile.host_id {
            let _ = writeln!(out, "host_id = \"{host_id}\"");
        }
        if let Some(ref env) = profile.host_id_env {
            let _ = writeln!(out, "host_id_env = \"{env}\"");
        }
        if let Some(ref username) = profile.username {
            let _ = writeln!(out, "username = \"{username}\"");
        }
        if profile.password.is_some() {
            let _ = writeln!(out, "password = \"{MASKED}\"");
        }
        if let Some(ref ca_cert) = profile.ca_cert {
            let _ = writeln!(out, "ca_cert = \"{}\"", ca_cert.display());
        }
        if let Some(insecure) = profile.insecure {
            let _ = writeln!(out, "insecure = {insecure}");
        }
        if let Some(timeout) = profile.timeout {
            let _ = writeln!(out, "timeout = {timeout}");
        }
    }

    out
}

pub(super) fn save_config(cfg: &Config) -> Result<(), CliError> {
    crate::config::save_config(cfg)?;
    Ok(())
}

pub(super) fn empty_profile() -> Profile {
    Profile {
        controller: String::new(),
        site: "default".into(),
        auth_mode: "integration".into(),
        api_key: None,
        api_key_env: None,
        host_id: None,
        host_id_env: None,
        username: None,
        password: None,
        totp_env: None,
        totp_command: None,
        ca_cert: None,
        insecure: None,
        timeout: None,
    }
}

pub(super) fn resolve_set_target(
    key: &str,
    active_profile: &str,
) -> Result<(String, String), CliError> {
    if let Some(rest) = key.strip_prefix("profiles.") {
        let mut parts = rest.splitn(2, '.');
        let Some(profile_name) = parts.next() else {
            unreachable!("splitn always yields at least one part");
        };
        let field = parts.next().ok_or_else(|| CliError::Validation {
            field: "key".into(),
            reason: format!("expected profiles.<name>.<key>, got '{key}'"),
        })?;

        if profile_name.is_empty() || field.is_empty() {
            return Err(CliError::Validation {
                field: "key".into(),
                reason: format!("expected profiles.<name>.<key>, got '{key}'"),
            });
        }

        Ok((profile_name.to_owned(), field.to_owned()))
    } else {
        Ok((active_profile.to_owned(), key.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use super::{format_config_redacted, redact_secrets, resolve_set_target};
    use crate::config::{Config, Defaults, Profile};

    fn sample_profile() -> Profile {
        Profile {
            controller: "https://192.168.1.1".into(),
            site: "default".into(),
            auth_mode: "hybrid".into(),
            api_key: Some("super-secret-api-key".into()),
            api_key_env: Some("UNIFI_API_KEY".into()),
            host_id: Some("host-123".into()),
            host_id_env: Some("UNIFI_HOST_ID".into()),
            username: Some("bliss".into()),
            password: Some("super-secret-password".into()),
            totp_env: None,
            totp_command: None,
            ca_cert: Some(PathBuf::from("/tmp/unifly.pem")),
            insecure: Some(true),
            timeout: Some(45),
        }
    }

    #[test]
    fn resolve_set_target_supports_profile_dot_path() {
        let target = resolve_set_target("profiles.home.controller", "default")
            .expect("profile dot path should resolve");
        assert_eq!(target, ("home".into(), "controller".into()));
    }

    #[test]
    fn resolve_set_target_uses_active_profile_for_bare_key() {
        let target =
            resolve_set_target("controller", "home").expect("bare key should use active profile");
        assert_eq!(target, ("home".into(), "controller".into()));
    }

    #[test]
    fn resolve_set_target_rejects_incomplete_profile_path() {
        let error = resolve_set_target("profiles.home", "default")
            .expect_err("incomplete profile path should fail");
        let message = error.to_string();
        assert!(message.contains("profiles.<name>.<key>"));
        assert!(message.contains("profiles.home"));
    }

    #[test]
    fn redact_secrets_masks_stored_secrets_and_nothing_else() {
        let mut profiles = HashMap::new();
        profiles.insert("home".into(), sample_profile());
        let mut keyring_only = sample_profile();
        keyring_only.api_key = None;
        keyring_only.password = None;
        profiles.insert("work".into(), keyring_only);
        let mut cfg = Config {
            default_profile: Some("home".into()),
            defaults: Defaults::default(),
            profiles,
            demo: crate::config::DemoConfig::default(),
        };

        redact_secrets(&mut cfg);

        let home = &cfg.profiles["home"];
        assert_eq!(home.api_key.as_deref(), Some("****"));
        assert_eq!(home.password.as_deref(), Some("****"));
        assert_eq!(home.username.as_deref(), Some("bliss"));
        assert_eq!(home.api_key_env.as_deref(), Some("UNIFI_API_KEY"));
        let work = &cfg.profiles["work"];
        assert_eq!(work.api_key, None);
        assert_eq!(work.password, None);
    }

    #[test]
    fn format_config_redacted_masks_secrets() {
        let mut profiles = HashMap::new();
        profiles.insert("home".into(), sample_profile());

        let rendered = format_config_redacted(&Config {
            default_profile: Some("home".into()),
            defaults: Defaults::default(),
            profiles,
            demo: crate::config::DemoConfig::default(),
        });

        assert!(rendered.contains("default_profile = \"home\""));
        assert!(rendered.contains("[profiles.home]"));
        assert!(rendered.contains("api_key = \"****\""));
        assert!(rendered.contains("password = \"****\""));
        assert!(rendered.contains("api_key_env = \"UNIFI_API_KEY\""));
        assert!(rendered.contains("host_id = \"host-123\""));
        assert!(rendered.contains("host_id_env = \"UNIFI_HOST_ID\""));
        assert!(rendered.contains("username = \"bliss\""));
        assert!(!rendered.contains("super-secret-api-key"));
        assert!(!rendered.contains("super-secret-password"));
    }
}
