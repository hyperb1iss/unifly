use crate::controller::Controller;
use crate::controller::support::require_session;
use crate::core_error::CoreError;

impl Controller {
    pub async fn get_site_stats(
        &self,
        interval: &str,
        start: Option<i64>,
        end: Option<i64>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        #[cfg(feature = "demo")]
        if let Some(demo) = self.demo_runtime() {
            return Ok(demo.with(|sim| sim.site_stats(interval, start, end, chrono::Utc::now())));
        }
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        Ok(session.get_site_stats(interval, start, end, attrs).await?)
    }

    pub async fn get_device_stats(
        &self,
        interval: &str,
        macs: Option<&[String]>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        self.get_device_stats_in_range(interval, None, None, macs, attrs)
            .await
    }

    /// Fetch device statistics with optional Unix millisecond bounds.
    pub async fn get_device_stats_in_range(
        &self,
        interval: &str,
        start: Option<i64>,
        end: Option<i64>,
        macs: Option<&[String]>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        Ok(session
            .get_device_stats_in_range(interval, start, end, macs, attrs)
            .await?)
    }

    pub async fn get_client_stats(
        &self,
        interval: &str,
        macs: Option<&[String]>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        self.get_client_stats_in_range(interval, None, None, macs, attrs)
            .await
    }

    /// Fetch client statistics with optional Unix millisecond bounds.
    pub async fn get_client_stats_in_range(
        &self,
        interval: &str,
        start: Option<i64>,
        end: Option<i64>,
        macs: Option<&[String]>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        Ok(session
            .get_client_stats_in_range(interval, start, end, macs, attrs)
            .await?)
    }

    pub async fn get_gateway_stats(
        &self,
        interval: &str,
        start: Option<i64>,
        end: Option<i64>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        #[cfg(feature = "demo")]
        if let Some(demo) = self.demo_runtime() {
            return Ok(demo.with(|sim| sim.gateway_stats(interval, start, end, chrono::Utc::now())));
        }
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        Ok(session
            .get_gateway_stats(interval, start, end, attrs)
            .await?)
    }

    pub async fn get_dpi_stats(
        &self,
        group_by: &str,
        macs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        #[cfg(feature = "demo")]
        if let Some(demo) = self.demo_runtime() {
            return Ok(demo.with(|sim| sim.dpi_stats(group_by)));
        }
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        Ok(session.get_dpi_stats(group_by, macs).await?)
    }
}
