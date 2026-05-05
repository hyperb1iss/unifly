use std::net::Ipv6Addr;
use std::sync::Arc;

use crate::config::{ControllerConfig, TlsVerification};
use crate::core_error::CoreError;
use crate::model::{EntityId, HealthSummary, MacAddress};
use crate::store::DataStore;
use crate::transport::{TlsMode, TransportConfig};
use crate::{IntegrationClient, SessionClient};

use super::Controller;

fn parse_ipv6_text(raw: &str) -> Option<Ipv6Addr> {
    let candidate = raw.trim().split('/').next().unwrap_or(raw).trim();
    candidate.parse::<Ipv6Addr>().ok()
}

fn pick_ipv6_from_value(value: &serde_json::Value) -> Option<String> {
    let mut first_link_local: Option<String> = None;

    let iter: Box<dyn Iterator<Item = &serde_json::Value> + '_> = match value {
        serde_json::Value::Array(items) => Box::new(items.iter()),
        _ => Box::new(std::iter::once(value)),
    };

    for item in iter {
        if let Some(ipv6) = item.as_str().and_then(parse_ipv6_text) {
            let ip_text = ipv6.to_string();
            if !ipv6.is_unicast_link_local() {
                return Some(ip_text);
            }
            if first_link_local.is_none() {
                first_link_local = Some(ip_text);
            }
        }
    }

    first_link_local
}

pub(super) fn parse_session_device_wan_ipv6(
    extra: &serde_json::Map<String, serde_json::Value>,
) -> Option<String> {
    if let Some(value) = extra
        .get("wan1")
        .and_then(|wan| wan.get("ipv6"))
        .and_then(pick_ipv6_from_value)
    {
        return Some(value);
    }

    extra.get("ipv6").and_then(pick_ipv6_from_value)
}

pub(super) fn convert_health_summaries(raw: Vec<serde_json::Value>) -> Vec<HealthSummary> {
    raw.into_iter()
        .map(|value| HealthSummary {
            subsystem: value
                .get("subsystem")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown")
                .to_owned(),
            status: value
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown")
                .to_owned(),
            #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
            num_adopted: value
                .get("num_adopted")
                .and_then(serde_json::Value::as_u64)
                .map(|value| value as u32),
            #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
            num_sta: value
                .get("num_sta")
                .and_then(serde_json::Value::as_u64)
                .map(|value| value as u32),
            tx_bytes_r: value.get("tx_bytes-r").and_then(serde_json::Value::as_u64),
            rx_bytes_r: value.get("rx_bytes-r").and_then(serde_json::Value::as_u64),
            latency: value.get("latency").and_then(serde_json::Value::as_f64),
            wan_ip: value
                .get("wan_ip")
                .and_then(|value| value.as_str())
                .map(String::from),
            gateways: value
                .get("gateways")
                .and_then(|value| value.as_array())
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|value| value.as_str().map(String::from))
                        .collect()
                }),
            extra: value,
        })
        .collect()
}

/// Build a [`TransportConfig`] from the controller configuration.
pub(super) fn build_transport(config: &ControllerConfig) -> TransportConfig {
    TransportConfig {
        tls: tls_to_transport(&config.tls),
        timeout: config.timeout,
        cookie_jar: None, // SessionClient::new adds one automatically
    }
}

pub(super) fn tls_to_transport(tls: &TlsVerification) -> TlsMode {
    match tls {
        TlsVerification::SystemDefaults => TlsMode::System,
        TlsVerification::CustomCa(path) => TlsMode::CustomCa(path.clone()),
        TlsVerification::DangerAcceptInvalid => TlsMode::DangerAcceptInvalid,
    }
}

/// A site resolved against the Integration API: the canonical UUID plus
/// the slug that the Session API expects in URL paths (`/api/s/<slug>/...`).
#[derive(Debug)]
pub(super) struct ResolvedSite {
    pub id: uuid::Uuid,
    pub slug: String,
}

/// Resolve a site identifier to its Integration UUID and Session slug.
///
/// Matches in priority order:
/// 1. UUID fast-path (input parses as `Uuid` and matches a known site).
/// 2. Exact `internal_reference` match (the slug, e.g. `default`).
/// 3. Exact `name` match (the display label, e.g. `Default` or `Home Network`).
/// 4. Case-insensitive match on either field.
///
/// Falling through to display-name matching avoids the trap where
/// `unifly sites list` shows the human-readable `name` column and the user
/// pastes that into config without realizing the slug is the canonical
/// identifier (issue #16). Both the UUID and the resolved slug are returned
/// so Session-backed callers can rebuild URLs against the correct slug
/// instead of the user's raw input.
pub(super) async fn resolve_site(
    client: &IntegrationClient,
    site_name: &str,
) -> Result<ResolvedSite, CoreError> {
    let sites = client
        .paginate_all(50, |off, lim| client.list_sites(off, lim))
        .await?;
    resolve_site_in(&sites, site_name)
}

/// Pure matching logic, factored out for direct unit testing without the
/// network round-trip in [`resolve_site`].
fn resolve_site_in(
    sites: &[crate::integration_types::SiteResponse],
    site_name: &str,
) -> Result<ResolvedSite, CoreError> {
    if let Ok(uuid) = uuid::Uuid::parse_str(site_name)
        && let Some(site) = sites.iter().find(|s| s.id == uuid)
    {
        return Ok(ResolvedSite {
            id: site.id,
            slug: site.internal_reference.clone(),
        });
    }

    // Slug is the canonical identifier; an exact slug match is unambiguous
    // by definition (UniFi enforces uniqueness server-side).
    if let Some(site) = sites.iter().find(|s| s.internal_reference == site_name) {
        return Ok(ResolvedSite {
            id: site.id,
            slug: site.internal_reference.clone(),
        });
    }

    // Fuzzy matches: collect every site that matches by display name or by
    // case-insensitive variants. Reject the operation if more than one
    // distinct site comes back -- silently picking the first hit can route
    // reads/writes to the wrong site.
    let fuzzy: Vec<&_> = sites
        .iter()
        .filter(|s| {
            s.name == site_name
                || s.internal_reference.eq_ignore_ascii_case(site_name)
                || s.name.eq_ignore_ascii_case(site_name)
        })
        .collect();

    let unique: std::collections::HashSet<uuid::Uuid> = fuzzy.iter().map(|s| s.id).collect();

    if unique.len() == 1 {
        let site = fuzzy[0];
        return Ok(ResolvedSite {
            id: site.id,
            slug: site.internal_reference.clone(),
        });
    }

    if unique.len() > 1 {
        let matches = fuzzy
            .into_iter()
            .map(|s| crate::core_error::SiteHint {
                internal_reference: s.internal_reference.clone(),
                display_name: s.name.clone(),
            })
            .collect();
        return Err(CoreError::SiteAmbiguous {
            name: site_name.to_owned(),
            matches,
        });
    }

    let available = sites
        .iter()
        .map(|s| crate::core_error::SiteHint {
            internal_reference: s.internal_reference.clone(),
            display_name: s.name.clone(),
        })
        .collect();

    Err(CoreError::SiteNotFound {
        name: site_name.to_owned(),
        available,
    })
}

/// Extract a `Uuid` from an `EntityId`, or return an error.
pub(super) fn require_uuid(id: &EntityId) -> Result<uuid::Uuid, CoreError> {
    id.as_uuid().copied().ok_or_else(|| CoreError::Unsupported {
        operation: "Integration API operation on legacy ID".into(),
        required: "UUID-based entity ID".into(),
    })
}

pub(super) fn require_session(
    session: Option<&Arc<SessionClient>>,
) -> Result<&SessionClient, CoreError> {
    session
        .map(Arc::as_ref)
        .ok_or_else(|| CoreError::Unsupported {
            operation: "Session API operation".into(),
            required: "Session API credentials (session or hybrid auth mode)".into(),
        })
}

pub(super) fn require_integration<'a>(
    integration: Option<&'a Arc<IntegrationClient>>,
    site_id: Option<uuid::Uuid>,
    operation: &str,
) -> Result<(&'a IntegrationClient, uuid::Uuid), CoreError> {
    let client = integration
        .map(Arc::as_ref)
        .ok_or_else(|| unsupported(operation))?;
    let sid = site_id.ok_or_else(|| unsupported(operation))?;
    Ok((client, sid))
}

pub(super) async fn integration_client_context(
    controller: &Controller,
    operation: &str,
) -> Result<Arc<IntegrationClient>, CoreError> {
    controller
        .inner
        .integration_client
        .lock()
        .await
        .as_ref()
        .cloned()
        .ok_or_else(|| unsupported(operation))
}

pub(super) async fn integration_site_context(
    controller: &Controller,
    operation: &str,
) -> Result<(Arc<IntegrationClient>, uuid::Uuid), CoreError> {
    let client = integration_client_context(controller, operation).await?;
    let site_id = controller
        .inner
        .site_id
        .lock()
        .await
        .ok_or_else(|| unsupported(operation))?;
    Ok((client, site_id))
}

pub(super) fn unsupported(operation: &str) -> CoreError {
    CoreError::Unsupported {
        operation: operation.into(),
        required: "Integration API".into(),
    }
}

/// Resolve an [`EntityId`] to a device MAC via the DataStore.
pub(super) fn device_mac(store: &DataStore, id: &EntityId) -> Result<MacAddress, CoreError> {
    store
        .device_by_id(id)
        .map(|device| device.mac.clone())
        .ok_or_else(|| CoreError::DeviceNotFound {
            identifier: id.to_string(),
        })
}

/// Resolve an [`EntityId`] to a client MAC via the DataStore.
pub(super) fn client_mac(store: &DataStore, id: &EntityId) -> Result<MacAddress, CoreError> {
    store
        .client_by_id(id)
        .map(|client| client.mac.clone())
        .ok_or_else(|| CoreError::ClientNotFound {
            identifier: id.to_string(),
        })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::resolve_site_in;
    use crate::core_error::CoreError;
    use crate::integration_types::SiteResponse;
    use uuid::Uuid;

    fn site(id: &str, internal: &str, name: &str) -> SiteResponse {
        SiteResponse {
            id: Uuid::parse_str(id).expect("valid uuid"),
            internal_reference: internal.into(),
            name: name.into(),
        }
    }

    fn default_site() -> SiteResponse {
        site(
            "11111111-1111-1111-1111-111111111111",
            "default",
            "Main Site",
        )
    }

    fn guest_site() -> SiteResponse {
        site(
            "22222222-2222-2222-2222-222222222222",
            "guest",
            "Guest Network",
        )
    }

    #[test]
    fn matches_known_uuid_returns_canonical_slug() {
        let sites = vec![default_site(), guest_site()];
        let resolved = resolve_site_in(&sites, "11111111-1111-1111-1111-111111111111").unwrap();
        assert_eq!(resolved.slug, "default");
    }

    #[test]
    fn unmatched_uuid_falls_through_to_not_found() {
        let sites = vec![default_site()];
        let err = resolve_site_in(&sites, "33333333-3333-3333-3333-333333333333").unwrap_err();
        assert!(matches!(err, CoreError::SiteNotFound { .. }));
    }

    #[test]
    fn exact_slug_match_wins_over_case_insensitive_name_collision() {
        // Site A has slug "default" + name "Default Site". Site B has slug
        // "default-backup" + name "default" (a contrived but legal label).
        // Input "default" must match Site A by exact slug, not Site B by name.
        let target = site(
            "44444444-4444-4444-4444-444444444444",
            "default",
            "Default Site",
        );
        let collision = site(
            "55555555-5555-5555-5555-555555555555",
            "default-backup",
            "default",
        );
        let sites = vec![target, collision];
        let resolved = resolve_site_in(&sites, "default").unwrap();
        assert_eq!(
            resolved.id.to_string(),
            "44444444-4444-4444-4444-444444444444"
        );
        assert_eq!(resolved.slug, "default");
    }

    #[test]
    fn duplicate_display_names_return_ambiguous() {
        let a = site("66666666-6666-6666-6666-666666666666", "home1", "Home");
        let b = site("77777777-7777-7777-7777-777777777777", "home2", "Home");
        let sites = vec![a, b];
        let err = resolve_site_in(&sites, "Home").unwrap_err();
        match err {
            CoreError::SiteAmbiguous { matches, .. } => assert_eq!(matches.len(), 2),
            other => panic!("expected SiteAmbiguous, got {other:?}"),
        }
    }

    #[test]
    fn case_insensitive_name_match_against_single_site_succeeds() {
        let sites = vec![default_site(), guest_site()];
        let resolved = resolve_site_in(&sites, "GUEST NETWORK").unwrap();
        assert_eq!(resolved.slug, "guest");
    }

    #[test]
    fn no_match_returns_not_found_with_full_candidate_list() {
        let sites = vec![default_site(), guest_site()];
        let err = resolve_site_in(&sites, "iot").unwrap_err();
        match err {
            CoreError::SiteNotFound { available, .. } => assert_eq!(available.len(), 2),
            other => panic!("expected SiteNotFound, got {other:?}"),
        }
    }
}
