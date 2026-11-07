// Session API site endpoints
//
// Site listing is controller-scoped (not site-scoped), using
// `/api/self/sites` rather than the usual `/api/s/{site}/...` pattern.

use serde_json::json;
use tracing::debug;

use crate::error::Error;
use crate::session::client::SessionClient;
use crate::session::models::SessionSite;

impl SessionClient {
    /// List all sites visible to the authenticated user.
    ///
    /// `GET /api/self/sites` (controller-level, not site-scoped)
    pub async fn list_sites(&self) -> Result<Vec<SessionSite>, Error> {
        let url = self.api_url("self/sites");
        debug!("listing sites");
        self.get(url).await
    }

    /// Get all site settings.
    ///
    /// `GET /api/s/{site}/rest/setting`
    pub async fn get_site_settings(&self) -> Result<Vec<serde_json::Value>, Error> {
        let url = self.site_url("rest/setting");
        debug!("fetching site settings");
        self.get(url).await
    }

    /// Create a new site.
    ///
    /// The controller generates the internal site reference. `name` is the display name.
    ///
    /// `POST /api/s/{site}/cmd/sitemgr` with `{"cmd": "add-site", "desc": "..."}`
    pub async fn create_site(&self, name: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/sitemgr");
        debug!(name, "creating site");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "add-site",
                    "desc": name,
                }),
            )
            .await?;
        Ok(())
    }

    /// Delete a site.
    ///
    /// `POST /api/s/{site}/cmd/sitemgr` with `{"cmd": "delete-site", "name": "..."}`
    pub async fn delete_site(&self, name: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/sitemgr");
        debug!(name, "deleting site");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "delete-site",
                    "name": name,
                }),
            )
            .await?;
        Ok(())
    }

    /// Invite a site administrator.
    ///
    /// `POST /api/s/{site}/cmd/sitemgr` with `{"cmd": "invite-admin", ...}`
    pub async fn invite_admin(&self, name: &str, email: &str, role: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/sitemgr");
        debug!(name, email, role, "inviting admin");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "invite-admin",
                    "name": name,
                    "email": email,
                    "role": role,
                }),
            )
            .await?;
        Ok(())
    }

    /// Revoke a site administrator.
    ///
    /// `POST /api/s/{site}/cmd/sitemgr` with `{"cmd": "revoke-admin", "admin": "..."}`
    pub async fn revoke_admin(&self, admin_id: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/sitemgr");
        debug!(admin_id, "revoking admin");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "revoke-admin",
                    "admin": admin_id,
                }),
            )
            .await?;
        Ok(())
    }

    /// Update a site setting.
    ///
    /// `PUT /api/s/{site}/set/setting/{key}` with an arbitrary JSON body.
    pub async fn set_site_setting(&self, key: &str, body: &serde_json::Value) -> Result<(), Error> {
        let url = self.site_url(&format!("set/setting/{key}"));
        debug!(key, "updating site setting");
        let _: Vec<serde_json::Value> = self.put(url, body).await?;
        Ok(())
    }

    /// Update site administrator role.
    ///
    /// `POST /api/s/{site}/cmd/sitemgr` with `{"cmd": "update-admin", ...}`
    pub async fn update_admin(&self, admin_id: &str, role: Option<&str>) -> Result<(), Error> {
        let url = self.site_url("cmd/sitemgr");
        debug!(admin_id, ?role, "updating admin");
        let mut body = serde_json::Map::new();
        body.insert(
            "cmd".into(),
            serde_json::Value::String("update-admin".into()),
        );
        body.insert(
            "admin".into(),
            serde_json::Value::String(admin_id.to_owned()),
        );
        if let Some(role) = role {
            body.insert("role".into(), serde_json::Value::String(role.to_owned()));
        }
        let _: Vec<serde_json::Value> = self.post(url, &serde_json::Value::Object(body)).await?;
        Ok(())
    }
}
