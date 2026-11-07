use std::collections::HashMap;

use serde_json::{Value, json};

use crate::command::{CreateNetworkRequest, DhcpUpdate, UpdateNetworkRequest};
use crate::core_error::CoreError;
use crate::integration_types::NetworkCreateUpdate;
use crate::model::{EntityId, NetworkManagement, NetworkPurpose};

use super::{apply_ipv4_update, parse_ipv4_cidr};

pub(in crate::controller) fn build_create_network_payload(
    request: CreateNetworkRequest,
) -> Result<NetworkCreateUpdate, CoreError> {
    if !request.dhcp_enabled
        && (request.dhcp_range_start.is_some()
            || request.dhcp_range_stop.is_some()
            || request.dhcp_lease_time.is_some()
            || request.dns_servers.is_some())
    {
        return Err(CoreError::ValidationFailed {
            message: "DHCP server settings require dhcp_enabled to be true (--dhcp)".into(),
        });
    }
    let management = request.management.unwrap_or_else(|| {
        if matches!(request.purpose, Some(NetworkPurpose::VlanOnly)) {
            NetworkManagement::Unmanaged
        } else if request.purpose.is_some() || request.subnet.is_some() || request.dhcp_enabled {
            NetworkManagement::Gateway
        } else {
            NetworkManagement::Unmanaged
        }
    });
    let management = match management {
        NetworkManagement::Gateway => "GATEWAY",
        NetworkManagement::Switch => "SWITCH",
        NetworkManagement::Unmanaged => "UNMANAGED",
    };
    let mut extra = HashMap::new();
    if management == "SWITCH" {
        let Some(EntityId::Uuid(device_id)) = request.device_id else {
            return Err(CoreError::ValidationFailed {
                message: "switch-managed network creation requires device_id with the L3 switch Integration UUID".into(),
            });
        };
        extra.insert("deviceId".into(), json!(device_id));
    } else if request.device_id.is_some() {
        return Err(CoreError::ValidationFailed {
            message: "device_id is only supported for switch-managed networks".into(),
        });
    }
    if management == "UNMANAGED" && (request.subnet.is_some() || request.dhcp_enabled) {
        return Err(CoreError::ValidationFailed {
            message: "IPv4 and DHCP server settings require a gateway- or switch-managed network"
                .into(),
        });
    }
    if management != "UNMANAGED" {
        let cidr = request
            .subnet
            .as_deref()
            .ok_or_else(|| CoreError::ValidationFailed {
                message: "gateway- and switch-managed network creation requires an IPv4 subnet"
                    .into(),
            })?;
        let (host, prefix) = parse_ipv4_cidr(cidr)?;
        extra.insert("isolationEnabled".into(), json!(request.isolation_enabled));
        extra.insert("cellularBackupEnabled".into(), Value::Bool(false));
        extra.insert(
            "ipv4Configuration".into(),
            json!({
                "hostIpAddress": host.to_string(), "prefixLength": prefix,
                "autoScaleEnabled": false,
                "dhcpConfiguration": request.dhcp_lease_time.map(|lease| json!({"leaseTimeSeconds": lease})),
            }),
        );
    }
    if management == "GATEWAY" {
        extra.insert(
            "internetAccessEnabled".into(),
            json!(request.internet_access_enabled),
        );
        extra.insert("mdnsForwardingEnabled".into(), Value::Bool(false));
    }
    if let Some(zone) = request.firewall_zone_id {
        if management != "GATEWAY" {
            return Err(CoreError::ValidationFailed {
                message: "firewall_zone_id is only supported for gateway-managed networks".into(),
            });
        }
        extra.insert("zoneId".into(), Value::String(zone));
    }
    let update = UpdateNetworkRequest {
        dhcp_enabled: Some(request.dhcp_enabled),
        dhcp: request.dhcp_enabled.then_some(DhcpUpdate {
            range_start: request.dhcp_range_start,
            range_stop: request.dhcp_range_stop,
            dns_servers: request.dns_servers,
        }),
        ..Default::default()
    };
    apply_ipv4_update(&mut extra, management, false, &update)?;
    Ok(NetworkCreateUpdate {
        name: request.name,
        enabled: request.enabled,
        management: management.into(),
        vlan_id: request.vlan_id.map_or(1, i32::from),
        dhcp_guarding: None,
        extra,
    })
}
