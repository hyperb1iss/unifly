use reqwest::Method;
use serde_json::Value;
use url::Url;

use super::IntegrationClient;
use crate::Error;
use crate::error::body_preview;

impl IntegrationClient {
    /// Raw requests retain the Integration client's API-key authentication and
    /// cloud connector prefix. Appending keeps user input on this controller.
    pub(crate) async fn raw_request(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, Error> {
        let url = Url::parse(&format!("{}{path}", self.base_url))?;
        let deleting = method == Method::DELETE;
        let mut request = self.http.request(method, url);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = self.send(request).await?;
        if deleting {
            self.handle_empty(response).await?;
            return Ok(Value::Null);
        }
        let status = response.status();
        if !status.is_success() {
            return Err(self.parse_error(status, response).await);
        }
        let body = response
            .text()
            .await
            .map_err(|error| Error::from_transport(error, self.timeout))?;
        if body.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&body).map_err(|error| Error::Deserialization {
            message: format!(
                "{error} (body preview: {:?})",
                body_preview(body.as_bytes())
            ),
            body,
        })
    }
}
