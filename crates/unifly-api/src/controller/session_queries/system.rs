use crate::controller::Controller;
use crate::controller::support::{convert_health_summaries, require_session};
use crate::core_error::CoreError;
use crate::model::{Alarm, HealthSummary, SysInfo, SystemInfo};

impl Controller {
    pub async fn is_dpi_enabled(&self) -> Result<bool, CoreError> {
        let guard = self.inner.session_client.lock().await;
        let session = require_session(guard.as_ref())?;
        let settings = session.get_site_settings().await?;
        let enabled = settings
            .iter()
            .find(|s| s.get("key").and_then(|v| v.as_str()) == Some("dpi"))
            .and_then(|s| s.get("enabled"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        Ok(enabled)
    }

    pub async fn list_alarms(&self) -> Result<Vec<Alarm>, CoreError> {
        #[cfg(feature = "demo")]
        if let Some(demo) = self.demo_runtime() {
            return Ok(demo.with(|sim| sim.alarms()));
        }
        let guard = self.inner.session_client.lock().await;
        let session = require_session(guard.as_ref())?;
        let raw = session.list_alarms().await?;
        Ok(raw.into_iter().map(Alarm::from).collect())
    }

    pub async fn get_system_info(&self) -> Result<SystemInfo, CoreError> {
        #[cfg(feature = "demo")]
        if let Some(demo) = self.demo_runtime() {
            return Ok(demo.with(|sim| sim.system_info()));
        }
        {
            let guard = self.inner.integration_client.lock().await;
            if let Some(ic) = guard.as_ref() {
                let info = ic.get_info().await?;
                let fields = &info.fields;
                return Ok(SystemInfo {
                    controller_name: fields
                        .get("applicationName")
                        .or_else(|| fields.get("name"))
                        .and_then(|value| value.as_str())
                        .map(String::from),
                    version: fields
                        .get("applicationVersion")
                        .or_else(|| fields.get("version"))
                        .and_then(|value| value.as_str())
                        .unwrap_or("unknown")
                        .to_owned(),
                    build: fields
                        .get("build")
                        .and_then(|value| value.as_str())
                        .map(String::from),
                    hostname: fields
                        .get("hostname")
                        .and_then(|value| value.as_str())
                        .map(String::from),
                    ip: None,
                    uptime_secs: fields.get("uptime").and_then(serde_json::Value::as_u64),
                    update_available: fields
                        .get("isUpdateAvailable")
                        .or_else(|| fields.get("update_available"))
                        .and_then(serde_json::Value::as_bool),
                });
            }
        }

        let guard = self.inner.session_client.lock().await;
        let session = require_session(guard.as_ref())?;
        let raw = session.get_sysinfo().await?;
        Ok(SystemInfo {
            controller_name: raw
                .get("controller_name")
                .or_else(|| raw.get("name"))
                .and_then(|value| value.as_str())
                .map(String::from),
            version: raw
                .get("version")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown")
                .to_owned(),
            build: raw
                .get("build")
                .and_then(|value| value.as_str())
                .map(String::from),
            hostname: raw
                .get("hostname")
                .and_then(|value| value.as_str())
                .map(String::from),
            ip: raw
                .get("ip_addrs")
                .and_then(|value| value.as_array())
                .and_then(|values| values.first())
                .and_then(|value| value.as_str())
                .and_then(|value| value.parse().ok()),
            uptime_secs: raw.get("uptime").and_then(serde_json::Value::as_u64),
            update_available: raw
                .get("update_available")
                .and_then(serde_json::Value::as_bool),
        })
    }

    pub async fn get_site_health(&self) -> Result<Vec<HealthSummary>, CoreError> {
        #[cfg(feature = "demo")]
        if let Some(demo) = self.demo_runtime() {
            return Ok(demo.with(|sim| sim.health()));
        }
        let guard = self.inner.session_client.lock().await;
        let session = require_session(guard.as_ref())?;
        let raw = session.get_health().await?;
        Ok(convert_health_summaries(raw))
    }

    pub async fn get_sysinfo(&self) -> Result<SysInfo, CoreError> {
        let guard = self.inner.session_client.lock().await;
        let session = require_session(guard.as_ref())?;
        let raw = session.get_sysinfo().await?;
        Ok(SysInfo {
            timezone: raw
                .get("timezone")
                .and_then(|value| value.as_str())
                .map(String::from),
            autobackup: raw.get("autobackup").and_then(serde_json::Value::as_bool),
            hostname: raw
                .get("hostname")
                .and_then(|value| value.as_str())
                .map(String::from),
            ip_addrs: raw
                .get("ip_addrs")
                .and_then(|value| value.as_array())
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|value| value.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            live_chat: raw
                .get("live_chat")
                .and_then(|value| value.as_str())
                .map(String::from),
            #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
            data_retention_days: raw
                .get("data_retention_days")
                .and_then(serde_json::Value::as_u64)
                .map(|value| value as u32),
            extra: raw,
        })
    }
}
