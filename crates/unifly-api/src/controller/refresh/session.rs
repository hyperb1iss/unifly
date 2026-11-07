use std::sync::Arc;

use crate::SessionClient;
use crate::controller::event_history::{EventHistoryQuery, fetch_events};
use crate::core_error::CoreError;
use crate::model::{Client, Device, Event, FirewallGroup, HealthSummary, NatPolicy, Site};
use crate::session::models::{SessionClientEntry, SessionDevice, SessionUserEntry};

use super::super::support::convert_health_summaries;
use super::RefreshScope;
use super::optional::keep_on_failure;

/// Session data that supplements an Integration refresh. Every field is
/// optional: `None` means the fetch failed (see [`keep_on_failure`]) or
/// there is no session client, or the scope excludes it. The store keeps what it has.
#[derive(Default)]
pub(super) struct OptionalSessionRefresh {
    pub(super) events: Option<Vec<Event>>,
    pub(super) health: Option<Vec<HealthSummary>>,
    pub(super) clients: Option<Vec<SessionClientEntry>>,
    pub(super) devices: Option<Vec<SessionDevice>>,
    pub(super) users: Option<Vec<SessionUserEntry>>,
    pub(super) nat: Option<Vec<NatPolicy>>,
    pub(super) firewall_groups: Option<Vec<FirewallGroup>>,
}

/// Session data for a refresh without the Integration API. Devices, clients,
/// and sites are required; events, NAT, and firewall groups are optional.
pub(super) struct SessionOnlyRefresh {
    pub(super) devices: Vec<Device>,
    pub(super) clients: Vec<Client>,
    pub(super) events: Option<Vec<Event>>,
    pub(super) sites: Vec<Site>,
    pub(super) firewall_groups: Option<Vec<FirewallGroup>>,
    pub(super) nat: Option<Vec<NatPolicy>>,
}

pub(super) async fn fetch_optional(
    session: Option<Arc<SessionClient>>,
    scope: RefreshScope,
) -> OptionalSessionRefresh {
    let Some(session) = session else {
        return OptionalSessionRefresh::default();
    };

    let (events_res, health_res, clients_res, devices_res, users_res, nat_res, fwg_res) = tokio::join!(
        fetch_event_history(&session, scope),
        session.get_health(),
        session.list_clients(),
        session.list_devices(),
        session.list_users(),
        session.list_nat_rules(),
        session.list_firewall_groups(),
    );

    OptionalSessionRefresh {
        events: events_res,
        health: keep_on_failure("stat/health", health_res).map(convert_health_summaries),
        clients: keep_on_failure("stat/sta", clients_res),
        devices: keep_on_failure("stat/device", devices_res),
        users: keep_on_failure("rest/user", users_res),
        nat: nat_policies(nat_res),
        firewall_groups: firewall_groups(fwg_res),
    }
}

pub(super) async fn fetch_required(
    session: Arc<SessionClient>,
    scope: RefreshScope,
) -> Result<SessionOnlyRefresh, CoreError> {
    let (devices_res, clients_res, events_res, sites_res, fwg_res, nat_res) = tokio::join!(
        session.list_devices(),
        session.list_clients(),
        fetch_event_history(&session, scope),
        session.list_sites(),
        session.list_firewall_groups(),
        session.list_nat_rules(),
    );

    Ok(SessionOnlyRefresh {
        devices: devices_res?.into_iter().map(Device::from).collect(),
        clients: clients_res?.into_iter().map(Client::from).collect(),
        events: events_res,
        sites: sites_res?.into_iter().map(Site::from).collect(),
        firewall_groups: firewall_groups(fwg_res),
        nat: nat_policies(nat_res),
    })
}

/// Preserve history under every auth mode when the event endpoint is unavailable.
async fn fetch_event_history(session: &SessionClient, scope: RefreshScope) -> Option<Vec<Event>> {
    if matches!(scope, RefreshScope::Inventory) {
        return None;
    }
    let result = fetch_events(session, EventHistoryQuery::default()).await;
    // The auth mode goes into the label for diagnosing a missing endpoint.
    let endpoint = format!("event history ({:?} auth)", session.auth());
    keep_on_failure(&endpoint, result)
}

fn nat_policies(result: Result<Vec<serde_json::Value>, crate::Error>) -> Option<Vec<NatPolicy>> {
    keep_on_failure("v2 nat", result).map(|raw| {
        raw.iter()
            .filter_map(crate::convert::nat_policy_from_v2)
            .collect()
    })
}

fn firewall_groups(
    result: Result<Vec<serde_json::Value>, crate::Error>,
) -> Option<Vec<FirewallGroup>> {
    keep_on_failure("rest/firewallgroup", result).map(|raw| {
        raw.iter()
            .filter_map(crate::convert::firewall_group_from_session)
            .collect()
    })
}
