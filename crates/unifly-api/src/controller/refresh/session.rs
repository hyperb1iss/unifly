use std::sync::Arc;

use tracing::{debug, warn};

use crate::SessionClient;
use crate::core_error::CoreError;
use crate::model::{Client, Device, Event, FirewallGroup, HealthSummary, NatPolicy, Site};
use crate::session::models::{SessionClientEntry, SessionDevice, SessionUserEntry};

use super::super::support::convert_health_summaries;

#[derive(Default)]
pub(super) struct OptionalSessionRefresh {
    pub(super) events: Vec<Event>,
    pub(super) health: Vec<HealthSummary>,
    pub(super) clients: Vec<SessionClientEntry>,
    pub(super) devices: Vec<SessionDevice>,
    pub(super) users: Vec<SessionUserEntry>,
    pub(super) nat: Vec<NatPolicy>,
    pub(super) firewall_groups: Vec<FirewallGroup>,
}

pub(super) struct SessionOnlyRefresh {
    pub(super) devices: Vec<Device>,
    pub(super) clients: Vec<Client>,
    pub(super) events: Vec<Event>,
    pub(super) sites: Vec<Site>,
    pub(super) firewall_groups: Vec<FirewallGroup>,
    pub(super) nat: Vec<NatPolicy>,
}

pub(super) async fn fetch_optional(session: Option<Arc<SessionClient>>) -> OptionalSessionRefresh {
    let Some(session) = session else {
        return OptionalSessionRefresh::default();
    };

    let (events_res, health_res, clients_res, devices_res, users_res, nat_res, fwg_res) = tokio::join!(
        session.list_events(Some(100)),
        session.get_health(),
        session.list_clients(),
        session.list_devices(),
        session.list_users(),
        session.list_nat_rules(),
        session.list_firewall_groups(),
    );

    OptionalSessionRefresh {
        events: optional_events(&session, events_res),
        health: match health_res {
            Ok(raw) => convert_health_summaries(raw),
            Err(error) => {
                warn!(error = %error, "session health fetch failed (non-fatal)");
                Vec::new()
            }
        },
        clients: optional_raw("session client", clients_res),
        devices: optional_raw("session device", devices_res),
        users: optional_raw("session user", users_res),
        nat: nat_or_empty(nat_res),
        firewall_groups: firewall_groups_or_empty(fwg_res),
    }
}

pub(super) async fn fetch_required(
    session: Arc<SessionClient>,
) -> Result<SessionOnlyRefresh, CoreError> {
    let (devices_res, clients_res, events_res, sites_res, fwg_res, nat_res) = tokio::join!(
        session.list_devices(),
        session.list_clients(),
        session.list_events(Some(100)),
        session.list_sites(),
        session.list_firewall_groups(),
        session.list_nat_rules(),
    );

    Ok(SessionOnlyRefresh {
        devices: devices_res?.into_iter().map(Device::from).collect(),
        clients: clients_res?.into_iter().map(Client::from).collect(),
        events: events_res?.into_iter().map(Event::from).collect(),
        sites: sites_res?.into_iter().map(Site::from).collect(),
        firewall_groups: firewall_groups_or_empty(fwg_res),
        nat: nat_or_empty(nat_res),
    })
}

fn nat_or_empty(result: Result<Vec<serde_json::Value>, crate::Error>) -> Vec<NatPolicy> {
    match result {
        Ok(raw) => raw
            .iter()
            .filter_map(crate::convert::nat_policy_from_v2)
            .collect(),
        Err(error) => {
            warn!(error = %error, "v2 NAT fetch failed (non-fatal)");
            Vec::new()
        }
    }
}

fn optional_events(
    session: &SessionClient,
    result: Result<Vec<crate::session::models::SessionEvent>, crate::Error>,
) -> Vec<Event> {
    match result {
        Ok(raw) => raw.into_iter().map(Event::from).collect(),
        Err(ref error) if error.is_not_found() => {
            debug!(
                auth = ?session.auth(),
                error = %error,
                "session event endpoint unavailable; treating as empty"
            );
            Vec::new()
        }
        Err(error) => {
            warn!(
                auth = ?session.auth(),
                error = %error,
                "session event fetch failed (non-fatal)"
            );
            Vec::new()
        }
    }
}

fn optional_raw<T>(label: &str, result: Result<Vec<T>, crate::Error>) -> Vec<T> {
    match result {
        Ok(raw) => raw,
        Err(error) => {
            warn!(error = %error, "{label} fetch failed (non-fatal)");
            Vec::new()
        }
    }
}

fn firewall_groups_or_empty(
    result: Result<Vec<serde_json::Value>, crate::Error>,
) -> Vec<FirewallGroup> {
    match result {
        Ok(raw) => raw
            .iter()
            .filter_map(crate::convert::firewall_group_from_session)
            .collect(),
        Err(error) => {
            warn!(error = %error, "firewall group fetch failed (non-fatal)");
            Vec::new()
        }
    }
}
