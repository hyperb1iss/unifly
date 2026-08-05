pub mod fleet_devices;
pub mod fleet_sites;
pub mod hosts;
pub mod isp;
pub mod sdwan;
pub mod switch;

use std::time::Duration;

use secrecy::SecretString;
use unifly_api::integration_types::SiteResponse;
use unifly_api::site_manager_types::Host;
use unifly_api::{
    ControllerPlatform, CoreError, IntegrationClient, SiteManagerClient, TlsMode, TransportConfig,
};

use crate::cli::args::{CloudArgs, CloudCommand, GlobalOpts};
use crate::cli::error::CliError;
use crate::config::{self, Defaults, Profile};

pub async fn handle(args: CloudArgs, global: &GlobalOpts) -> Result<(), CliError> {
    match args.command {
        CloudCommand::Switch(args) => switch::handle(args, global).await,
        command => {
            let client = build_site_manager_client(global)?;
            match command {
                CloudCommand::Hosts(args) => hosts::handle(&client, args, global).await,
                CloudCommand::Sites(args) => fleet_sites::handle(&client, args, global).await,
                CloudCommand::Devices(args) => fleet_devices::handle(&client, args, global).await,
                CloudCommand::Isp(args) => isp::handle(&client, args, global).await,
                CloudCommand::Sdwan(args) => sdwan::handle(&client, args, global).await,
                CloudCommand::Switch(_) => unreachable!("switch is handled above"),
            }
        }
    }
}

pub(crate) fn build_site_manager_client(
    global: &GlobalOpts,
) -> Result<SiteManagerClient, CliError> {
    let (profile_name, profile, defaults) = active_profile(global);

    let api_key = resolve_cloud_api_key(profile.as_ref(), &profile_name, global)?;
    let controller = resolve_site_manager_url(profile.as_ref(), global);
    let transport = cloud_transport(
        global,
        profile.as_ref().and_then(|p| p.timeout),
        Some(defaults.timeout),
    );

    SiteManagerClient::from_api_key(&controller, &api_key, &transport).map_err(api_error)
}

pub async fn auto_resolve_host_id(global: &GlobalOpts) -> Result<String, CliError> {
    let client = build_site_manager_client(global)?;
    let hosts = client.list_hosts().await.map_err(api_error)?;
    choose_host_id(&hosts)
}

pub(crate) fn api_error(error: unifly_api::Error) -> CliError {
    CliError::from(CoreError::from(error))
}

pub(crate) async fn build_cloud_integration_client(
    profile: &Profile,
    profile_name: &str,
    global: &GlobalOpts,
) -> Result<IntegrationClient, CliError> {
    let api_key = resolve_cloud_api_key(Some(profile), profile_name, global)?;
    let controller = resolve_site_manager_url(Some(profile), global);
    let transport = cloud_transport(global, profile.timeout, None);
    let host_id = if let Some(host_id) = &global.host_id {
        host_id.clone()
    } else if let Ok(host_id) = config::resolve_host_id(profile) {
        host_id
    } else {
        auto_resolve_host_id(global).await?
    };

    IntegrationClient::from_api_key(
        &format!(
            "{}/v1/connector/consoles/{host_id}",
            controller.trim_end_matches('/')
        ),
        &api_key,
        &transport,
        ControllerPlatform::Cloud,
    )
    .map_err(api_error)
}

pub(crate) async fn load_cloud_connector_sites(
    profile: &Profile,
    profile_name: &str,
    global: &GlobalOpts,
) -> Result<Vec<SiteResponse>, CliError> {
    let integration = build_cloud_integration_client(profile, profile_name, global).await?;
    integration
        .paginate_all(50, |offset, limit| integration.list_sites(offset, limit))
        .await
        .map_err(api_error)
}

pub(crate) fn active_profile(global: &GlobalOpts) -> (String, Option<Profile>, Defaults) {
    let cfg = config::resolve::load_config_or_default();
    let profile_name = config::resolve::active_profile_name(global, &cfg);
    let profile = cfg.profiles.get(&profile_name).cloned();
    (profile_name, profile, cfg.defaults)
}

fn cloud_transport(
    global: &GlobalOpts,
    profile_timeout: Option<u64>,
    defaults_timeout: Option<u64>,
) -> TransportConfig {
    TransportConfig {
        tls: TlsMode::System,
        timeout: Duration::from_secs(global.timeout_secs(profile_timeout, defaults_timeout)),
        cookie_jar: None,
    }
}

pub(crate) fn resolve_cloud_api_key(
    profile: Option<&Profile>,
    profile_name: &str,
    global: &GlobalOpts,
) -> Result<SecretString, CliError> {
    if let Some(api_key) = &global.api_key {
        return Ok(SecretString::from(api_key.clone()));
    }

    let profile = profile.ok_or_else(|| CliError::NoCredentials {
        profile: profile_name.to_owned(),
    })?;

    config::resolve_api_key(profile, profile_name).map_err(CliError::from)
}

pub(crate) fn resolve_site_manager_url(profile: Option<&Profile>, global: &GlobalOpts) -> String {
    global
        .controller
        .clone()
        .or_else(|| {
            profile.and_then(|profile| {
                if profile.auth_mode == "cloud" && !profile.controller.trim().is_empty() {
                    Some(profile.controller.clone())
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| config::DEFAULT_CLOUD_CONTROLLER_URL.into())
}

fn choose_host_id(hosts: &[Host]) -> Result<String, CliError> {
    match hosts {
        [] => Err(CliError::Validation {
            field: "host_id".into(),
            reason: "no cloud consoles are accessible with the current API key".into(),
        }),
        [host] => Ok(host.id.clone()),
        _ => {
            let owner_hosts: Vec<&Host> =
                hosts.iter().filter(|host| host.is_owner_host()).collect();
            if owner_hosts.len() == 1 {
                return Ok(owner_hosts[0].id.clone());
            }

            let available = hosts
                .iter()
                .map(|host| format!("{} ({})", host.display_name(), host.id))
                .collect::<Vec<_>>()
                .join(", ");

            Err(CliError::Validation {
                field: "host_id".into(),
                reason: format!(
                    "multiple cloud consoles are available; set host_id in your profile or pass --host-id. Available hosts: {available}"
                ),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use unifly_api::site_manager_types::Host;

    use super::choose_host_id;
    use crate::cli::error::CliError;

    fn host(id: &str, name: &str, is_owner: bool) -> Host {
        Host {
            id: id.into(),
            name: Some(name.into()),
            model: None,
            firmware_version: None,
            mac_address: None,
            reported_state: None,
            user_data: None,
            is_owner: Some(is_owner),
            extra: std::collections::BTreeMap::default(),
        }
    }

    #[test]
    fn choose_host_prefers_single_owner_console() {
        let hosts = vec![host("host-1", "Home", true), host("host-2", "Lab", false)];
        let selected = choose_host_id(&hosts).expect("owner host should be selected");
        assert_eq!(selected, "host-1");
    }

    #[test]
    fn choose_host_errors_when_multiple_non_owner_hosts_exist() {
        let hosts = vec![host("host-1", "Home", false), host("host-2", "Lab", false)];
        let error = choose_host_id(&hosts).expect_err("selection should be ambiguous");
        assert!(matches!(error, CliError::Validation { field, .. } if field == "host_id"));
    }
}
