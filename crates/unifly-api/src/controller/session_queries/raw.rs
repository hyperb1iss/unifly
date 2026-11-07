use reqwest::Method;
use serde_json::Value;

use crate::controller::Controller;
use crate::controller::support::{integration_client_context, require_session};
use crate::core_error::CoreError;

impl Controller {
    /// Send a raw GET to a controller-relative path without unwrapping envelopes.
    /// Integration paths use API-key auth; other paths use session HTTP auth.
    pub async fn raw_get(&self, path: &str) -> Result<Value, CoreError> {
        self.raw_request(Method::GET, path, None).await
    }

    /// Send a raw POST without replaying a failed mutation.
    pub async fn raw_post(&self, path: &str, body: &Value) -> Result<Value, CoreError> {
        self.raw_request(Method::POST, path, Some(body)).await
    }

    /// Send a raw PUT without replaying a failed mutation.
    pub async fn raw_put(&self, path: &str, body: &Value) -> Result<Value, CoreError> {
        self.raw_request(Method::PUT, path, Some(body)).await
    }

    /// Send a raw PATCH without replaying a failed mutation.
    pub async fn raw_patch(&self, path: &str, body: &Value) -> Result<Value, CoreError> {
        self.raw_request(Method::PATCH, path, Some(body)).await
    }

    /// Send a raw DELETE without replaying a failed mutation.
    pub async fn raw_delete(&self, path: &str) -> Result<(), CoreError> {
        self.raw_request(Method::DELETE, path, None).await?;
        Ok(())
    }

    async fn raw_request(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, CoreError> {
        let path = path.trim_start_matches('/');
        if let Some(path) = path.strip_prefix("integration/") {
            let client = integration_client_context(self, "raw Integration API request").await?;
            return Ok(client.raw_request(method, path, body).await?);
        }
        let session = self.inner.session_client.lock().await.clone();
        let session = require_session(session.as_ref())?;
        let empty = Value::Null;
        let body = body.unwrap_or(&empty);
        Ok(match method {
            Method::GET => session.raw_get(path).await?,
            Method::POST => session.raw_post(path, body).await?,
            Method::PUT => session.raw_put(path, body).await?,
            Method::PATCH => session.raw_patch(path, body).await?,
            Method::DELETE => {
                session.raw_delete(path).await?;
                Value::Null
            }
            _ => unreachable!("private dispatcher receives only supported methods"),
        })
    }
}

#[cfg(test)]
mod tests;
