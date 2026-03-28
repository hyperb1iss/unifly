//! Config subcommand handlers.

use std::collections::HashMap;

use dialoguer::{Input, Select};

use crate::cli::args::{ConfigArgs, ConfigCommand, GlobalOpts};
use crate::cli::error::CliError;
use crate::cli::output;
use crate::config::resolve;
use crate::config::{Config, Defaults, Profile};

// ── Helpers ─────────────────────────────────────────────────────────

/// Format config for display, masking sensitive fields.
fn format_config_redacted(cfg: &Config) -> String {
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
        let p = &cfg.profiles[name];
        let _ = writeln!(out);
        let _ = writeln!(out, "[profiles.{name}]");
        let _ = writeln!(out, "controller = \"{}\"", p.controller);
        let _ = writeln!(out, "site = \"{}\"", p.site);
        let _ = writeln!(out, "auth_mode = \"{}\"", p.auth_mode);
        if p.api_key.is_some() {
            let _ = writeln!(out, "api_key = \"****\"");
        }
        if let Some(ref env) = p.api_key_env {
            let _ = writeln!(out, "api_key_env = \"{env}\"");
        }
        if let Some(ref u) = p.username {
            let _ = writeln!(out, "username = \"{u}\"");
        }
        if p.password.is_some() {
            let _ = writeln!(out, "password = \"****\"");
        }
        if let Some(ref ca) = p.ca_cert {
            let _ = writeln!(out, "ca_cert = \"{}\"", ca.display());
        }
        if let Some(insecure) = p.insecure {
            let _ = writeln!(out, "insecure = {insecure}");
        }
        if let Some(timeout) = p.timeout {
            let _ = writeln!(out, "timeout = {timeout}");
        }
    }

    out
}

/// Delegate to the shared config crate's save function.
fn save_config(cfg: &Config) -> Result<(), CliError> {
    crate::config::save_config(cfg)?;
    Ok(())
}

/// Map a dialoguer / interactive I/O failure into CliError.
fn prompt_err(e: impl std::fmt::Display) -> CliError {
    CliError::Validation {
        field: "interactive".into(),
        reason: format!("prompt failed: {e}"),
    }
}

/// Prompt for username and password, validating neither is empty.
fn prompt_credentials() -> Result<(String, String), CliError> {
    let user: String = Input::new()
        .with_prompt("Username")
        .interact_text()
        .map_err(prompt_err)?;

    let pass = rpassword::prompt_password("Password: ").map_err(prompt_err)?;

    if user.is_empty() || pass.is_empty() {
        return Err(CliError::Validation {
            field: "credentials".into(),
            reason: "username and password cannot be empty".into(),
        });
    }

    Ok((user, pass))
}

/// Offer to store a secret in the system keyring or return it for plaintext config.
///
/// Returns `Some(secret)` if the user chose plaintext, `None` if stored in keyring.
fn prompt_keyring_storage(
    secret: &str,
    keyring_key: &str,
    prompt: &str,
    label: &str,
) -> Result<Option<String>, CliError> {
    let choices = &[
        "Store in system keyring (recommended)",
        "Save to config file (plaintext)",
    ];
    let selection = Select::new()
        .with_prompt(prompt)
        .items(choices)
        .default(0)
        .interact()
        .map_err(prompt_err)?;

    if selection == 0 {
        let entry =
            keyring::Entry::new("unifly", keyring_key).map_err(|e| CliError::Validation {
                field: "keyring".into(),
                reason: format!("failed to access keyring: {e}"),
            })?;
        entry
            .set_password(secret)
            .map_err(|e| CliError::Validation {
                field: "keyring".into(),
                reason: format!("failed to store {label} in keyring: {e}"),
            })?;
        eprintln!("   ✓ {label} stored in system keyring");
        Ok(None)
    } else {
        Ok(Some(secret.to_owned()))
    }
}

// ── Handler ─────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
pub fn handle(args: ConfigArgs, global: &GlobalOpts) -> Result<(), CliError> {
    match args.command {
        // ── Init: interactive wizard ────────────────────────────────
        ConfigCommand::Init => {
            let config_path = crate::config::config_path();
            eprintln!("✨ UniFi CLI — configuration wizard");
            eprintln!("   Config path: {}\n", config_path.display());

            // 1. Profile name
            let profile_name: String = Input::new()
                .with_prompt("Profile name")
                .default("default".into())
                .interact_text()
                .map_err(prompt_err)?;

            // 2. Controller URL
            let controller: String = Input::new()
                .with_prompt("Controller URL")
                .default("https://192.168.1.1".into())
                .interact_text()
                .map_err(prompt_err)?;

            // 3. Auth mode
            let auth_choices = &[
                "API Key (recommended)",
                "Username/Password",
                "Hybrid (API key + credentials for full access)",
            ];
            let auth_selection = Select::new()
                .with_prompt("Authentication method")
                .items(auth_choices)
                .default(0)
                .interact()
                .map_err(prompt_err)?;

            let (auth_mode, api_key, username, password) = match auth_selection {
                0 => {
                    // --- API Key flow ---
                    let key = rpassword::prompt_password("API key: ").map_err(prompt_err)?;

                    if key.is_empty() {
                        return Err(CliError::Validation {
                            field: "api_key".into(),
                            reason: "API key cannot be empty".into(),
                        });
                    }

                    let api_key_field = prompt_keyring_storage(
                        &key,
                        &format!("{profile_name}/api-key"),
                        "Where to store the API key?",
                        "API key",
                    )?;

                    ("integration".to_string(), api_key_field, None, None)
                }
                1 => {
                    // --- Username/Password flow ---
                    let (user, pass) = prompt_credentials()?;

                    let password_field = prompt_keyring_storage(
                        &pass,
                        &format!("{profile_name}/password"),
                        "Where to store the password?",
                        "Password",
                    )?;

                    ("legacy".to_string(), None, Some(user), password_field)
                }
                _ => {
                    // --- Hybrid flow: API key + credentials ---
                    eprintln!("\n   Hybrid mode uses an API key for the Integration API");
                    eprintln!(
                        "   and username/password for the Legacy API (stats, events, alarms).\n"
                    );

                    let key = rpassword::prompt_password("API key: ").map_err(prompt_err)?;

                    if key.is_empty() {
                        return Err(CliError::Validation {
                            field: "api_key".into(),
                            reason: "API key cannot be empty".into(),
                        });
                    }

                    let api_key_field = prompt_keyring_storage(
                        &key,
                        &format!("{profile_name}/api-key"),
                        "Where to store the API key?",
                        "API key",
                    )?;

                    let (user, pass) = prompt_credentials()?;

                    let password_field = prompt_keyring_storage(
                        &pass,
                        &format!("{profile_name}/password"),
                        "Where to store the password?",
                        "Password",
                    )?;

                    (
                        "hybrid".to_string(),
                        api_key_field,
                        Some(user),
                        password_field,
                    )
                }
            };

            // 4. Site name
            let site: String = Input::new()
                .with_prompt("Site name")
                .default("default".into())
                .interact_text()
                .map_err(prompt_err)?;

            // 5. Build profile and config
            let profile = Profile {
                controller,
                site,
                auth_mode,
                api_key,
                api_key_env: None,
                username,
                password,
                ca_cert: None,
                insecure: None,
                timeout: None,
                totp_env: None,
            };

            let mut profiles = HashMap::new();
            profiles.insert(profile_name.clone(), profile);

            let cfg = Config {
                default_profile: Some(profile_name.clone()),
                defaults: Defaults::default(),
                profiles,
            };

            // 6. Write config
            save_config(&cfg)?;

            eprintln!("\n✓ Configuration written to {}", config_path.display());
            eprintln!("  Active profile: {profile_name}");
            eprintln!("\n  Test it: unifly system info --insecure");

            Ok(())
        }

        // ── Show ────────────────────────────────────────────────────
        ConfigCommand::Show => {
            let cfg = crate::config::load_config_or_default();
            let out = output::render_single(&global.output, &cfg, format_config_redacted, |_| {
                "config".into()
            });
            output::print_output(&out, global.quiet);
            Ok(())
        }

        // ── Set <key> <value> ───────────────────────────────────────
        ConfigCommand::Set { key, value } => {
            let mut cfg = crate::config::load_config_or_default();
            let profile_name = resolve::active_profile_name(global, &cfg);

            let profile = cfg
                .profiles
                .entry(profile_name.clone())
                .or_insert_with(|| Profile {
                    controller: String::new(),
                    site: "default".into(),
                    auth_mode: "integration".into(),
                    api_key: None,
                    api_key_env: None,
                    username: None,
                    password: None,
                    ca_cert: None,
                    insecure: None,
                    timeout: None,
                    totp_env: None,
                });

            match key.as_str() {
                "controller" => profile.controller = value,
                "site" => profile.site = value,
                "auth_mode" | "auth-mode" => {
                    if !matches!(value.as_str(), "integration" | "legacy" | "hybrid") {
                        return Err(CliError::Validation {
                            field: "auth_mode".into(),
                            reason: "must be 'integration', 'legacy', or 'hybrid'".into(),
                        });
                    }
                    profile.auth_mode = value;
                }
                "api_key" | "api-key" => profile.api_key = Some(value),
                "api_key_env" | "api-key-env" => profile.api_key_env = Some(value),
                "username" => profile.username = Some(value),
                "insecure" => {
                    profile.insecure = Some(value.parse().map_err(|_| CliError::Validation {
                        field: "insecure".into(),
                        reason: "must be 'true' or 'false'".into(),
                    })?);
                }
                "timeout" => {
                    profile.timeout = Some(value.parse().map_err(|_| CliError::Validation {
                        field: "timeout".into(),
                        reason: "must be a number (seconds)".into(),
                    })?);
                }
                "ca_cert" | "ca-cert" => profile.ca_cert = Some(value.into()),
                other => {
                    return Err(CliError::Validation {
                        field: other.into(),
                        reason: format!(
                            "unknown config key '{other}'. Valid keys: controller, site, \
                             auth_mode, api_key, api_key_env, username, insecure, timeout, ca_cert"
                        ),
                    });
                }
            }

            save_config(&cfg)?;
            eprintln!("✓ Set {key} on profile '{profile_name}'");
            Ok(())
        }

        // ── Profiles ────────────────────────────────────────────────
        ConfigCommand::Profiles => {
            let cfg = crate::config::load_config_or_default();
            let default = cfg.default_profile.as_deref().unwrap_or("default");
            if cfg.profiles.is_empty() {
                eprintln!("No profiles configured. Run: unifly config init");
            } else {
                for name in cfg.profiles.keys() {
                    let marker = if name == default { " *" } else { "" };
                    println!("{name}{marker}");
                }
            }
            Ok(())
        }

        // ── Use <name> ─────────────────────────────────────────────
        ConfigCommand::Use { name } => {
            let mut cfg = crate::config::load_config_or_default();

            if !cfg.profiles.contains_key(&name) {
                let available: Vec<_> = cfg.profiles.keys().cloned().collect();
                return Err(CliError::ProfileNotFound {
                    name,
                    available: if available.is_empty() {
                        "(none)".into()
                    } else {
                        available.join(", ")
                    },
                });
            }

            cfg.default_profile = Some(name.clone());
            save_config(&cfg)?;
            eprintln!("✓ Default profile set to '{name}'");
            Ok(())
        }

        // ── SetPassword ─────────────────────────────────────────────
        ConfigCommand::SetPassword { profile } => {
            let cfg = crate::config::load_config_or_default();
            let profile_name =
                profile.unwrap_or_else(|| resolve::active_profile_name(global, &cfg));

            let prof = cfg.profiles.get(&profile_name).ok_or_else(|| {
                let available: Vec<_> = cfg.profiles.keys().cloned().collect();
                CliError::ProfileNotFound {
                    name: profile_name.clone(),
                    available: if available.is_empty() {
                        "(none)".into()
                    } else {
                        available.join(", ")
                    },
                }
            })?;

            let store_secret = |key: &str, label: &str| -> Result<(), CliError> {
                let secret = rpassword::prompt_password(label).map_err(prompt_err)?;
                if secret.is_empty() {
                    return Err(CliError::Validation {
                        field: "secret".into(),
                        reason: "value cannot be empty".into(),
                    });
                }
                let entry =
                    keyring::Entry::new("unifly", key).map_err(|e| CliError::Validation {
                        field: "keyring".into(),
                        reason: format!("failed to access keyring: {e}"),
                    })?;
                entry
                    .set_password(&secret)
                    .map_err(|e| CliError::Validation {
                        field: "keyring".into(),
                        reason: format!("failed to store secret in keyring: {e}"),
                    })?;
                Ok(())
            };

            match prof.auth_mode.as_str() {
                "hybrid" => {
                    // Hybrid needs both API key and password
                    store_secret(&format!("{profile_name}/api-key"), "API key: ")?;
                    store_secret(&format!("{profile_name}/password"), "Password: ")?;
                }
                "integration" => {
                    store_secret(&format!("{profile_name}/api-key"), "API key: ")?;
                }
                _ => {
                    store_secret(&format!("{profile_name}/password"), "Password: ")?;
                }
            }

            eprintln!("✓ Secret(s) stored in system keyring for profile '{profile_name}'");
            Ok(())
        }
    }
}
