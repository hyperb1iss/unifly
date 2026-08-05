use unifly_api::integration_types::SiteResponse;

use crate::cli::args::{CloudSwitchArgs, GlobalOpts};
use crate::cli::error::CliError;
use crate::config::{self, Profile};

use super::load_cloud_connector_sites;

fn available_profiles(cfg: &config::Config) -> String {
    let mut available: Vec<_> = cfg.profiles.keys().cloned().collect();
    available.sort();
    if available.is_empty() {
        "(none)".into()
    } else {
        available.join(", ")
    }
}

fn require_cloud_profile(
    global: &GlobalOpts,
) -> Result<(config::Config, String, Profile), CliError> {
    let cfg = config::resolve::load_config_or_default();
    let profile_name = config::resolve::active_profile_name(global, &cfg);
    let Some(profile) = cfg.profiles.get(&profile_name).cloned() else {
        return Err(CliError::ProfileNotFound {
            name: profile_name,
            available: available_profiles(&cfg),
        });
    };

    if profile.auth_mode != "cloud" {
        return Err(CliError::Unsupported {
            operation: "cloud switch".into(),
            required: "an active cloud profile".into(),
        });
    }

    Ok((cfg, profile_name, profile))
}

fn site_label(site: &SiteResponse) -> String {
    if site.name == site.internal_reference {
        site.name.clone()
    } else {
        format!("{} ({})", site.name, site.internal_reference)
    }
}

fn resolve_site<'a>(sites: &'a [SiteResponse], query: &str) -> Result<&'a SiteResponse, CliError> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err(CliError::Validation {
            field: "site".into(),
            reason: "site cannot be empty".into(),
        });
    }

    if let Some(site) = sites.iter().find(|site| {
        site.internal_reference == trimmed || site.name == trimmed || site.id.to_string() == trimmed
    }) {
        return Ok(site);
    }

    let folded = trimmed.to_ascii_lowercase();
    let mut matches = sites
        .iter()
        .filter(|site| {
            site.internal_reference.eq_ignore_ascii_case(trimmed)
                || site.name.eq_ignore_ascii_case(trimmed)
                || site.id.to_string().eq_ignore_ascii_case(trimmed)
        })
        .collect::<Vec<_>>();

    if matches.len() == 1 {
        return Ok(matches.remove(0));
    }

    if matches.len() > 1 {
        let choices = matches
            .iter()
            .map(|site| site_label(site))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(CliError::Validation {
            field: "site".into(),
            reason: format!("'{query}' matches multiple sites: {choices}"),
        });
    }

    let available = sites
        .iter()
        .filter(|site| {
            site.internal_reference
                .to_ascii_lowercase()
                .contains(&folded)
                || site.name.to_ascii_lowercase().contains(&folded)
                || site.id.to_string().to_ascii_lowercase().contains(&folded)
        })
        .map(site_label)
        .collect::<Vec<_>>();

    let reason = if available.is_empty() {
        format!("unknown site '{query}'")
    } else {
        format!(
            "unknown site '{query}'. Close matches: {}",
            available.join(", ")
        )
    };

    Err(CliError::Validation {
        field: "site".into(),
        reason,
    })
}

pub async fn handle(args: CloudSwitchArgs, global: &GlobalOpts) -> Result<(), CliError> {
    let (mut cfg, profile_name, profile) = require_cloud_profile(global)?;
    let sites =
        load_cloud_connector_sites(&profile, &profile_name, global, Some(cfg.defaults.timeout))
            .await?;
    let site = resolve_site(&sites, &args.site)?;

    let Some(active_profile) = cfg.profiles.get_mut(&profile_name) else {
        unreachable!("active profile was loaded from this config");
    };
    active_profile.site.clone_from(&site.internal_reference);

    crate::config::save_config(&cfg)?;

    if !global.quiet {
        eprintln!(
            "✓ Cloud profile '{}' now targets site '{}' ({})",
            profile_name, site.name, site.internal_reference
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::resolve_site;
    use crate::cli::error::CliError;
    use unifly_api::integration_types::SiteResponse;

    fn site(id: &str, name: &str, internal_reference: &str) -> SiteResponse {
        SiteResponse {
            id: Uuid::parse_str(id).expect("site UUID"),
            name: name.into(),
            internal_reference: internal_reference.into(),
        }
    }

    #[test]
    fn resolve_site_matches_internal_reference() {
        let sites = vec![site(
            "00000000-0000-0000-0000-000000000001",
            "Default",
            "default",
        )];

        let resolved = resolve_site(&sites, "default").expect("site should resolve");
        assert_eq!(resolved.internal_reference, "default");
    }

    #[test]
    fn resolve_site_matches_name_case_insensitively() {
        let sites = vec![site(
            "00000000-0000-0000-0000-000000000001",
            "Work Network",
            "default",
        )];

        let resolved = resolve_site(&sites, "work network").expect("site should resolve");
        assert_eq!(resolved.name, "Work Network");
    }

    #[test]
    fn resolve_site_reports_close_matches() {
        let sites = vec![
            site("00000000-0000-0000-0000-000000000001", "Work HQ", "default"),
            site("00000000-0000-0000-0000-000000000002", "Work Lab", "lab"),
        ];

        let error = resolve_site(&sites, "work").expect_err("site should be unresolved");
        assert!(matches!(error, CliError::Validation { ref field, .. } if field == "site"));
        assert!(error.to_string().contains("Close matches"));
    }
}
