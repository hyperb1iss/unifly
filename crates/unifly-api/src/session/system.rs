// Session API system endpoints
//
// Controller-level operations: sysinfo, health dashboard, backup management.

use serde_json::json;
use tracing::debug;

use crate::error::Error;
use crate::session::client::SessionClient;

impl SessionClient {
    /// Get controller system information.
    ///
    /// `GET /api/s/{site}/stat/sysinfo`
    ///
    /// Returns loosely-typed JSON because the field set varies by
    /// platform and firmware version.
    pub async fn get_sysinfo(&self) -> Result<serde_json::Value, Error> {
        let url = self.site_url("stat/sysinfo");
        debug!("fetching sysinfo");
        let mut data: Vec<serde_json::Value> = self.get(url).await?;
        // sysinfo typically returns a single-element array
        Ok(data.pop().unwrap_or(serde_json::Value::Null))
    }

    /// Get site health dashboard metrics.
    ///
    /// `GET /api/s/{site}/stat/health`
    ///
    /// Returns subsystem health entries (wan, lan, wlan, vpn, etc.).
    pub async fn get_health(&self) -> Result<Vec<serde_json::Value>, Error> {
        let url = self.site_url("stat/health");
        debug!("fetching site health");
        self.get(url).await
    }

    /// List available controller backups.
    ///
    /// `POST /api/s/{site}/cmd/backup` with `{"cmd": "list-backups"}`
    pub async fn list_backups(&self) -> Result<Vec<serde_json::Value>, Error> {
        let url = self.site_url("cmd/backup");
        debug!("listing backups");
        self.post(url, &json!({ "cmd": "list-backups" })).await
    }

    /// Delete a backup file from the controller.
    ///
    /// `POST /api/s/{site}/cmd/backup` with
    /// `{"cmd": "delete-backup", "filename": "..."}`
    pub async fn delete_backup(&self, filename: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/backup");
        debug!(filename, "deleting backup");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "delete-backup",
                    "filename": filename,
                }),
            )
            .await?;
        Ok(())
    }

    /// Download a backup file from the controller.
    ///
    /// `GET /dl/autobackup/{filename}`
    pub async fn download_backup(&self, filename: &str) -> Result<Vec<u8>, Error> {
        let prefix = self
            .platform()
            .session_prefix()
            .unwrap_or("")
            .trim_end_matches('/');
        let base = self.base_url().as_str().trim_end_matches('/');
        let encoded: String = url::form_urlencoded::byte_serialize(filename.as_bytes()).collect();
        let url = format!("{base}{prefix}/dl/autobackup/{encoded}");
        debug!(filename, url, "downloading backup");
        let resp = self
            .http()
            .get(url)
            .send()
            .await
            .map_err(|e| self.transport_error(e))?;
        if !resp.status().is_success() {
            return Err(Error::SessionApi {
                message: format!("backup download failed: HTTP {}", resp.status()),
            });
        }
        let bytes = resp.bytes().await.map_err(|e| self.transport_error(e))?;
        Ok(bytes.to_vec())
    }

    /// List controller admins.
    ///
    /// `GET /api/stat/admin` — controller-level (not site-scoped).
    pub async fn list_admins(&self) -> Result<Vec<serde_json::Value>, Error> {
        let url = self.api_url("stat/admin");
        debug!("listing admins");
        self.get(url).await
    }

    /// Create a new controller backup.
    ///
    /// `POST /api/s/{site}/cmd/backup` with `{"cmd": "backup"}`
    pub async fn create_backup(&self) -> Result<(), Error> {
        let url = self.site_url("cmd/backup");
        debug!("creating backup");
        let _: Vec<serde_json::Value> = self.post(url, &json!({ "cmd": "backup" })).await?;
        Ok(())
    }

    /// Reboot the controller.
    ///
    /// `POST /api/system/reboot`
    pub async fn reboot_controller(&self) -> Result<(), Error> {
        let url = self.api_url("system/reboot");
        debug!("rebooting controller");
        let _: Vec<serde_json::Value> = self.post(url, &json!({})).await?;
        Ok(())
    }

    /// Power off the controller.
    ///
    /// `POST /api/system/poweroff`
    pub async fn poweroff_controller(&self) -> Result<(), Error> {
        let url = self.api_url("system/poweroff");
        debug!("powering off controller");
        let _: Vec<serde_json::Value> = self.post(url, &json!({})).await?;
        Ok(())
    }
}
