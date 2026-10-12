use crate::integration_types;
use crate::model::common::DataSource;
use crate::model::entity_id::EntityId;
use crate::model::firewall::{NatPolicy, NatType};

use super::helpers::origin_from_metadata;

impl From<integration_types::NatPolicyResponse> for NatPolicy {
    fn from(r: integration_types::NatPolicyResponse) -> Self {
        let nat_type = match r.nat_type.as_str() {
            "MASQUERADE" => NatType::Masquerade,
            "SOURCE_NAT" => NatType::Source,
            _ => NatType::Destination,
        };

        let src_address = r
            .source
            .as_ref()
            .and_then(|s| s.get("address"))
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned);
        let src_port = r
            .source
            .as_ref()
            .and_then(|s| s.get("port"))
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned);
        let dst_address = r
            .destination
            .as_ref()
            .and_then(|d| d.get("address"))
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned);
        let dst_port = r
            .destination
            .as_ref()
            .and_then(|d| d.get("port"))
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned);

        NatPolicy {
            id: EntityId::Uuid(r.id),
            name: r.name,
            description: r.description,
            enabled: r.enabled,
            nat_type,
            interface_id: r.interface_id.map(EntityId::Uuid),
            protocol: r.protocol,
            src_address,
            src_port,
            dst_address,
            dst_port,
            translated_address: r.translated_address,
            translated_port: r.translated_port,
            rule_index: None,
            origin: r.metadata.as_ref().and_then(origin_from_metadata),
            data_source: DataSource::IntegrationApi,
        }
    }
}

pub fn nat_policy_from_v2(v: &serde_json::Value) -> Option<NatPolicy> {
    let id_str = v.get("_id").and_then(|v| v.as_str())?;
    let nat_type_str = v.get("type").and_then(|v| v.as_str()).unwrap_or("DNAT");
    let nat_type = match nat_type_str {
        "MASQUERADE" => NatType::Masquerade,
        "SNAT" => NatType::Source,
        _ => NatType::Destination,
    };

    let filter_addr = |filter: Option<&serde_json::Value>| -> Option<String> {
        filter
            .and_then(|f| f.get("address"))
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned)
    };
    let filter_port = |filter: Option<&serde_json::Value>| -> Option<String> {
        filter
            .and_then(|f| f.get("port"))
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned)
    };

    let src_filter = v.get("source_filter");
    let dst_filter = v.get("destination_filter");

    Some(NatPolicy {
        id: EntityId::from(id_str.to_owned()),
        name: v
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned(),
        description: None,
        enabled: v
            .get("enabled")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        nat_type,
        interface_id: v
            .get("in_interface")
            .or_else(|| v.get("out_interface"))
            .and_then(|v| v.as_str())
            .map(|s| EntityId::from(s.to_owned())),
        protocol: v
            .get("protocol")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned),
        src_address: filter_addr(src_filter),
        src_port: filter_port(src_filter),
        dst_address: filter_addr(dst_filter),
        dst_port: filter_port(dst_filter),
        translated_address: v
            .get("ip_address")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned),
        translated_port: v
            .get("port")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned),
        rule_index: v.get("rule_index").and_then(serde_json::Value::as_u64),
        origin: None,
        data_source: DataSource::SessionApi,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::nat_policy_from_v2;

    #[test]
    fn v2_nat_rule_keeps_its_rule_index() {
        let rule = nat_policy_from_v2(&json!({
            "_id": "nat-7",
            "type": "DNAT",
            "description": "web",
            "enabled": true,
            "rule_index": 7
        }))
        .expect("v2 NAT rule should parse");
        assert_eq!(rule.rule_index, Some(7));

        let unindexed = nat_policy_from_v2(&json!({"_id": "nat-8"})).expect("should parse");
        assert_eq!(unindexed.rule_index, None);
    }
}
