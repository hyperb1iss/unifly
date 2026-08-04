use std::sync::Arc;

use futures_util::stream::{self, StreamExt};
use tracing::{info, warn};

use crate::IntegrationClient;
use crate::core_error::CoreError;
use crate::model::{
    AclRule, Client, Device, DnsPolicy, EntityId, FirewallPolicy, FirewallZone, Network, Site,
    TrafficMatchingList, Voucher, WifiBroadcast,
};

use super::super::REFRESH_DETAIL_CONCURRENCY;

pub(super) struct IntegrationRefresh {
    pub(super) devices: Vec<Device>,
    pub(super) clients: Vec<Client>,
    pub(super) networks: Vec<Network>,
    pub(super) wifi: Vec<WifiBroadcast>,
    pub(super) policies: Vec<FirewallPolicy>,
    pub(super) zones: Vec<FirewallZone>,
    pub(super) acls: Vec<AclRule>,
    pub(super) dns: Vec<DnsPolicy>,
    pub(super) vouchers: Vec<Voucher>,
    pub(super) sites: Vec<Site>,
    pub(super) traffic_matching_lists: Vec<TrafficMatchingList>,
}

pub(super) async fn fetch(
    integration: Arc<IntegrationClient>,
    site_id: uuid::Uuid,
) -> Result<IntegrationRefresh, CoreError> {
    let page_limit = 200;

    let (devices_res, clients_res, networks_res, wifi_res) = tokio::join!(
        integration.paginate_all(page_limit, |off, lim| {
            integration.list_devices(&site_id, off, lim)
        }),
        integration.paginate_all(page_limit, |off, lim| {
            integration.list_clients(&site_id, off, lim)
        }),
        integration.paginate_all(page_limit, |off, lim| {
            integration.list_networks(&site_id, off, lim)
        }),
        integration.paginate_all(page_limit, |off, lim| {
            integration.list_wifi_broadcasts(&site_id, off, lim)
        }),
    );

    let (policies_res, zones_res, acls_res, dns_res, vouchers_res) = tokio::join!(
        integration.paginate_all(page_limit, |off, lim| {
            integration.list_firewall_policies(&site_id, off, lim)
        }),
        integration.paginate_all(page_limit, |off, lim| {
            integration.list_firewall_zones(&site_id, off, lim)
        }),
        integration.paginate_all(page_limit, |off, lim| {
            integration.list_acl_rules(&site_id, off, lim)
        }),
        integration.paginate_all(page_limit, |off, lim| {
            integration.list_dns_policies(&site_id, off, lim)
        }),
        integration.paginate_all(page_limit, |off, lim| {
            integration.list_vouchers(&site_id, off, lim)
        }),
    );

    let (sites_res, tml_res) = tokio::join!(
        integration.paginate_all(50, |off, lim| integration.list_sites(off, lim)),
        integration.paginate_all(page_limit, |off, lim| {
            integration.list_traffic_matching_lists(&site_id, off, lim)
        }),
    );

    let devices: Vec<Device> = devices_res?.into_iter().map(Device::from).collect();
    let clients: Vec<Client> = clients_res?.into_iter().map(Client::from).collect();
    let network_ids: Vec<uuid::Uuid> = networks_res?
        .into_iter()
        .map(|network| network.id)
        .collect();
    info!(
        network_count = network_ids.len(),
        "fetching network details"
    );
    let networks = fetch_network_details(Arc::clone(&integration), site_id, network_ids).await;
    let wifi: Vec<WifiBroadcast> = wifi_res?.into_iter().map(WifiBroadcast::from).collect();
    let sites: Vec<Site> = sites_res?.into_iter().map(Site::from).collect();
    let traffic_matching_lists: Vec<TrafficMatchingList> = tml_res?
        .into_iter()
        .map(TrafficMatchingList::from)
        .collect();

    let policies = unwrap_or_empty("firewall/policies", policies_res);
    let zones = unwrap_or_empty("firewall/zones", zones_res);
    let acls = unwrap_or_empty("acl/rules", acls_res);
    let dns = unwrap_or_empty_with(
        "dns/policies",
        dns_res,
        crate::convert::dns_policy_from_integration,
    );
    let vouchers = unwrap_or_empty("vouchers", vouchers_res);

    info!(
        device_count = devices.len(),
        "enriching devices with statistics"
    );
    let devices = fetch_device_statistics(integration, site_id, devices).await;

    Ok(IntegrationRefresh {
        devices,
        clients,
        networks,
        wifi,
        policies,
        zones,
        acls,
        dns,
        vouchers,
        sites,
        traffic_matching_lists,
    })
}

async fn fetch_network_details(
    integration: Arc<IntegrationClient>,
    site_id: uuid::Uuid,
    network_ids: Vec<uuid::Uuid>,
) -> Vec<Network> {
    stream::iter(network_ids.into_iter().map(|network_id| {
        let integration = Arc::clone(&integration);
        async move {
            match integration.get_network(&site_id, &network_id).await {
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
}

async fn fetch_device_statistics(
    integration: Arc<IntegrationClient>,
    site_id: uuid::Uuid,
    devices: Vec<Device>,
) -> Vec<Device> {
    stream::iter(devices.into_iter().map(|mut device| {
        let integration = Arc::clone(&integration);
        async move {
            if let EntityId::Uuid(device_uuid) = &device.id {
                match integration
                    .get_device_statistics(&site_id, device_uuid)
                    .await
                {
                    Ok(stats_resp) => {
                        device.stats = crate::convert::device_stats_from_integration(&stats_resp);
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
}

/// Downgrade a failed paginated fetch to an empty `Vec` so one optional
/// endpoint cannot abort the whole refresh. 404s (endpoints missing on older
/// firmware) log at debug; every other error logs at warn.
fn unwrap_or_empty<S, D>(endpoint: &str, result: Result<Vec<S>, crate::error::Error>) -> Vec<D>
where
    D: From<S>,
{
    unwrap_or_empty_with(endpoint, result, |item| Some(D::from(item)))
}

/// Like [`unwrap_or_empty`], for conversions that can reject individual items.
fn unwrap_or_empty_with<S, D>(
    endpoint: &str,
    result: Result<Vec<S>, crate::error::Error>,
    convert: impl Fn(S) -> Option<D>,
) -> Vec<D> {
    match result {
        Ok(items) => items.into_iter().filter_map(convert).collect(),
        Err(ref error) if error.is_not_found() => {
            tracing::debug!("{endpoint}: not available (404), treating as empty");
            Vec::new()
        }
        Err(error) => {
            warn!("{endpoint}: unexpected error {error}, treating as empty");
            Vec::new()
        }
    }
}
