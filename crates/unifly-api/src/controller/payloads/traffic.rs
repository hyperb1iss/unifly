use crate::command::requests::{PortSpec, TrafficFilterSpec};
use crate::core_error::CoreError;

fn parse_port(value: &str) -> Result<u16, CoreError> {
    value
        .parse::<u16>()
        .map_err(|_| CoreError::ValidationFailed {
            message: format!("invalid port number {value:?} (expected 0-65535)"),
        })
}

fn build_port_item_json(p: &str) -> Result<serde_json::Value, CoreError> {
    if p.contains('-') {
        let mut parts = p.splitn(2, '-');
        let start_str = parts.next().unwrap_or("");
        let end_str = parts.next().unwrap_or("");
        let start = parse_port(start_str)?;
        let end = parse_port(end_str)?;
        Ok(serde_json::json!({
            "type": "PORT_NUMBER_RANGE",
            "startPort": start,
            "endPort": end,
        }))
    } else {
        let port = parse_port(p)?;
        Ok(serde_json::json!({ "type": "PORT_NUMBER", "value": port }))
    }
}

fn build_port_filter_json(spec: &PortSpec) -> Result<serde_json::Value, CoreError> {
    match spec {
        PortSpec::Values {
            items,
            match_opposite,
        } => {
            let json_items: Vec<serde_json::Value> = items
                .iter()
                .map(|p| build_port_item_json(p))
                .collect::<Result<_, _>>()?;
            Ok(serde_json::json!({
                "type": "PORTS",
                "items": json_items,
                "matchOpposite": match_opposite,
            }))
        }
        PortSpec::MatchingList {
            list_id,
            match_opposite,
        } => Ok(serde_json::json!({
            "type": "TRAFFIC_MATCHING_LIST",
            "trafficMatchingListId": list_id,
            "matchOpposite": match_opposite,
        })),
    }
}

pub(in super::super) fn build_endpoint_json(
    zone_id: &str,
    filter: Option<&TrafficFilterSpec>,
) -> Result<serde_json::Value, CoreError> {
    let mut obj = serde_json::json!({ "zoneId": zone_id });

    if let Some(spec) = filter {
        let traffic_filter = match spec {
            TrafficFilterSpec::Network {
                network_ids,
                match_opposite,
                ports,
            } => {
                let mut v = serde_json::json!({
                    "type": "NETWORK",
                    "networkFilter": {
                        "networkIds": network_ids,
                        "matchOpposite": match_opposite,
                    }
                });
                if let Some(ports) = ports {
                    v.as_object_mut()
                        .expect("json! produces object")
                        .insert("portFilter".into(), build_port_filter_json(ports)?);
                }
                v
            }
            TrafficFilterSpec::IpAddress {
                addresses,
                match_opposite,
                ports,
            } => {
                let items: Vec<serde_json::Value> = addresses
                    .iter()
                    .map(|addr| {
                        if addr.contains('/') {
                            serde_json::json!({ "type": "SUBNET", "value": addr })
                        } else if addr.contains('-') {
                            let parts: Vec<&str> = addr.splitn(2, '-').collect();
                            serde_json::json!({ "type": "RANGE", "start": parts[0], "stop": parts.get(1).unwrap_or(&"") })
                        } else {
                            serde_json::json!({ "type": "IP_ADDRESS", "value": addr })
                        }
                    })
                    .collect();
                let mut v = serde_json::json!({
                    "type": "IP_ADDRESS",
                    "ipAddressFilter": {
                        "type": "IP_ADDRESSES",
                        "items": items,
                        "matchOpposite": match_opposite,
                    }
                });
                if let Some(ports) = ports {
                    v.as_object_mut()
                        .expect("json! produces object")
                        .insert("portFilter".into(), build_port_filter_json(ports)?);
                }
                v
            }
            TrafficFilterSpec::Port { ports } => serde_json::json!({
                "type": "PORT",
                "portFilter": build_port_filter_json(ports)?,
            }),
        };
        obj.as_object_mut()
            .expect("json! produces object")
            .insert("trafficFilter".into(), traffic_filter);
    }

    Ok(obj)
}

pub(in super::super) fn traffic_matching_list_items(
    entries: &[String],
    raw_items: Option<&[serde_json::Value]>,
) -> Vec<serde_json::Value> {
    raw_items.map_or_else(
        || {
            entries
                .iter()
                .cloned()
                .map(serde_json::Value::String)
                .collect()
        },
        <[serde_json::Value]>::to_vec,
    )
}

#[cfg(test)]
mod tests {
    use super::{build_endpoint_json, traffic_matching_list_items};
    use crate::command::requests::{PortSpec, TrafficFilterSpec};
    use crate::core_error::CoreError;
    use serde_json::json;

    #[test]
    fn traffic_matching_list_items_prefer_raw_payloads() {
        let raw_items = [json!({"type": "PORT_NUMBER", "value": 443})];
        let items = traffic_matching_list_items(&["80".into()], Some(&raw_items));
        assert_eq!(items, vec![json!({"type": "PORT_NUMBER", "value": 443})]);
    }

    #[test]
    fn build_endpoint_json_ip_with_port_nests_port_filter() {
        let spec = TrafficFilterSpec::IpAddress {
            addresses: vec!["10.0.40.10".into()],
            match_opposite: false,
            ports: Some(PortSpec::Values {
                items: vec!["80".into()],
                match_opposite: false,
            }),
        };
        let result = build_endpoint_json("zone-uuid", Some(&spec)).expect("build endpoint json");
        let tf = result.get("trafficFilter").expect("trafficFilter present");
        assert_eq!(tf.get("type").and_then(|v| v.as_str()), Some("IP_ADDRESS"));
        // ipAddressFilter should be present
        let ip_filter = tf.get("ipAddressFilter").expect("ipAddressFilter present");
        assert_eq!(
            ip_filter
                .get("items")
                .and_then(|v| v.as_array())
                .map(Vec::len),
            Some(1)
        );
        // portFilter should be nested alongside ipAddressFilter
        let port_filter = tf.get("portFilter").expect("portFilter present");
        assert_eq!(
            port_filter
                .get("items")
                .and_then(|v| v.as_array())
                .map(Vec::len),
            Some(1)
        );
        let port_item = &port_filter["items"][0];
        assert_eq!(port_item["type"].as_str(), Some("PORT_NUMBER"));
        assert_eq!(port_item["value"].as_u64(), Some(80));
    }

    #[test]
    fn build_endpoint_json_ip_without_port_omits_port_filter() {
        let spec = TrafficFilterSpec::IpAddress {
            addresses: vec!["10.0.0.1".into()],
            match_opposite: false,
            ports: None,
        };
        let result = build_endpoint_json("zone-uuid", Some(&spec)).expect("build endpoint json");
        let tf = result.get("trafficFilter").expect("trafficFilter present");
        assert!(tf.get("portFilter").is_none());
    }

    #[test]
    fn build_port_filter_json_rejects_invalid_port_number() {
        let spec = PortSpec::Values {
            items: vec!["abc".into()],
            match_opposite: false,
        };
        let err = super::build_port_filter_json(&spec).expect_err("invalid port should error");
        assert!(
            matches!(err, CoreError::ValidationFailed { .. }),
            "expected ValidationFailed, got {err:?}",
        );
    }

    #[test]
    fn build_port_filter_json_rejects_invalid_port_range() {
        let spec = PortSpec::Values {
            items: vec!["80-abc".into()],
            match_opposite: false,
        };
        let err = super::build_port_filter_json(&spec).expect_err("invalid range end should error");
        assert!(matches!(err, CoreError::ValidationFailed { .. }));
    }

    /// Full round-trip: a JSONC-style payload using the new tagged PortSpec
    /// shape deserializes into the request, then the payload builder emits
    /// the matching wire JSON. Demonstrates the schema is consistent across
    /// from-file → in-memory → wire.
    #[test]
    fn full_roundtrip_ip_address_with_port_matching_list() {
        use crate::command::requests::CreateFirewallPolicyRequest;

        let mut req: CreateFirewallPolicyRequest = serde_json::from_value(json!({
            "name": "Apple Companion Link",
            "action": "Allow",
            "source_zone_id": "d2864b8e-56fb-4945-b69f-6d424fa5b248",
            "destination_zone_id": "5888bc93-aaae-4242-ae2f-2050d76211fd",
            "destination_filter": {
                "type": "ip_address",
                "addresses": ["10.0.10.2"],
                "ports": {
                    "type": "matching_list",
                    "list_id": "companion-link-ports-uuid"
                }
            }
        }))
        .expect("deserialize");
        req.resolve_filters().expect("resolve");

        let wire = build_endpoint_json(
            "5888bc93-aaae-4242-ae2f-2050d76211fd",
            req.destination_filter.as_ref(),
        )
        .expect("build endpoint json");

        // Print so `cargo test -- --nocapture` shows the wire shape.
        eprintln!(
            "wire payload:\n{}",
            serde_json::to_string_pretty(&wire).expect("serializing test wire payload"),
        );

        assert_eq!(wire["trafficFilter"]["type"].as_str(), Some("IP_ADDRESS"));
        assert_eq!(
            wire["trafficFilter"]["portFilter"]["type"].as_str(),
            Some("TRAFFIC_MATCHING_LIST"),
        );
        assert_eq!(
            wire["trafficFilter"]["portFilter"]["trafficMatchingListId"].as_str(),
            Some("companion-link-ports-uuid"),
        );
    }

    #[test]
    fn build_endpoint_json_ip_with_port_matching_list_emits_traffic_matching_list() {
        let spec = TrafficFilterSpec::IpAddress {
            addresses: vec!["10.0.0.5".into()],
            match_opposite: false,
            ports: Some(PortSpec::MatchingList {
                list_id: "24740a56-9cb9-4890-a5ac-589d30914a55".into(),
                match_opposite: false,
            }),
        };
        let result = build_endpoint_json("zone-uuid", Some(&spec)).expect("build endpoint json");
        let port_filter = result["trafficFilter"]
            .get("portFilter")
            .expect("portFilter present");
        assert_eq!(port_filter["type"].as_str(), Some("TRAFFIC_MATCHING_LIST"));
        assert_eq!(
            port_filter["trafficMatchingListId"].as_str(),
            Some("24740a56-9cb9-4890-a5ac-589d30914a55")
        );
    }
}
