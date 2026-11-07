//! One translation from effective profile settings to controller transport.

use std::time::Duration;

use unifly_api::{ControllerConfig, TlsVerification};

use super::{
    ConfigError, DEFAULT_CLOUD_CONTROLLER_URL, Defaults, Profile, redact_url_userinfo,
    resolve_auth, resolve_totp_provider, resolve_totp_token,
};

/// Runtime scheduling differs between a command and a persistent dashboard.
#[derive(Clone, Copy)]
pub enum ConnectionUsage {
    Command,
    Interactive,
}

pub(super) fn build(
    profile: &Profile,
    profile_name: Option<&str>,
    defaults: &Defaults,
    usage: ConnectionUsage,
) -> Result<ControllerConfig, ConfigError> {
    let is_cloud = profile.auth_mode == "cloud";
    let controller_url = if is_cloud && profile.controller.trim().is_empty() {
        DEFAULT_CLOUD_CONTROLLER_URL
    } else {
        profile.controller.as_str()
    };

    let url: url::Url = controller_url
        .parse()
        .map_err(|_| ConfigError::Validation {
            field: "controller".into(),
            reason: format!("invalid URL: {}", redact_url_userinfo(controller_url)),
        })?;

    let auth = resolve_auth(profile, profile_name.unwrap_or("default"))?;

    // Profile insecure > profile ca_cert > defaults insecure: a CA is
    // still verification, so only an explicit insecure=true skips it.
    let tls = if is_cloud {
        TlsVerification::SystemDefaults
    } else if profile.insecure.unwrap_or(false) {
        TlsVerification::DangerAcceptInvalid
    } else if let Some(ref ca_path) = profile.ca_cert {
        TlsVerification::CustomCa(ca_path.clone())
    } else if profile.insecure.unwrap_or(defaults.insecure) {
        TlsVerification::DangerAcceptInvalid
    } else {
        TlsVerification::SystemDefaults
    };

    let timeout = Duration::from_secs(profile.timeout.unwrap_or(defaults.timeout));

    let totp_token = resolve_totp_token(profile);
    let totp_provider = resolve_totp_provider(profile)?;

    let mut config = ControllerConfig {
        url,
        auth,
        site: profile.site.clone(),
        tls,
        timeout,
        refresh_interval_secs: if is_cloud { 60 } else { 10 },
        websocket_enabled: !is_cloud,
        polling_interval_secs: if is_cloud { 30 } else { 10 },
        totp_token,
        totp_provider,
        profile_name: profile_name.map(str::to_owned),
        no_session_cache: is_cloud,
    };
    if matches!(usage, ConnectionUsage::Command) {
        config.refresh_interval_secs = 0;
        config.websocket_enabled = false;
        config.polling_interval_secs = 30;
    }
    Ok(config)
}
