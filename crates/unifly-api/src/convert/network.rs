use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};

use serde_json::Value;

use crate::integration_types;
use crate::model::common::DataSource;
use crate::model::entity_id::EntityId;
use crate::model::network::{DhcpConfig, Ipv6Mode, Network, NetworkManagement};

use super::helpers::map_origin;

fn net_field<'a>(
    extra: &'a HashMap<String, Value>,
    metadata: &'a Value,
    key: &str,
) -> Option<&'a Value> {
    extra.get(key).or_else(|| metadata.get(key))
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn parse_network_fields(
    id: uuid::Uuid,
    name: String,
    enabled: bool,
    management_str: &str,
    vlan_id: i32,
    is_default: bool,
    metadata: &Value,
    extra: &HashMap<String, Value>,
) -> Network {
    // ── Feature flags ───────────────────────────────────────────
    let isolation_enabled = net_field(extra, metadata, "isolationEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let internet_access_enabled = net_field(extra, metadata, "internetAccessEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let mdns_forwarding_enabled = net_field(extra, metadata, "mdnsForwardingEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let cellular_backup_enabled = net_field(extra, metadata, "cellularBackupEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    // ── Firewall zone ───────────────────────────────────────────
    let firewall_zone_id = net_field(extra, metadata, "zoneId")
        .and_then(Value::as_str)
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .map(EntityId::Uuid);

    // ── IPv4 configuration ──────────────────────────────────────
    let ipv4 = net_field(extra, metadata, "ipv4Configuration");

    let gateway_ip: Option<Ipv4Addr> = ipv4
        .and_then(|v| v.get("hostIpAddress").or_else(|| v.get("host")))
        .and_then(Value::as_str)
        .and_then(|s| s.parse().ok());

    let subnet = ipv4.and_then(|v| {
        let host = v.get("hostIpAddress").or_else(|| v.get("host"))?.as_str()?;
        let prefix = v
            .get("prefixLength")
            .or_else(|| v.get("prefix"))?
            .as_u64()?;
        Some(format!("{host}/{prefix}"))
    });

    // ── DHCP ────────────────────────────────────────────────────
    let dhcp = ipv4.and_then(|v| {
        if let Some(dhcp_cfg) = v.get("dhcpConfiguration") {
            let mode = dhcp_cfg.get("mode").and_then(Value::as_str).unwrap_or("");
            let dhcp_enabled = mode == "SERVER";
            // The spec's `IP address range` has exactly `start` and `stop`.
            let range = dhcp_cfg.get("ipAddressRange");
            let range_start = range
                .and_then(|r| r.get("start"))
                .and_then(Value::as_str)
                .and_then(|s| s.parse().ok());
            let range_stop = range
                .and_then(|r| r.get("stop"))
                .and_then(Value::as_str)
                .and_then(|s| s.parse().ok());
            let lease_time_secs = dhcp_cfg.get("leaseTimeSeconds").and_then(Value::as_u64);
            let dns_servers = dhcp_cfg
                .get("dnsServerIpAddressesOverride")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str()?.parse::<IpAddr>().ok())
                        .collect()
                })
                .unwrap_or_default();
            return Some(DhcpConfig {
                enabled: dhcp_enabled,
                range_start,
                range_stop,
                lease_time_secs,
                dns_servers,
                gateway: gateway_ip,
            });
        }

        let server = v.get("dhcp")?.get("server")?;
        let dhcp_enabled = server
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let range_start = server
            .get("rangeStart")
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok());
        let range_stop = server
            .get("rangeStop")
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok());
        let lease_time_secs = server.get("leaseTimeSec").and_then(Value::as_u64);
        let dns_servers = server
            .get("dnsOverride")
            .and_then(|d| d.get("servers"))
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str()?.parse::<IpAddr>().ok())
                    .collect()
            })
            .unwrap_or_default();
        let gateway = server
            .get("gateway")
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok())
            .or(gateway_ip);
        Some(DhcpConfig {
            enabled: dhcp_enabled,
            range_start,
            range_stop,
            lease_time_secs,
            dns_servers,
            gateway,
        })
    });

    // ── PXE / NTP / TFTP ────────────────────────────────────────
    let pxe_enabled = ipv4
        .and_then(|v| v.get("pxe"))
        .and_then(|v| v.get("enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let ntp_server = ipv4
        .and_then(|v| v.get("ntp"))
        .and_then(|v| v.get("server"))
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<IpAddr>().ok());
    let tftp_server = ipv4
        .and_then(|v| v.get("tftp"))
        .and_then(|v| v.get("server"))
        .and_then(Value::as_str)
        .map(String::from);

    // ── IPv6 ────────────────────────────────────────────────────
    let ipv6 = net_field(extra, metadata, "ipv6Configuration");
    let ipv6_enabled = ipv6.is_some();
    let ipv6_mode = ipv6
        .and_then(|v| v.get("interfaceType").or_else(|| v.get("type")))
        .and_then(Value::as_str)
        .and_then(|s| match s {
            "PREFIX_DELEGATION" => Some(Ipv6Mode::PrefixDelegation),
            "STATIC" => Some(Ipv6Mode::Static),
            _ => None,
        });
    let slaac_enabled = ipv6
        .and_then(|v| {
            v.get("clientAddressAssignment")
                .and_then(|ca| ca.get("slaacEnabled"))
                .and_then(Value::as_bool)
                .or_else(|| {
                    v.get("slaac")
                        .and_then(|s| s.get("enabled"))
                        .and_then(Value::as_bool)
                })
        })
        .unwrap_or(false);
    let dhcpv6_enabled = ipv6
        .and_then(|v| {
            v.get("clientAddressAssignment")
                .and_then(|ca| ca.get("dhcpv6Enabled"))
                .and_then(Value::as_bool)
                .or_else(|| {
                    v.get("dhcpv6")
                        .and_then(|d| d.get("enabled"))
                        .and_then(Value::as_bool)
                })
        })
        .unwrap_or(false);
    let ipv6_prefix = ipv6.and_then(|v| {
        v.get("additionalHostIpSubnets")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_str)
            .map(String::from)
            .or_else(|| v.get("prefix").and_then(Value::as_str).map(String::from))
    });

    // ── Management type inference ───────────────────────────────
    let has_ipv4_config = ipv4.is_some();
    let has_device_id = extra.contains_key("deviceId");
    let management = if has_ipv4_config && !has_device_id {
        Some(NetworkManagement::Gateway)
    } else if has_device_id {
        Some(NetworkManagement::Switch)
    } else if has_ipv4_config {
        Some(NetworkManagement::Gateway)
    } else {
        None
    };

    Network {
        id: EntityId::Uuid(id),
        name,
        enabled,
        management,
        purpose: None,
        is_default,
        #[allow(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        vlan_id: Some(vlan_id as u16),
        subnet,
        gateway_ip,
        dhcp,
        ipv6_enabled,
        ipv6_mode,
        ipv6_prefix,
        dhcpv6_enabled,
        slaac_enabled,
        ntp_server,
        pxe_enabled,
        tftp_server,
        firewall_zone_id,
        isolation_enabled,
        internet_access_enabled,
        mdns_forwarding_enabled,
        cellular_backup_enabled,
        origin: map_origin(management_str),
        source: DataSource::IntegrationApi,
    }
}

impl From<integration_types::NetworkResponse> for Network {
    fn from(n: integration_types::NetworkResponse) -> Self {
        parse_network_fields(
            n.id,
            n.name,
            n.enabled,
            &n.management,
            n.vlan_id,
            n.default,
            &n.metadata,
            &n.extra,
        )
    }
}

impl From<integration_types::NetworkDetailsResponse> for Network {
    fn from(n: integration_types::NetworkDetailsResponse) -> Self {
        parse_network_fields(
            n.id,
            n.name,
            n.enabled,
            &n.management,
            n.vlan_id,
            n.default,
            &n.metadata,
            &n.extra,
        )
    }
}
