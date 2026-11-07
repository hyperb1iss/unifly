use reqwest::{Method, StatusCode};
use serde_json::Value;
use url::Url;

use super::IntegrationClient;
use crate::Error;

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
        if deleting || response.status() == StatusCode::NO_CONTENT {
            self.handle_empty(response).await?;
            Ok(Value::Null)
        } else {
            self.handle_response(response).await
        }
    }
}
