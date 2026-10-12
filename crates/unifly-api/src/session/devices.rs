// Session API device endpoints
//
// Device management via stat/device (read) and cmd/devmgr (commands).
// Covers listing, adoption, restart, firmware upgrade, and LED locate.

use serde_json::json;
use tracing::debug;

use crate::error::Error;
use crate::session::client::SessionClient;
use crate::session::models::SessionDevice;

impl SessionClient {
    /// List all devices with full statistics.
    ///
    /// `GET /api/s/{site}/stat/device`
    pub async fn list_devices(&self) -> Result<Vec<SessionDevice>, Error> {
        let url = self.site_url("stat/device");
        debug!("listing devices");
        self.get(url).await
    }

    /// Get a single device by MAC address.
    ///
    /// Filters the device list by MAC. Returns `None` if no device matches.
    pub async fn get_device(&self, mac: &str) -> Result<Option<SessionDevice>, Error> {
        let url = self.site_url("stat/device");
        let body = json!({ "macs": [mac.to_lowercase()] });
        let devices: Vec<SessionDevice> = self.post(url, &body).await?;
        Ok(devices.into_iter().next())
    }

    /// Get a single device as the untyped session record.
    ///
    /// Used by the switch-port surface, which must round-trip
    /// `port_overrides` and read arbitrary `port_table` columns. Everything
    /// else should prefer the typed [`get_device`](Self::get_device).
    pub async fn get_device_raw(&self, mac: &str) -> Result<Option<serde_json::Value>, Error> {
        let url = self.site_url("stat/device");
        let body = json!({ "macs": [mac.to_lowercase()] });
        let devices: Vec<serde_json::Value> = self.post(url, &body).await?;
        Ok(devices.into_iter().next())
    }

    /// Adopt a pending device.
    ///
    /// `POST /api/s/{site}/cmd/devmgr` with `{"cmd": "adopt", "mac": "..."}`
    pub async fn adopt_device(&self, mac: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/devmgr");
        debug!(mac, "adopting device");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "adopt",
                    "mac": mac,
                }),
            )
            .await?;
        Ok(())
    }

    /// Restart a device.
    ///
    /// `POST /api/s/{site}/cmd/devmgr` with `{"cmd": "restart", "mac": "..."}`
    pub async fn restart_device(&self, mac: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/devmgr");
        debug!(mac, "restarting device");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "restart",
                    "mac": mac,
                }),
            )
            .await?;
        Ok(())
    }

    /// Upgrade device firmware.
    ///
    /// If `url` is `Some`, upgrades from that URL (`cmd: "upgrade-external"`).
    /// Otherwise upgrades from Ubiquiti's cloud (`cmd: "upgrade"`).
    pub async fn upgrade_device(&self, mac: &str, firmware_url: Option<&str>) -> Result<(), Error> {
        let api_url = self.site_url("cmd/devmgr");
        debug!(mac, ?firmware_url, "upgrading device firmware");

        let body = match firmware_url {
            Some(fw_url) => json!({
                "cmd": "upgrade-external",
                "mac": mac,
                "url": fw_url,
            }),
            None => json!({
                "cmd": "upgrade",
                "mac": mac,
            }),
        };

        let _: Vec<serde_json::Value> = self.post(api_url, &body).await?;
        Ok(())
    }

    /// Force re-provision a device configuration.
    ///
    /// `POST /api/s/{site}/cmd/devmgr` with `{"cmd": "force-provision", "mac": "..."}`
    pub async fn provision_device(&self, mac: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/devmgr");
        debug!(mac, "force-provisioning device");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "force-provision",
                    "mac": mac,
                }),
            )
            .await?;
        Ok(())
    }

    /// Trigger a site speed test (gateway).
    ///
    /// `POST /api/s/{site}/cmd/devmgr` with `{"cmd": "speedtest"}`
    pub async fn speedtest(&self) -> Result<(), Error> {
        let url = self.site_url("cmd/devmgr");
        debug!("starting speed test");
        let _: Vec<serde_json::Value> = self.post(url, &json!({ "cmd": "speedtest" })).await?;
        Ok(())
    }

    /// Toggle the LED locator on a device.
    ///
    /// `enable: true` sends `set-locate`, `false` sends `unset-locate`.
    pub async fn locate_device(&self, mac: &str, enable: bool) -> Result<(), Error> {
        let url = self.site_url("cmd/devmgr");
        let cmd = if enable { "set-locate" } else { "unset-locate" };
        debug!(mac, cmd, "toggling device locate LED");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": cmd,
                    "mac": mac,
                }),
            )
            .await?;
        Ok(())
    }

    /// Update the `port_overrides` array on a device.
    ///
    /// `PUT /api/s/{site}/rest/device/{device_id}` replaces the full
    /// `port_overrides` list in one shot. Callers should merge the existing
    /// overrides with the desired change before calling this method.
    pub async fn update_device_port_overrides(
        &self,
        device_id: &str,
        port_overrides: Vec<serde_json::Value>,
    ) -> Result<(), Error> {
        let url = self.site_url(&format!("rest/device/{device_id}"));
        debug!(device_id, "updating device port_overrides");
        let _: Vec<serde_json::Value> = self
            .put(url, &json!({ "port_overrides": port_overrides }))
            .await?;
        Ok(())
    }
}
