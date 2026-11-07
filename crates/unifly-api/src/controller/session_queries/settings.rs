use crate::controller::Controller;
use crate::controller::support::require_session;
use crate::core_error::CoreError;
use crate::model::VpnSetting;

use super::common::redact_sensitive_value;

const VPN_SETTING_KEYS: &[&str] = &[
    "teleport",
    "magic_site_to_site_vpn",
    "openvpn",
    "peer_to_peer",
];

impl Controller {
    pub async fn list_vpn_settings(&self) -> Result<Vec<VpnSetting>, CoreError> {
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        let raw = session.get_site_settings().await?;
        let mut settings = raw
            .iter()
            .filter_map(vpn_setting_from_raw)
            .collect::<Vec<_>>();
        settings.sort_by(|left, right| left.key.cmp(&right.key));
        Ok(settings)
    }

    pub async fn get_vpn_setting(&self, key: &str) -> Result<VpnSetting, CoreError> {
        self.list_vpn_settings()
            .await?
            .into_iter()
            .find(|setting| setting.key == key)
            .ok_or_else(|| CoreError::NotFound {
                entity_type: "vpn setting".into(),
                identifier: key.into(),
            })
    }

    pub async fn update_vpn_setting(
        &self,
        key: &str,
        body: &serde_json::Value,
    ) -> Result<VpnSetting, CoreError> {
        if !VPN_SETTING_KEYS.contains(&key) {
            return Err(CoreError::NotFound {
                entity_type: "vpn setting".into(),
                identifier: key.into(),
            });
        }

        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        session.set_site_setting(key, body).await?;

        self.get_vpn_setting(key).await
    }

    pub async fn get_all_site_settings(&self) -> Result<Vec<serde_json::Value>, CoreError> {
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        Ok(session.get_site_settings().await?)
    }

    pub async fn get_site_setting(&self, key: &str) -> Result<serde_json::Value, CoreError> {
        self.get_all_site_settings()
            .await?
            .into_iter()
            .find(|s| s.get("key").and_then(|v| v.as_str()) == Some(key))
            .ok_or_else(|| CoreError::NotFound {
                entity_type: "setting".into(),
                identifier: key.into(),
            })
    }

    pub async fn update_site_setting(
        &self,
        key: &str,
        body: &serde_json::Value,
    ) -> Result<(), CoreError> {
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        session.set_site_setting(key, body).await?;
        Ok(())
    }
}

fn vpn_setting_from_raw(raw: &serde_json::Value) -> Option<VpnSetting> {
    let object = raw.as_object()?;
    let key = object.get("key")?.as_str()?;
    if !VPN_SETTING_KEYS.contains(&key) {
        return None;
    }

    let mut fields = object.clone();
    fields.remove("_id");
    fields.remove("key");
    fields.remove("site_id");
    let fields = redact_sensitive_value(&serde_json::Value::Object(fields))
        .as_object()
        .cloned()
        .unwrap_or_default();

    Some(VpnSetting {
        key: key.to_owned(),
        enabled: fields.get("enabled").and_then(serde_json::Value::as_bool),
        fields,
    })
}

#[cfg(test)]
mod tests {
    use super::vpn_setting_from_raw;

    #[test]
    fn vpn_setting_from_raw_filters_to_known_keys() {
        let raw = serde_json::json!({
            "key": "teleport",
            "enabled": true,
            "_id": "abc123",
            "site_id": "default",
        });
        let setting = vpn_setting_from_raw(&raw).expect("teleport should be recognized");

        assert_eq!(setting.key, "teleport");
        assert_eq!(setting.enabled, Some(true));
        assert!(!setting.fields.contains_key("_id"));
        assert!(!setting.fields.contains_key("site_id"));
    }
}
