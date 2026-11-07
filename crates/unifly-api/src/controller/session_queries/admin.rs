use crate::controller::Controller;
use crate::controller::support::require_session;
use crate::core_error::CoreError;
use crate::model::{Admin, EntityId};

impl Controller {
    pub async fn list_admins(&self) -> Result<Vec<Admin>, CoreError> {
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        let raw = session.list_admins().await?;
        Ok(raw
            .into_iter()
            .map(|value| Admin {
                id: value
                    .get("_id")
                    .and_then(|value| value.as_str())
                    .map_or_else(
                        || EntityId::Legacy("unknown".into()),
                        |value| EntityId::Legacy(value.into()),
                    ),
                name: value
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_owned(),
                email: value
                    .get("email")
                    .and_then(|value| value.as_str())
                    .map(String::from),
                role: value
                    .get("role")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown")
                    .to_owned(),
                is_super: value
                    .get("is_super")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                last_login: None,
            })
            .collect())
    }

    pub async fn list_users(
        &self,
    ) -> Result<Vec<crate::session::models::SessionUserEntry>, CoreError> {
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        Ok(session.list_users().await?)
    }
}
