use std::collections::HashMap;

use serde_json::Value;

use crate::integration_types;
use crate::model::common::DataSource;
use crate::model::dns::{DnsPolicy, DnsPolicyType};
use crate::model::entity_id::EntityId;

use super::helpers::origin_from_metadata;

fn dns_value_from_extra(policy_type: DnsPolicyType, extra: &HashMap<String, Value>) -> String {
    match policy_type {
        DnsPolicyType::ARecord => extra
            .get("ipv4Address")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        DnsPolicyType::AaaaRecord => extra
            .get("ipv6Address")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        DnsPolicyType::CnameRecord => extra
            .get("targetDomain")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        DnsPolicyType::MxRecord => extra
            .get("mailServerDomain")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        DnsPolicyType::TxtRecord => extra
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        DnsPolicyType::SrvRecord => {
            let server = extra
                .get("serverDomain")
                .and_then(Value::as_str)
                .unwrap_or("");
            let service = extra.get("service").and_then(Value::as_str).unwrap_or("");
            let protocol = extra.get("protocol").and_then(Value::as_str).unwrap_or("");
            let port = extra.get("port").and_then(Value::as_u64);
            let priority = extra.get("priority").and_then(Value::as_u64);
            let weight = extra.get("weight").and_then(Value::as_u64);

            let mut parts = Vec::new();
            if !server.is_empty() {
                parts.push(server.to_owned());
            }
            if !service.is_empty() || !protocol.is_empty() {
                parts.push(format!("service={service}{protocol}"));
            }
            if let Some(port) = port {
                parts.push(format!("port={port}"));
            }
            if let Some(priority) = priority {
                parts.push(format!("priority={priority}"));
            }
            if let Some(weight) = weight {
                parts.push(format!("weight={weight}"));
            }
            parts.join(" ")
        }
        DnsPolicyType::ForwardDomain => extra
            .get("ipAddress")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    }
}

pub(crate) fn dns_policy_from_integration(
    d: integration_types::DnsPolicyResponse,
) -> Option<DnsPolicy> {
    let Some(policy_type) = DnsPolicyType::from_wire(&d.policy_type) else {
        tracing::warn!(
            policy_type = %d.policy_type,
            id = %d.id,
            "skipping DNS policy with unrecognized record type"
        );
        return None;
    };

    Some(DnsPolicy {
        id: EntityId::Uuid(d.id),
        policy_type,
        domain: d.domain.unwrap_or_default(),
        value: dns_value_from_extra(policy_type, &d.extra),
        #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
        ttl_seconds: d
            .extra
            .get("ttlSeconds")
            .and_then(serde_json::Value::as_u64)
            .map(|t| t as u32),
        origin: origin_from_metadata(&d.metadata),
        source: DataSource::IntegrationApi,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use serde_json::json;

    fn response(
        policy_type: &str,
        extra: HashMap<String, Value>,
    ) -> integration_types::DnsPolicyResponse {
        integration_types::DnsPolicyResponse {
            id: uuid::Uuid::nil(),
            policy_type: policy_type.into(),
            enabled: true,
            domain: Some("example.com".into()),
            metadata: json!({"origin": "USER"}),
            extra,
        }
    }

    #[test]
    fn integration_dns_policy_uses_type_specific_fields() {
        let dns = dns_policy_from_integration(response(
            "A_RECORD",
            HashMap::from([
                ("ipv4Address".into(), json!("192.168.1.10")),
                ("ttlSeconds".into(), json!(600)),
            ]),
        ))
        .expect("known record type");

        assert_eq!(dns.policy_type, DnsPolicyType::ARecord);
        assert_eq!(dns.value, "192.168.1.10");
        assert_eq!(dns.ttl_seconds, Some(600));
    }

    #[test]
    fn integration_dns_policy_extracts_value_for_every_record_type() {
        let cases: [(&str, DnsPolicyType, HashMap<String, Value>, &str); 7] = [
            (
                "A_RECORD",
                DnsPolicyType::ARecord,
                HashMap::from([("ipv4Address".into(), json!("10.0.1.1"))]),
                "10.0.1.1",
            ),
            (
                "AAAA_RECORD",
                DnsPolicyType::AaaaRecord,
                HashMap::from([("ipv6Address".into(), json!("fd00::1"))]),
                "fd00::1",
            ),
            (
                "CNAME_RECORD",
                DnsPolicyType::CnameRecord,
                HashMap::from([("targetDomain".into(), json!("target.example.com"))]),
                "target.example.com",
            ),
            (
                "MX_RECORD",
                DnsPolicyType::MxRecord,
                HashMap::from([("mailServerDomain".into(), json!("mail.example.com"))]),
                "mail.example.com",
            ),
            (
                "TXT_RECORD",
                DnsPolicyType::TxtRecord,
                HashMap::from([("text".into(), json!("v=spf1 -all"))]),
                "v=spf1 -all",
            ),
            (
                "SRV_RECORD",
                DnsPolicyType::SrvRecord,
                HashMap::from([
                    ("serverDomain".into(), json!("sip.example.com")),
                    ("port".into(), json!(5060)),
                ]),
                "sip.example.com port=5060",
            ),
            (
                "FORWARD_DOMAIN",
                DnsPolicyType::ForwardDomain,
                HashMap::from([("ipAddress".into(), json!("1.1.1.1"))]),
                "1.1.1.1",
            ),
        ];

        for (wire, expected_type, extra, expected_value) in cases {
            let dns = dns_policy_from_integration(response(wire, extra))
                .unwrap_or_else(|| panic!("{wire} should convert"));
            assert_eq!(dns.policy_type, expected_type, "type for {wire}");
            assert_eq!(dns.value, expected_value, "value for {wire}");
        }
    }

    #[test]
    fn integration_dns_policy_accepts_legacy_short_type_names() {
        let dns = dns_policy_from_integration(response(
            "A",
            HashMap::from([("ipv4Address".into(), json!("10.0.1.1"))]),
        ))
        .expect("legacy short name");
        assert_eq!(dns.policy_type, DnsPolicyType::ARecord);
        assert_eq!(dns.value, "10.0.1.1");
    }

    #[test]
    fn integration_dns_policy_skips_unknown_record_types() {
        assert!(dns_policy_from_integration(response("PTR_RECORD", HashMap::new())).is_none());
    }
}
