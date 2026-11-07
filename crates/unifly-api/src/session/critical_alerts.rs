use serde_json::json;

use crate::error::Error;
use crate::session::client::SessionClient;
use crate::session::system_log::SystemLogEntry;

impl SessionClient {
    /// List aggregated critical dashboard alerts, rather than the full system-log history.
    ///
    /// `POST /v2/api/site/{site}/system-log/critical`
    pub async fn list_critical_alerts(&self) -> Result<Vec<SystemLogEntry>, Error> {
        self.post_read_raw(self.site_url_v2("system-log/critical"), &json!({}))
            .await
    }

    /// Mark a critical alert group as read, including other records in the same group.
    ///
    /// `PUT /v2/api/site/{site}/system-log/critical/{id}/mark-as-read`
    ///
    /// The mutation is not replayed if the session expires during the request.
    pub async fn acknowledge_critical_alert(&self, id: &str) -> Result<(), Error> {
        if id.is_empty() || matches!(id, "." | "..") {
            return Err(Error::SessionApi {
                message: "Critical alert ID must be a nonempty path segment other than '.' or '..'"
                    .into(),
            });
        }

        let mut url = self.site_url_v2("system-log/critical");
        url.path_segments_mut()
            .map_err(|()| Error::SessionApi {
                message: "Critical alert endpoint cannot contain path segments".into(),
            })?
            .push(id)
            .push("mark-as-read");
        self.put_empty(url, &json!({})).await
    }

    /// Mark all critical dashboard alert groups as read.
    ///
    /// `PUT /v2/api/site/{site}/system-log/critical/mark-all-as-read`
    ///
    /// The mutation is not replayed if the session expires during the request.
    pub async fn acknowledge_all_critical_alerts(&self) -> Result<(), Error> {
        self.put_empty(
            self.site_url_v2("system-log/critical/mark-all-as-read"),
            &json!({}),
        )
        .await
    }
}
