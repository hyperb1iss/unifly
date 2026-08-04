use crate::core_error::CoreError;
use crate::model::DnsPolicyType;

pub(in super::super) fn dns_policy_type_name(policy_type: DnsPolicyType) -> &'static str {
    policy_type.wire_name()
}

fn validation_failed(message: impl Into<String>) -> CoreError {
    CoreError::ValidationFailed {
        message: message.into(),
    }
}

fn dns_domain_value(
    domain: Option<&str>,
    domains: Option<&[String]>,
    fallback: Option<&str>,
) -> Option<String> {
    domain
        .map(str::to_owned)
        .or_else(|| domains.and_then(|values| values.first().cloned()))
        .or_else(|| {
            fallback
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
}

fn insert_string_field(
    fields: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: Option<String>,
) {
    if let Some(value) = value {
        fields.insert(key.into(), serde_json::Value::String(value));
    }
}

fn insert_u16_field(
    fields: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: Option<u16>,
) {
    if let Some(value) = value {
        fields.insert(
            key.into(),
            serde_json::Value::Number(serde_json::Number::from(value)),
        );
    }
}

fn insert_u32_field(
    fields: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: Option<u32>,
) {
    if let Some(value) = value {
        fields.insert(
            key.into(),
            serde_json::Value::Number(serde_json::Number::from(value)),
        );
    }
}

fn ensure_dns_required_string(
    fields: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    policy_type: DnsPolicyType,
) -> Result<(), CoreError> {
    if fields
        .get(key)
        .and_then(serde_json::Value::as_str)
        .is_some()
    {
        Ok(())
    } else {
        Err(validation_failed(format!(
            "{policy_type} DNS policy requires `{key}`"
        )))
    }
}

fn ensure_dns_required_number(
    fields: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    policy_type: DnsPolicyType,
) -> Result<(), CoreError> {
    if fields
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .is_some()
    {
        Ok(())
    } else {
        Err(validation_failed(format!(
            "{policy_type} DNS policy requires `{key}`"
        )))
    }
}

fn validate_dns_policy_fields(
    policy_type: DnsPolicyType,
    fields: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), CoreError> {
    ensure_dns_required_string(fields, "domain", policy_type)?;

    match policy_type {
        DnsPolicyType::ARecord => {
            ensure_dns_required_string(fields, "ipv4Address", policy_type)?;
            ensure_dns_required_number(fields, "ttlSeconds", policy_type)?;
        }
        DnsPolicyType::AaaaRecord => {
            ensure_dns_required_string(fields, "ipv6Address", policy_type)?;
            ensure_dns_required_number(fields, "ttlSeconds", policy_type)?;
        }
        DnsPolicyType::CnameRecord => {
            ensure_dns_required_string(fields, "targetDomain", policy_type)?;
            ensure_dns_required_number(fields, "ttlSeconds", policy_type)?;
        }
        DnsPolicyType::MxRecord => {
            ensure_dns_required_string(fields, "mailServerDomain", policy_type)?;
            ensure_dns_required_number(fields, "priority", policy_type)?;
        }
        DnsPolicyType::TxtRecord => {
            ensure_dns_required_string(fields, "text", policy_type)?;
        }
        DnsPolicyType::SrvRecord => {
            for key in ["serverDomain", "service", "protocol"] {
                ensure_dns_required_string(fields, key, policy_type)?;
            }
            for key in ["port", "priority", "weight"] {
                ensure_dns_required_number(fields, key, policy_type)?;
            }
        }
        DnsPolicyType::ForwardDomain => {
            ensure_dns_required_string(fields, "ipAddress", policy_type)?;
        }
    }

    Ok(())
}

pub(in super::super) fn build_create_dns_policy_fields(
    req: &crate::command::CreateDnsPolicyRequest,
) -> Result<serde_json::Map<String, serde_json::Value>, CoreError> {
    let mut fields = serde_json::Map::new();
    let domain = dns_domain_value(
        req.domain.as_deref(),
        req.domains.as_deref(),
        Some(req.name.as_str()),
    )
    .ok_or_else(|| validation_failed("DNS policy requires `domain`"))?;
    fields.insert("domain".into(), serde_json::Value::String(domain));

    match req.policy_type {
        DnsPolicyType::ARecord => {
            insert_string_field(
                &mut fields,
                "ipv4Address",
                req.ipv4_address.clone().or_else(|| req.value.clone()),
            );
            insert_u32_field(&mut fields, "ttlSeconds", req.ttl_seconds);
        }
        DnsPolicyType::AaaaRecord => {
            insert_string_field(
                &mut fields,
                "ipv6Address",
                req.ipv6_address.clone().or_else(|| req.value.clone()),
            );
            insert_u32_field(&mut fields, "ttlSeconds", req.ttl_seconds);
        }
        DnsPolicyType::CnameRecord => {
            insert_string_field(
                &mut fields,
                "targetDomain",
                req.target_domain.clone().or_else(|| req.value.clone()),
            );
            insert_u32_field(&mut fields, "ttlSeconds", req.ttl_seconds);
        }
        DnsPolicyType::MxRecord => {
            insert_string_field(
                &mut fields,
                "mailServerDomain",
                req.mail_server_domain.clone().or_else(|| req.value.clone()),
            );
            insert_u16_field(&mut fields, "priority", req.priority);
        }
        DnsPolicyType::TxtRecord => {
            insert_string_field(
                &mut fields,
                "text",
                req.text.clone().or_else(|| req.value.clone()),
            );
        }
        DnsPolicyType::SrvRecord => {
            insert_string_field(
                &mut fields,
                "serverDomain",
                req.server_domain.clone().or_else(|| req.value.clone()),
            );
            insert_string_field(&mut fields, "service", req.service.clone());
            insert_string_field(&mut fields, "protocol", req.protocol.clone());
            insert_u16_field(&mut fields, "port", req.port);
            insert_u16_field(&mut fields, "priority", req.priority);
            insert_u16_field(&mut fields, "weight", req.weight);
        }
        DnsPolicyType::ForwardDomain => {
            insert_string_field(
                &mut fields,
                "ipAddress",
                req.ip_address
                    .clone()
                    .or_else(|| req.upstream.clone())
                    .or_else(|| req.value.clone()),
            );
        }
    }

    validate_dns_policy_fields(req.policy_type, &fields)?;
    Ok(fields)
}

pub(in super::super) fn build_update_dns_policy_fields(
    existing: &crate::integration_types::DnsPolicyResponse,
    update: &crate::command::UpdateDnsPolicyRequest,
) -> Result<serde_json::Map<String, serde_json::Value>, CoreError> {
    let policy_type = DnsPolicyType::from_wire(&existing.policy_type).ok_or_else(|| {
        validation_failed(format!(
            "cannot update DNS policy with unrecognized record type `{}`",
            existing.policy_type
        ))
    })?;
    let mut fields: serde_json::Map<String, serde_json::Value> =
        existing.extra.clone().into_iter().collect();

    if let Some(domain) = dns_domain_value(
        update.domain.as_deref(),
        update.domains.as_deref(),
        existing.domain.as_deref(),
    ) {
        fields.insert("domain".into(), serde_json::Value::String(domain));
    }

    match policy_type {
        DnsPolicyType::ARecord => {
            insert_string_field(
                &mut fields,
                "ipv4Address",
                update.ipv4_address.clone().or_else(|| update.value.clone()),
            );
            insert_u32_field(&mut fields, "ttlSeconds", update.ttl_seconds);
        }
        DnsPolicyType::AaaaRecord => {
            insert_string_field(
                &mut fields,
                "ipv6Address",
                update.ipv6_address.clone().or_else(|| update.value.clone()),
            );
            insert_u32_field(&mut fields, "ttlSeconds", update.ttl_seconds);
        }
        DnsPolicyType::CnameRecord => {
            insert_string_field(
                &mut fields,
                "targetDomain",
                update
                    .target_domain
                    .clone()
                    .or_else(|| update.value.clone()),
            );
            insert_u32_field(&mut fields, "ttlSeconds", update.ttl_seconds);
        }
        DnsPolicyType::MxRecord => {
            insert_string_field(
                &mut fields,
                "mailServerDomain",
                update
                    .mail_server_domain
                    .clone()
                    .or_else(|| update.value.clone()),
            );
            insert_u16_field(&mut fields, "priority", update.priority);
        }
        DnsPolicyType::TxtRecord => {
            insert_string_field(
                &mut fields,
                "text",
                update.text.clone().or_else(|| update.value.clone()),
            );
        }
        DnsPolicyType::SrvRecord => {
            insert_string_field(
                &mut fields,
                "serverDomain",
                update
                    .server_domain
                    .clone()
                    .or_else(|| update.value.clone()),
            );
            insert_string_field(&mut fields, "service", update.service.clone());
            insert_string_field(&mut fields, "protocol", update.protocol.clone());
            insert_u16_field(&mut fields, "port", update.port);
            insert_u16_field(&mut fields, "priority", update.priority);
            insert_u16_field(&mut fields, "weight", update.weight);
        }
        DnsPolicyType::ForwardDomain => {
            insert_string_field(
                &mut fields,
                "ipAddress",
                update
                    .ip_address
                    .clone()
                    .or_else(|| update.upstream.clone())
                    .or_else(|| update.value.clone()),
            );
        }
    }

    validate_dns_policy_fields(policy_type, &fields)?;
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::{build_create_dns_policy_fields, dns_policy_type_name};
    use crate::command::CreateDnsPolicyRequest;
    use crate::model::DnsPolicyType;
    use serde_json::json;

    #[test]
    fn dns_policy_type_names_use_wire_record_tokens() {
        let cases = [
            (DnsPolicyType::ARecord, "A_RECORD"),
            (DnsPolicyType::AaaaRecord, "AAAA_RECORD"),
            (DnsPolicyType::CnameRecord, "CNAME_RECORD"),
            (DnsPolicyType::MxRecord, "MX_RECORD"),
            (DnsPolicyType::TxtRecord, "TXT_RECORD"),
            (DnsPolicyType::SrvRecord, "SRV_RECORD"),
            (DnsPolicyType::ForwardDomain, "FORWARD_DOMAIN"),
        ];
        for (policy_type, wire) in cases {
            assert_eq!(dns_policy_type_name(policy_type), wire);
            assert_eq!(
                DnsPolicyType::from_wire(wire),
                Some(policy_type),
                "round-trip for {wire}"
            );
        }
    }

    #[test]
    fn dns_create_fields_use_type_specific_schema_keys() {
        let fields = build_create_dns_policy_fields(&CreateDnsPolicyRequest {
            name: "example.com".into(),
            policy_type: DnsPolicyType::ARecord,
            enabled: true,
            domain: Some("example.com".into()),
            domains: None,
            upstream: None,
            value: Some("192.168.1.10".into()),
            ttl_seconds: Some(600),
            priority: None,
            ipv4_address: None,
            ipv6_address: None,
            target_domain: None,
            mail_server_domain: None,
            text: None,
            ip_address: None,
            server_domain: None,
            service: None,
            protocol: None,
            port: None,
            weight: None,
        })
        .expect("valid DNS fields");

        assert_eq!(fields.get("domain"), Some(&json!("example.com")));
        assert_eq!(fields.get("ipv4Address"), Some(&json!("192.168.1.10")));
        assert_eq!(fields.get("ttlSeconds"), Some(&json!(600)));
        assert!(fields.get("value").is_none());
        assert!(fields.get("ttl").is_none());
    }
}
