pub(crate) fn redact_sensitive_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        if should_redact_field(key) {
                            serde_json::Value::String("***REDACTED***".into())
                        } else {
                            redact_sensitive_value(value)
                        },
                    )
                })
                .collect(),
        ),
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(redact_sensitive_value).collect())
        }
        _ => value.clone(),
    }
}

fn should_redact_field(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "private",
        "password",
        "secret",
        "token",
        "psk",
        "shared_key",
        "certificate",
        "dh_key",
    ]
    .into_iter()
    .any(|needle| key.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::redact_sensitive_value;

    #[test]
    fn redact_sensitive_value_masks_nested_vpn_secrets() {
        let redacted = redact_sensitive_value(&serde_json::json!({
            "enabled": true,
            "public_key": "keep-me",
            "x_private_key": "secret",
            "nested": {
                "psk": "hide-me",
                "certificatePem": "hide-me-too"
            }
        }));

        assert_eq!(
            redacted.get("enabled").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            redacted
                .get("public_key")
                .and_then(serde_json::Value::as_str),
            Some("keep-me")
        );
        assert_eq!(
            redacted
                .get("x_private_key")
                .and_then(serde_json::Value::as_str),
            Some("***REDACTED***")
        );
        assert_eq!(redacted["nested"]["psk"].as_str(), Some("***REDACTED***"));
        assert_eq!(
            redacted["nested"]["certificatePem"].as_str(),
            Some("***REDACTED***")
        );
    }
}
