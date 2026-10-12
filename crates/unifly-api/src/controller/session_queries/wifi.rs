use crate::controller::Controller;
use crate::controller::support::require_session;
use crate::core_error::CoreError;
use crate::session::models::{ChannelAvailability, RogueAp};

impl Controller {
    pub async fn list_rogue_aps(
        &self,
        within_secs: Option<i64>,
    ) -> Result<Vec<RogueAp>, CoreError> {
        #[cfg(feature = "demo")]
        if let Some(demo) = self.demo_runtime() {
            return Ok(demo.with(|sim| sim.rogue_aps(within_secs)));
        }
        let guard = self.inner.session_client.lock().await;
        let session = require_session(guard.as_ref())?;
        Ok(session.list_rogue_aps(within_secs).await?)
    }

    pub async fn list_channels(&self) -> Result<Vec<ChannelAvailability>, CoreError> {
        #[cfg(feature = "demo")]
        if self.is_demo() {
            return Ok(crate::demo::sim::Simulation::channels());
        }
        let guard = self.inner.session_client.lock().await;
        let session = require_session(guard.as_ref())?;
        Ok(session.list_channels().await?)
    }

    pub async fn get_client_roams(
        &self,
        mac: &str,
        limit: Option<u32>,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        #[cfg(feature = "demo")]
        if let Some(demo) = self.demo_runtime() {
            return demo.with(|sim| sim.client_roams(mac, limit));
        }
        let guard = self.inner.session_client.lock().await;
        let session = require_session(guard.as_ref())?;
        Ok(session.get_client_roams(mac, limit).await?)
    }

    pub async fn get_client_wifi_experience(
        &self,
        client_ip: &str,
    ) -> Result<serde_json::Value, CoreError> {
        #[cfg(feature = "demo")]
        if let Some(demo) = self.demo_runtime() {
            return demo.with(|sim| sim.client_wifi_experience(client_ip));
        }
        let guard = self.inner.session_client.lock().await;
        let session = require_session(guard.as_ref())?;
        Ok(session.get_client_wifi_experience(client_ip).await?)
    }
}
