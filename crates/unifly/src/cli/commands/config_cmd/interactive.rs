use std::collections::HashMap;
use std::io::IsTerminal;
use std::time::Duration;

use dialoguer::{Confirm, Input, Select};
use opaline::adapters::owo_colors::OwoThemeExt;
use owo_colors::OwoColorize;
use secrecy::{ExposeSecret, SecretString};
use unifly_api::integration_types::SiteResponse;
use unifly_api::site_manager_types::Host;
use unifly_api::{
    ControllerPlatform, CoreError, IntegrationClient, SiteManagerClient, TlsMode, TransportConfig,
};

use crate::cli::args::{ColorMode, GlobalOpts};
use crate::cli::error::CliError;
use crate::cli::output;
use crate::config::{Config, Defaults, Profile};

use super::support::save_config;

enum ApiKeyStorage {
    Env(String),
    Keyring,
    Plaintext,
}

struct CloudApiKeySetup {
    secret: SecretString,
    storage: ApiKeyStorage,
}

struct WizardUi {
    theme: opaline::Theme,
    enabled: bool,
}

impl WizardUi {
    fn new(global: &GlobalOpts) -> Self {
        let enabled = match global.color {
            ColorMode::Always => true,
            ColorMode::Never => false,
            ColorMode::Auto => {
                std::io::stderr().is_terminal() && std::env::var("NO_COLOR").is_err()
            }
        };

        Self {
            theme: output::load_theme(None),
            enabled,
        }
    }

    fn paint(&self, text: &str, token: &str) -> String {
        if self.enabled {
            output::themed(&self.theme, text, token)
        } else {
            text.to_owned()
        }
    }

    fn keyword(&self, text: &str) -> String {
        if self.enabled {
            format!("{}", text.style(self.theme.owo_style("keyword")))
        } else {
            text.to_owned()
        }
    }

    fn accent(&self, text: &str) -> String {
        self.paint(text, "accent.secondary")
    }

    fn muted(&self, text: &str) -> String {
        self.paint(text, "text.muted")
    }

    fn detail_id(&self, text: &str) -> String {
        self.paint(text, "text.dim")
    }

    fn rule(&self) -> String {
        self.muted(&"─".repeat(58))
    }

    fn banner(&self, title: &str, subtitle: &str) {
        eprintln!("{}", self.rule());
        eprintln!("{} {}", self.accent("◆"), self.keyword(title));
        eprintln!("  {}", self.muted(subtitle));
        eprintln!("{}", self.rule());
    }

    fn status(&self, icon: &str, token: &str, message: &str) {
        eprintln!("  {} {}", self.paint(icon, token), message);
    }

    fn step(&self, message: &str) {
        self.status("↻", "accent.secondary", message);
    }

    fn success(&self, message: &str) {
        self.status("✓", "success", message);
    }

    fn warning(&self, message: &str) {
        self.status("!", "warning", message);
    }

    fn meta(&self, label: &str, value: &str) {
        let label = format!("{label:>12}:");
        eprintln!("  {} {}", self.muted(&label), value);
    }
}

fn prompt_err(error: impl std::fmt::Display) -> CliError {
    CliError::Validation {
        field: "interactive".into(),
        reason: format!("prompt failed: {error}"),
    }
}

fn cloud_api_error(error: unifly_api::Error) -> CliError {
    CliError::from(CoreError::from(error))
}

fn write_keyring_secret(secret: &str, keyring_key: &str, label: &str) -> Result<(), CliError> {
    let entry =
        keyring::Entry::new("unifly", keyring_key).map_err(|error| CliError::Validation {
            field: "keyring".into(),
            reason: format!("failed to access keyring: {error}"),
        })?;
    entry
        .set_password(secret)
        .map_err(|error| CliError::Validation {
            field: "keyring".into(),
            reason: format!("failed to store {label} in keyring: {error}"),
        })?;
    Ok(())
}

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

fn prompt_keyring_storage(
    ui: &WizardUi,
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
        write_keyring_secret(secret, keyring_key, label)?;
        ui.success(&format!("{label} stored in system keyring"));
        Ok(None)
    } else {
        Ok(Some(secret.to_owned()))
    }
}

fn store_secret(key: &str, label: &str) -> Result<(), CliError> {
    let secret = rpassword::prompt_password(label).map_err(prompt_err)?;
    if secret.is_empty() {
        return Err(CliError::Validation {
            field: "secret".into(),
            reason: "value cannot be empty".into(),
        });
    }
    write_keyring_secret(&secret, key, "secret")
}

fn build_transport(timeout_secs: u64) -> TransportConfig {
    TransportConfig {
        tls: TlsMode::System,
        timeout: Duration::from_secs(timeout_secs),
        cookie_jar: None,
    }
}

fn build_site_manager_client(
    controller: &str,
    api_key: &SecretString,
    timeout_secs: u64,
) -> Result<SiteManagerClient, CliError> {
    let transport = build_transport(timeout_secs);
    SiteManagerClient::from_api_key(controller, api_key, &transport).map_err(cloud_api_error)
}

fn cloud_connector_url(controller: &str, host_id: &str) -> String {
    format!(
        "{}/v1/connector/consoles/{host_id}",
        controller.trim_end_matches('/')
    )
}

fn build_cloud_integration_client(
    controller: &str,
    host_id: &str,
    api_key: &SecretString,
    timeout_secs: u64,
) -> Result<IntegrationClient, CliError> {
    let transport = build_transport(timeout_secs);
    IntegrationClient::from_api_key(
        &cloud_connector_url(controller, host_id),
        api_key,
        &transport,
        ControllerPlatform::Cloud,
    )
    .map_err(cloud_api_error)
}

/// Guess whether a controller URL points at a LAN device that will present
/// a self-signed certificate: bare IPs, single-label hostnames, and
/// local-only suffixes default the wizard's TLS prompt to "yes", public
/// hostnames to "no". Unparseable input is treated as local.
fn likely_self_signed(controller: &str) -> bool {
    let Ok(url) = url::Url::parse(controller) else {
        return true;
    };

    match url.host() {
        Some(url::Host::Ipv4(_) | url::Host::Ipv6(_)) | None => true,
        Some(url::Host::Domain(domain)) => {
            let domain = domain.trim_end_matches('.').to_ascii_lowercase();
            !domain.contains('.')
                || [".local", ".lan", ".home", ".internal", ".home.arpa"]
                    .iter()
                    .any(|suffix| domain.ends_with(suffix))
        }
    }
}

fn slugify_profile_name(name: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;

    for ch in name.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_dash = false;
        } else if !slug.is_empty() && !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }

    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "cloud".into()
    } else {
        trimmed.into()
    }
}

fn next_available_profile_name(base: &str, profiles: &HashMap<String, Profile>) -> String {
    let base = slugify_profile_name(base);
    if !profiles.contains_key(&base) {
        return base;
    }

    for index in 2.. {
        let candidate = format!("{base}-{index}");
        if !profiles.contains_key(&candidate) {
            return candidate;
        }
    }

    unreachable!("integer sequence is unbounded")
}

fn host_choice_label(host: &Host) -> String {
    let mut details = Vec::new();
    let model = host.model_name();
    let status = host.status();

    if !model.is_empty() {
        details.push(model);
    }
    if status != "unknown" {
        details.push(status);
    }
    if host.is_owner_host() {
        details.push("owner".into());
    }

    if details.is_empty() {
        host.display_name()
    } else {
        format!("{} [{}]", host.display_name(), details.join(" • "))
    }
}

fn site_choice_label(site: &SiteResponse) -> String {
    if site.name == site.internal_reference {
        site.name.clone()
    } else {
        format!("{} ({})", site.name, site.internal_reference)
    }
}

fn prompt_cloud_api_key(_ui: &WizardUi) -> Result<CloudApiKeySetup, CliError> {
    if let Ok(api_key) = std::env::var("UNIFI_API_KEY")
        && !api_key.trim().is_empty()
    {
        let selection = Select::new()
            .with_prompt("How should unifly get your Site Manager API key?")
            .items([
                "Use current UNIFI_API_KEY environment variable (recommended)",
                "Paste a different API key",
            ])
            .default(0)
            .interact()
            .map_err(prompt_err)?;

        if selection == 0 {
            return Ok(CloudApiKeySetup {
                secret: SecretString::from(api_key),
                storage: ApiKeyStorage::Env("UNIFI_API_KEY".into()),
            });
        }
    }

    let api_key = rpassword::prompt_password("Site Manager API key: ").map_err(prompt_err)?;
    if api_key.trim().is_empty() {
        return Err(CliError::Validation {
            field: "api_key".into(),
            reason: "API key cannot be empty".into(),
        });
    }

    let storage = match Select::new()
        .with_prompt("Where should unifly store the API key?")
        .items([
            "Store in system keyring (recommended)",
            "Save to config file (plaintext)",
        ])
        .default(0)
        .interact()
        .map_err(prompt_err)?
    {
        0 => ApiKeyStorage::Keyring,
        _ => ApiKeyStorage::Plaintext,
    };

    Ok(CloudApiKeySetup {
        secret: SecretString::from(api_key),
        storage,
    })
}

fn select_host<'a>(ui: &WizardUi, hosts: &'a [Host]) -> Result<&'a Host, CliError> {
    match hosts {
        [] => Err(CliError::Validation {
            field: "api_key".into(),
            reason: "no cloud consoles are accessible with this API key".into(),
        }),
        [host] => {
            ui.success(&format!(
                "Using console {}",
                ui.accent(&host.display_name())
            ));
            Ok(host)
        }
        _ => {
            let choices = hosts.iter().map(host_choice_label).collect::<Vec<_>>();
            let default = hosts.iter().position(Host::is_owner_host).unwrap_or(0);
            let selection = Select::new()
                .with_prompt("Which console should this profile use?")
                .items(&choices)
                .default(default)
                .interact()
                .map_err(prompt_err)?;
            Ok(&hosts[selection])
        }
    }
}

fn select_site_internal_reference(
    ui: &WizardUi,
    sites: &[SiteResponse],
) -> Result<String, CliError> {
    match sites {
        [] => {
            ui.warning("No sites were returned by the connector; falling back to default");
            Ok("default".into())
        }
        [site] => {
            ui.success(&format!(
                "Using site {}",
                ui.accent(&site_choice_label(site))
            ));
            Ok(site.internal_reference.clone())
        }
        _ => {
            let choices = sites.iter().map(site_choice_label).collect::<Vec<_>>();
            let default = sites
                .iter()
                .position(|site| site.internal_reference == "default")
                .unwrap_or(0);
            let selection = Select::new()
                .with_prompt("Which site should this profile target by default?")
                .items(&choices)
                .default(default)
                .interact()
                .map_err(prompt_err)?;
            Ok(sites[selection].internal_reference.clone())
        }
    }
}

fn prompt_profile_name(
    ui: &WizardUi,
    default_name: &str,
    profiles: &HashMap<String, Profile>,
) -> Result<String, CliError> {
    let mut suggested = default_name.to_owned();

    loop {
        let entered: String = Input::new()
            .with_prompt("Profile name")
            .default(suggested.clone())
            .interact_text()
            .map_err(prompt_err)?;
        let entered = entered.trim();

        if entered.is_empty() {
            ui.warning("Profile name cannot be empty");
            continue;
        }

        if !profiles.contains_key(entered) {
            return Ok(entered.into());
        }

        let overwrite = Confirm::new()
            .with_prompt(format!("Profile '{entered}' already exists. Replace it?"))
            .default(false)
            .interact()
            .map_err(prompt_err)?;

        if overwrite {
            return Ok(entered.into());
        }

        suggested = next_available_profile_name(entered, profiles);
        ui.warning(&format!(
            "Keeping the existing profile. Try {} instead.",
            ui.accent(&suggested)
        ));
    }
}

fn persist_cloud_api_key(
    ui: &WizardUi,
    profile_name: &str,
    setup: &CloudApiKeySetup,
) -> Result<(Option<String>, Option<String>), CliError> {
    match &setup.storage {
        ApiKeyStorage::Env(env_name) => Ok((None, Some(env_name.clone()))),
        ApiKeyStorage::Keyring => {
            write_keyring_secret(
                setup.secret.expose_secret(),
                &format!("{profile_name}/api-key"),
                "API key",
            )?;
            ui.success("API key stored in system keyring");
            Ok((None, None))
        }
        ApiKeyStorage::Plaintext => Ok((Some(setup.secret.expose_secret().to_owned()), None)),
    }
}

#[allow(clippy::too_many_lines)]
pub(super) fn run_init(global: &GlobalOpts) -> Result<(), CliError> {
    let ui = WizardUi::new(global);
    let config_path = crate::config::config_path();
    ui.banner(
        "UniFi CLI • local setup",
        "Create a direct controller profile for Integration, Session, or Hybrid auth.",
    );
    ui.meta(
        "Config path",
        &ui.accent(&config_path.display().to_string()),
    );
    eprintln!();

    let profile_name: String = Input::new()
        .with_prompt("Profile name")
        .default("default".into())
        .interact_text()
        .map_err(prompt_err)?;

    let controller: String = Input::new()
        .with_prompt("Controller URL")
        .default("https://192.168.1.1".into())
        .interact_text()
        .map_err(prompt_err)?;

    // UDMs/UCGs ship self-signed certs; a profile without `insecure` fails
    // TLS in every surface that doesn't pass -k (issue #25 root cause).
    let insecure = Confirm::new()
        .with_prompt("Does the controller use a self-signed certificate? (skips TLS verification)")
        .default(likely_self_signed(&controller))
        .interact()
        .map_err(prompt_err)?;

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
            let key = rpassword::prompt_password("API key: ").map_err(prompt_err)?;

            if key.is_empty() {
                return Err(CliError::Validation {
                    field: "api_key".into(),
                    reason: "API key cannot be empty".into(),
                });
            }

            let api_key_field = prompt_keyring_storage(
                &ui,
                &key,
                &format!("{profile_name}/api-key"),
                "Where to store the API key?",
                "API key",
            )?;

            ("integration".to_string(), api_key_field, None, None)
        }
        1 => {
            let (user, pass) = prompt_credentials()?;

            let password_field = prompt_keyring_storage(
                &ui,
                &pass,
                &format!("{profile_name}/password"),
                "Where to store the password?",
                "Password",
            )?;

            ("session".to_string(), None, Some(user), password_field)
        }
        _ => {
            eprintln!();
            ui.step("Hybrid mode uses the API key for Integration + Session HTTP.");
            eprintln!(
                "  {}",
                ui.muted("Username/password is kept for live WebSocket events.")
            );
            eprintln!();

            let key = rpassword::prompt_password("API key: ").map_err(prompt_err)?;

            if key.is_empty() {
                return Err(CliError::Validation {
                    field: "api_key".into(),
                    reason: "API key cannot be empty".into(),
                });
            }

            let api_key_field = prompt_keyring_storage(
                &ui,
                &key,
                &format!("{profile_name}/api-key"),
                "Where to store the API key?",
                "API key",
            )?;

            let (user, pass) = prompt_credentials()?;

            let password_field = prompt_keyring_storage(
                &ui,
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

    let site: String = Input::new()
        .with_prompt("Site name")
        .default("default".into())
        .interact_text()
        .map_err(prompt_err)?;

    let controller_display = controller.clone();
    let site_display = site.clone();

    let profile = Profile {
        controller,
        site,
        auth_mode,
        api_key,
        api_key_env: None,
        host_id: None,
        host_id_env: None,
        username,
        password,
        totp_env: None,
        ca_cert: None,
        insecure: insecure.then_some(true),
        timeout: None,
    };

    let mut profiles = HashMap::new();
    profiles.insert(profile_name.clone(), profile);

    let cfg = Config {
        default_profile: Some(profile_name.clone()),
        defaults: Defaults::default(),
        profiles,
        demo: crate::config::DemoConfig::default(),
    };

    save_config(&cfg)?;

    eprintln!();
    ui.success("Configuration written");
    ui.meta("Profile", &ui.accent(&profile_name));
    ui.meta("Controller", &ui.accent(&controller_display));
    ui.meta("Site", &ui.accent(&site_display));
    if insecure {
        ui.meta("TLS", &ui.accent("accept self-signed (insecure = true)"));
    }
    ui.meta(
        "Config path",
        &ui.accent(&config_path.display().to_string()),
    );
    eprintln!();
    ui.meta("Try", &ui.keyword("unifly system info"));

    Ok(())
}

#[allow(clippy::too_many_lines)]
pub(super) async fn run_cloud_setup(global: &GlobalOpts) -> Result<(), CliError> {
    let ui = WizardUi::new(global);
    let config_path = crate::config::config_path();
    let controller = global
        .controller
        .clone()
        .unwrap_or_else(|| crate::config::DEFAULT_CLOUD_CONTROLLER_URL.into());

    ui.banner(
        "UniFi CLI • cloud setup",
        "Validate a Site Manager API key, pick a console, and write a ready-to-use cloud profile.",
    );
    ui.meta(
        "Config path",
        &ui.accent(&config_path.display().to_string()),
    );
    ui.meta("API base", &ui.accent(&controller));
    eprintln!();

    let api_key_setup = prompt_cloud_api_key(&ui)?;
    let site_manager = build_site_manager_client(
        &controller,
        &api_key_setup.secret,
        global.timeout_secs(None, None),
    )?;

    ui.step("Checking which consoles this API key can see...");
    let hosts = site_manager.list_hosts().await.map_err(cloud_api_error)?;
    let host = select_host(&ui, &hosts)?;

    ui.step(&format!(
        "Reading sites from {}...",
        ui.accent(&host.display_name())
    ));
    let integration = build_cloud_integration_client(
        &controller,
        &host.id,
        &api_key_setup.secret,
        global.timeout_secs(None, None),
    )?;
    let sites = integration
        .paginate_all(50, |offset, limit| integration.list_sites(offset, limit))
        .await
        .map_err(cloud_api_error)?;
    let site = select_site_internal_reference(&ui, &sites)?;

    let mut cfg = crate::config::load_config_or_default();
    let suggested_profile = next_available_profile_name(&host.display_name(), &cfg.profiles);
    let profile_name = prompt_profile_name(&ui, &suggested_profile, &cfg.profiles)?;
    let (api_key, api_key_env) = persist_cloud_api_key(&ui, &profile_name, &api_key_setup)?;

    let make_default = if cfg.default_profile.as_deref() == Some(profile_name.as_str()) {
        true
    } else {
        Confirm::new()
            .with_prompt("Make this your default profile?")
            .default(cfg.profiles.is_empty() || cfg.default_profile.is_none())
            .interact()
            .map_err(prompt_err)?
    };

    cfg.profiles.insert(
        profile_name.clone(),
        Profile {
            controller: controller.clone(),
            site: site.clone(),
            auth_mode: "cloud".into(),
            api_key,
            api_key_env,
            host_id: Some(host.id.clone()),
            host_id_env: None,
            username: None,
            password: None,
            totp_env: None,
            ca_cert: None,
            insecure: None,
            timeout: None,
        },
    );

    if make_default {
        cfg.default_profile = Some(profile_name.clone());
    }

    save_config(&cfg)?;

    eprintln!();
    ui.success("Cloud profile written");
    ui.meta("Profile", &ui.accent(&profile_name));
    ui.meta("Console", &ui.accent(&host.display_name()));
    ui.meta("Host ID", &ui.detail_id(&host.id));
    ui.meta("Site", &ui.accent(&site));
    ui.meta(
        "Config path",
        &ui.accent(&config_path.display().to_string()),
    );

    if let ApiKeyStorage::Env(env_name) = &api_key_setup.storage {
        eprintln!();
        ui.warning(&format!(
            "Keep {} exported when using this profile.",
            ui.keyword(env_name)
        ));
    }

    eprintln!();
    ui.meta(
        "Try",
        &ui.keyword(&format!("unifly -p {profile_name} devices list")),
    );
    ui.meta("Cloud", &ui.keyword("unifly cloud hosts"));
    ui.meta("TUI", &ui.keyword(&format!("unifly -p {profile_name} tui")));

    Ok(())
}

pub(super) fn store_profile_secrets(profile_name: &str, auth_mode: &str) -> Result<(), CliError> {
    match auth_mode {
        "hybrid" => {
            store_secret(&format!("{profile_name}/api-key"), "API key: ")?;
            store_secret(&format!("{profile_name}/password"), "Password: ")?;
        }
        "integration" | "cloud" => {
            store_secret(&format!("{profile_name}/api-key"), "API key: ")?;
        }
        _ => {
            store_secret(&format!("{profile_name}/password"), "Password: ")?;
        }
    }

    eprintln!("✓ Secret(s) stored in system keyring for profile '{profile_name}'");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        cloud_connector_url, likely_self_signed, next_available_profile_name, slugify_profile_name,
    };
    use crate::config::Profile;

    fn sample_profile() -> Profile {
        Profile {
            controller: String::new(),
            site: "default".into(),
            auth_mode: "cloud".into(),
            api_key: None,
            api_key_env: None,
            host_id: Some("host-1".into()),
            host_id_env: None,
            username: None,
            password: None,
            totp_env: None,
            ca_cert: None,
            insecure: None,
            timeout: None,
        }
    }

    #[test]
    fn slugify_profile_name_normalizes_console_names() {
        assert_eq!(slugify_profile_name("Work UDM-Pro Max"), "work-udm-pro-max");
        assert_eq!(slugify_profile_name("   "), "cloud");
    }

    #[test]
    fn next_available_profile_name_appends_numeric_suffix() {
        let mut profiles = HashMap::new();
        profiles.insert("work".into(), sample_profile());
        profiles.insert("work-2".into(), sample_profile());

        assert_eq!(next_available_profile_name("work", &profiles), "work-3");
    }

    #[test]
    fn likely_self_signed_defaults_yes_for_local_controllers() {
        assert!(likely_self_signed("https://192.168.1.1"));
        assert!(likely_self_signed("https://[fe80::1]:8443"));
        assert!(likely_self_signed("https://udm.local"));
        assert!(likely_self_signed("https://gateway.lan"));
        assert!(likely_self_signed("https://dream-machine"));
        assert!(likely_self_signed("192.168.1.1"));
    }

    #[test]
    fn likely_self_signed_defaults_no_for_public_hostnames() {
        assert!(!likely_self_signed("https://unifi.example.com"));
        assert!(!likely_self_signed("https://controller.example.com:8443"));
    }

    #[test]
    fn cloud_connector_url_points_at_selected_console() {
        assert_eq!(
            cloud_connector_url("https://api.ui.com", "host-123"),
            "https://api.ui.com/v1/connector/consoles/host-123"
        );
        assert_eq!(
            cloud_connector_url("https://api.ui.com/", "host-123"),
            "https://api.ui.com/v1/connector/consoles/host-123"
        );
    }
}
