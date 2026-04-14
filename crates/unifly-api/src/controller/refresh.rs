use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use futures_util::stream::{self, StreamExt};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::core_error::CoreError;
use crate::model::{
    AclRule, Client, Device, DnsPolicy, EntityId, Event, FirewallGroup, FirewallPolicy,
    FirewallZone, HealthSummary, MacAddress, NatPolicy, Network, Site, TrafficMatchingList,
    Voucher, WifiBroadcast,
};
use crate::store::{DataStore, event_storage_key};

use super::support::{convert_health_summaries, parse_session_device_wan_ipv6};
use super::{Controller, REFRESH_DETAIL_CONCURRENCY};

impl Controller {
    /// Fetch all data from the controller and update the DataStore.
    ///
    /// Pulls devices, clients, and events from the controller APIs, converts
    /// them to domain types, and applies them to the store. Events are
    /// broadcast through the event channel after snapshot application.
    #[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
    pub async fn full_refresh(&self) -> Result<(), CoreError> {
        let integration = self.inner.integration_client.lock().await.clone();
        let site_id = *self.inner.site_id.lock().await;

        if let (Some(integration), Some(sid)) = (integration, site_id) {
            let page_limit = 200;

            let (devices_res, clients_res, networks_res, wifi_res) = tokio::join!(
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_devices(&sid, off, lim)
                }),
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_clients(&sid, off, lim)
                }),
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_networks(&sid, off, lim)
                }),
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_wifi_broadcasts(&sid, off, lim)
                }),
            );

            let (policies_res, zones_res, acls_res, dns_res, vouchers_res) = tokio::join!(
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_firewall_policies(&sid, off, lim)
                }),
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_firewall_zones(&sid, off, lim)
                }),
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_acl_rules(&sid, off, lim)
                }),
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_dns_policies(&sid, off, lim)
                }),
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_vouchers(&sid, off, lim)
                }),
            );

            let (sites_res, tml_res) = tokio::join!(
                integration.paginate_all(50, |off, lim| integration.list_sites(off, lim)),
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_traffic_matching_lists(&sid, off, lim)
                }),
            );

            let devices: Vec<Device> = devices_res?.into_iter().map(Device::from).collect();
            let mut clients: Vec<Client> = clients_res?.into_iter().map(Client::from).collect();
            let network_ids: Vec<uuid::Uuid> = networks_res?
                .into_iter()
                .map(|network| network.id)
                .collect();
            info!(
                network_count = network_ids.len(),
                "fetching network details"
            );
            let networks: Vec<Network> = {
                stream::iter(network_ids.into_iter().map(|network_id| {
                    let integration = Arc::clone(&integration);
                    async move {
                        match integration.get_network(&sid, &network_id).await {
                            Ok(detail) => Some(Network::from(detail)),
                            Err(error) => {
                                warn!(network_id = %network_id, error = %error, "network detail fetch failed");
                                None
                            }
                        }
                    }
                }))
                .buffer_unordered(REFRESH_DETAIL_CONCURRENCY)
                .filter_map(async move |network| network)
                .collect::<Vec<_>>()
                .await
            };
            let wifi: Vec<WifiBroadcast> = wifi_res?.into_iter().map(WifiBroadcast::from).collect();
            let sites: Vec<Site> = sites_res?.into_iter().map(Site::from).collect();
            let traffic_matching_lists: Vec<TrafficMatchingList> = tml_res?
                .into_iter()
                .map(TrafficMatchingList::from)
                .collect();

            // Optional endpoints — errors (404, not-configured, etc.) are non-fatal
            let policies: Vec<FirewallPolicy> = unwrap_or_empty("firewall/policies", policies_res);
            let zones: Vec<FirewallZone> = unwrap_or_empty("firewall/zones", zones_res);
            let acls: Vec<AclRule> = unwrap_or_empty("acl/rules", acls_res);
            let dns: Vec<DnsPolicy> = unwrap_or_empty("dns/policies", dns_res);
            let vouchers: Vec<Voucher> = unwrap_or_empty("vouchers", vouchers_res);

            info!(
                device_count = devices.len(),
                "enriching devices with statistics"
            );
            let mut devices = {
                stream::iter(devices.into_iter().map(|mut device| {
                    let integration = Arc::clone(&integration);
                    async move {
                        if let EntityId::Uuid(device_uuid) = &device.id {
                            match integration.get_device_statistics(&sid, device_uuid).await {
                                Ok(stats_resp) => {
                                    device.stats =
                                        crate::convert::device_stats_from_integration(&stats_resp);
                                    crate::convert::enrich_radios_from_stats(
                                        &mut device.radios,
                                        &stats_resp.interfaces,
                                    );
                                }
                                Err(error) => {
                                    warn!(device = ?device.name, error = %error, "device stats fetch failed");
                                }
                            }
                        }
                        device
                    }
                }))
                .buffer_unordered(REFRESH_DETAIL_CONCURRENCY)
                .collect::<Vec<_>>()
                .await
            };

            #[allow(clippy::type_complexity)]
            let (
                session_events,
                session_health,
                session_clients,
                session_devices,
                session_users,
                nat,
                firewall_groups,
            ): (
                Vec<Event>,
                Vec<HealthSummary>,
                Vec<crate::session::models::SessionClientEntry>,
                Vec<crate::session::models::SessionDevice>,
                Vec<crate::session::models::SessionUserEntry>,
                Vec<NatPolicy>,
                Vec<FirewallGroup>,
            ) = match self.inner.session_client.lock().await.clone() {
                Some(session) => {
                    let (
                        events_res,
                        health_res,
                        clients_res,
                        devices_res,
                        users_res,
                        nat_res,
                        fwg_res,
                    ) = tokio::join!(
                        session.list_events(Some(100)),
                        session.get_health(),
                        session.list_clients(),
                        session.list_devices(),
                        session.list_users(),
                        session.list_nat_rules(),
                        session.list_firewall_groups(),
                    );

                    let events = match events_res {
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
                    };

                    let health = match health_res {
                        Ok(raw) => convert_health_summaries(raw),
                        Err(error) => {
                            warn!(error = %error, "session health fetch failed (non-fatal)");
                            Vec::new()
                        }
                    };

                    let session_clients = match clients_res {
                        Ok(raw) => raw,
                        Err(error) => {
                            warn!(error = %error, "session client fetch failed (non-fatal)");
                            Vec::new()
                        }
                    };

                    let session_devices = match devices_res {
                        Ok(raw) => raw,
                        Err(error) => {
                            warn!(error = %error, "session device fetch failed (non-fatal)");
                            Vec::new()
                        }
                    };

                    let session_users = match users_res {
                        Ok(raw) => raw,
                        Err(error) => {
                            warn!(error = %error, "session user fetch failed (non-fatal)");
                            Vec::new()
                        }
                    };

                    let nat = match nat_res {
                        Ok(raw) => raw
                            .iter()
                            .filter_map(crate::convert::nat_policy_from_v2)
                            .collect(),
                        Err(error) => {
                            warn!(error = %error, "v2 NAT fetch failed (non-fatal)");
                            Vec::new()
                        }
                    };

                    let firewall_groups = match fwg_res {
                        Ok(raw) => raw
                            .iter()
                            .filter_map(crate::convert::firewall_group_from_session)
                            .collect(),
                        Err(error) => {
                            warn!(error = %error, "firewall group fetch failed (non-fatal)");
                            Vec::new()
                        }
                    };

                    (
                        events,
                        health,
                        session_clients,
                        session_devices,
                        session_users,
                        nat,
                        firewall_groups,
                    )
                }
                None => (
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
            };

            if !session_clients.is_empty() {
                let session_by_ip: HashMap<&str, &crate::session::models::SessionClientEntry> =
                    session_clients
                        .iter()
                        .filter_map(|client| client.ip.as_deref().map(|ip| (ip, client)))
                        .collect();
                let mut merged = 0u32;
                for client in &mut clients {
                    let ip_key = client.ip.map(|ip| ip.to_string());
                    if let Some(session_client) =
                        ip_key.as_deref().and_then(|ip| session_by_ip.get(ip))
                    {
                        if client.tx_bytes.is_none() {
                            client.tx_bytes = session_client
                                .tx_bytes
                                .and_then(|bytes| u64::try_from(bytes).ok());
                        }
                        if client.rx_bytes.is_none() {
                            client.rx_bytes = session_client
                                .rx_bytes
                                .and_then(|bytes| u64::try_from(bytes).ok());
                        }
                        if client.hostname.is_none() {
                            client.hostname.clone_from(&session_client.hostname);
                        }
                        if client.wireless.is_none() {
                            let session_client: Client = Client::from((*session_client).clone());
                            client.wireless = session_client.wireless;
                        }
                        if client.uplink_device_mac.is_none() {
                            let uplink = if session_client.is_wired.unwrap_or(true) {
                                session_client.sw_mac.as_deref()
                            } else {
                                session_client.ap_mac.as_deref()
                            };
                            client.uplink_device_mac = uplink.map(MacAddress::new);
                        }
                        merged += 1;
                    }
                }
                debug!(
                    total_clients = clients.len(),
                    legacy_available = session_by_ip.len(),
                    merged,
                    "client traffic merge (by IP)"
                );
            }

            if !session_users.is_empty() {
                let users_by_mac: HashMap<String, &crate::session::models::SessionUserEntry> =
                    session_users
                        .iter()
                        .map(|user| (user.mac.to_lowercase(), user))
                        .collect();
                let mut merged_users = 0u32;
                for client in &mut clients {
                    // Try MAC first, then fall back to matching the session
                    // client entry (already joined by IP) whose MAC maps to
                    // a user record. The Integration API may return UUIDs
                    // instead of real MACs when access.macAddress is absent.
                    let user = users_by_mac
                        .get(&client.mac.as_str().to_lowercase())
                        .or_else(|| {
                            let ip_str = client.ip.map(|ip| ip.to_string())?;
                            let session_client = session_clients
                                .iter()
                                .find(|lc| lc.ip.as_deref() == Some(ip_str.as_str()))?;
                            users_by_mac.get(&session_client.mac.to_lowercase())
                        });
                    if let Some(user) = user {
                        client.use_fixedip = user.use_fixedip.unwrap_or(false);
                        client.fixed_ip = user.fixed_ip.as_deref().and_then(|ip| ip.parse().ok());
                        if client.use_fixedip {
                            merged_users += 1;
                        }
                    }
                }
                debug!(
                    users_available = users_by_mac.len(),
                    merged_users, "user DHCP reservation merge"
                );
            }

            if !session_devices.is_empty() {
                let session_by_mac: HashMap<&str, &crate::session::models::SessionDevice> =
                    session_devices
                        .iter()
                        .map(|device| (device.mac.as_str(), device))
                        .collect();
                for device in &mut devices {
                    if let Some(legacy_device) = session_by_mac.get(device.mac.as_str()) {
                        if device.client_count.is_none() {
                            device.client_count = legacy_device
                                .num_sta
                                .and_then(|count| count.try_into().ok());
                        }
                        if device.wan_ipv6.is_none() {
                            device.wan_ipv6 = parse_session_device_wan_ipv6(&legacy_device.extra);
                        }
                        if device.ports.is_empty() || device.radios.is_empty() {
                            let session_dev: Device = Device::from((*legacy_device).clone());
                            if device.ports.is_empty() && !session_dev.ports.is_empty() {
                                device.ports = session_dev.ports;
                            }
                            if device.radios.is_empty() && !session_dev.radios.is_empty() {
                                device.radios = session_dev.radios;
                            }
                        }
                    }
                }
            }

            if !session_health.is_empty() {
                self.inner
                    .store
                    .site_health
                    .send_modify(|health| *health = Arc::new(session_health));
            }

            let fresh_legacy_events = unseen_events(self.store(), &session_events);

            self.inner
                .store
                .apply_integration_snapshot(crate::store::RefreshSnapshot {
                    devices,
                    clients,
                    networks,
                    wifi,
                    policies,
                    zones,
                    acls,
                    nat,
                    dns,
                    vouchers,
                    sites,
                    events: session_events,
                    traffic_matching_lists,
                    firewall_groups,
                });

            for event in fresh_legacy_events {
                let _ = self.inner.event_tx.send(Arc::new(event));
            }
        } else {
            let session = self
                .inner
                .session_client
                .lock()
                .await
                .clone()
                .ok_or(CoreError::ControllerDisconnected)?;

            let (devices_res, clients_res, events_res, sites_res) = tokio::join!(
                session.list_devices(),
                session.list_clients(),
                session.list_events(Some(100)),
                session.list_sites(),
            );

            let devices: Vec<Device> = devices_res?.into_iter().map(Device::from).collect();
            let clients: Vec<Client> = clients_res?.into_iter().map(Client::from).collect();
            let events: Vec<Event> = events_res?.into_iter().map(Event::from).collect();
            let sites: Vec<Site> = sites_res?.into_iter().map(Site::from).collect();
            let fresh_events = unseen_events(self.store(), &events);

            self.inner
                .store
                .apply_integration_snapshot(crate::store::RefreshSnapshot {
                    devices,
                    clients,
                    networks: Vec::new(),
                    wifi: Vec::new(),
                    policies: Vec::new(),
                    zones: Vec::new(),
                    acls: Vec::new(),
                    nat: Vec::new(),
                    dns: Vec::new(),
                    vouchers: Vec::new(),
                    sites,
                    events,
                    traffic_matching_lists: Vec::new(),
                    firewall_groups: Vec::new(),
                });

            for event in fresh_events {
                let _ = self.inner.event_tx.send(Arc::new(event));
            }
        }

        debug!(
            devices = self.inner.store.device_count(),
            clients = self.inner.store.client_count(),
            "data refresh complete"
        );

        Ok(())
    }
}

/// Periodically refresh data from the controller.
pub(super) async fn refresh_task(
    controller: Controller,
    interval_secs: u64,
    cancel: CancellationToken,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
    interval.tick().await;

    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => break,
            _ = interval.tick() => {
                if let Err(error) = controller.full_refresh().await {
                    warn!(error = %error, "periodic refresh failed");
                }
            }
        }
    }
}

/// Downgrade a paginated result to an empty `Vec` when the endpoint returns 404.
///
/// Some Integration API endpoints are optional on older controller firmware.
fn unwrap_or_empty<S, D>(endpoint: &str, result: Result<Vec<S>, crate::error::Error>) -> Vec<D>
where
    D: From<S>,
{
    match result {
        Ok(items) => items.into_iter().map(D::from).collect(),
        Err(ref error) if error.is_not_found() => {
            debug!("{endpoint}: not available (404), treating as empty");
            Vec::new()
        }
        Err(error) => {
            warn!("{endpoint}: unexpected error {error}, treating as empty");
            Vec::new()
        }
    }
}

fn unseen_events(store: &DataStore, events: &[Event]) -> Vec<Event> {
    let mut seen: HashSet<String> = store
        .events_snapshot()
        .iter()
        .map(|event| event_storage_key(event))
        .collect();

    events
        .iter()
        .filter(|event| seen.insert(event_storage_key(event)))
        .cloned()
        .collect()
}
