use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;

use super::SessionClient;
use crate::error::Error;
use crate::session::models::SessionResponse;

fn check_error_envelope(value: &serde_json::Value) -> Result<(), Error> {
    if let Some(meta) = value.get("meta") {
        let meta: crate::session::models::Meta =
            serde_json::from_value(meta.clone()).map_err(|error| Error::Deserialization {
                message: error.to_string(),
                body: value.to_string(),
            })?;
        if meta.rc != "ok" {
            return Err(Error::SessionApi {
                message: meta.msg.unwrap_or_else(|| format!("rc={}", meta.rc)),
            });
        }
    }
    Ok(())
}

impl SessionClient {
    /// POST a read-only query, preserving raw response metadata and retrying expired sessions.
    pub(crate) async fn post_read_raw<T: DeserializeOwned>(
        &self,
        url: Url,
        body: &(impl Serialize + Sync),
    ) -> Result<T, Error> {
        self.with_reauth(|| async {
            let builder = self.apply_csrf(self.http.post(url.clone()).json(body));
            let response = builder.send().await.map_err(|e| self.transport_error(e))?;
            let bytes = self.checked_raw_body(response).await?;
            let value = super::decode_raw_json(&bytes)?;
            check_error_envelope(&value)?;
            serde_json::from_value(value).map_err(|error| Error::Deserialization {
                message: error.to_string(),
                body: String::from_utf8_lossy(&bytes).into_owned(),
            })
        })
        .await
    }

    /// Read an enveloped page without dropping malformed records or changing its offsets.
    pub(crate) async fn post_read<T: DeserializeOwned>(
        &self,
        url: Url,
        body: &(impl Serialize + Sync),
    ) -> Result<Vec<T>, Error> {
        let envelope: SessionResponse<T> = self.post_read_raw(url, body).await?;
        Ok(envelope.data)
    }

    /// PUT a mutation whose success response may be empty, without replaying it after login.
    pub(crate) async fn put_empty(
        &self,
        url: Url,
        body: &(impl Serialize + Sync),
    ) -> Result<(), Error> {
        self.with_reauth_no_replay(|| async {
            let builder = self.apply_csrf(self.http.put(url.clone()).json(body));
            let response = builder.send().await.map_err(|e| self.transport_error(e))?;
            let status = response.status();
            let bytes = self.checked_raw_body(response).await?;
            if status == reqwest::StatusCode::MULTI_STATUS {
                return Err(Error::SessionApi {
                    message: "HTTP 207 Multi-Status: acknowledgement may be partial; the controller did not confirm that all requested alerts were marked as read".into(),
                });
            }
            if !bytes.is_empty() {
                let value = super::decode_raw_json(&bytes)?;
                check_error_envelope(&value)?;
            }
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::auth::ControllerPlatform;
    use crate::session::ReauthCredentials;
    use crate::transport::TransportConfig;

    #[tokio::test]
    async fn empty_put_accepts_204_and_never_replays_after_expiry() {
        for status in [204, 401] {
            let server = MockServer::start().await;
            let client = SessionClient::new(
                Url::parse(&server.uri()).expect("mock URL"),
                "default".into(),
                ControllerPlatform::UnifiOs,
                &TransportConfig::default(),
            )
            .expect("client");
            client
                .enable_reauth(ReauthCredentials {
                    username: "admin".into(),
                    password: "test-password".to_string().into(),
                    totp_provider: None,
                    cache: None,
                })
                .await;
            Mock::given(method("PUT"))
                .and(path(
                    "/proxy/network/v2/api/site/default/system-log/critical",
                ))
                .and(body_json(json!({"status": "READ"})))
                .respond_with(ResponseTemplate::new(status))
                .expect(1)
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path("/api/auth/login"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .insert_header("Set-Cookie", "TOKEN=renewed; Path=/")
                        .set_body_json(json!({})),
                )
                .expect(u64::from(status == 401))
                .mount(&server)
                .await;
            let result = client
                .put_empty(
                    client.site_url_v2("system-log/critical"),
                    &json!({"status": "READ"}),
                )
                .await;
            if status == 204 {
                assert!(result.is_ok(), "{result:?}");
            } else {
                assert!(matches!(result, Err(Error::SessionExpired)), "{result:?}");
            }
            server.verify().await;
        }
    }

    #[tokio::test]
    async fn empty_put_rejects_error_envelopes_and_malformed_success_bodies() {
        let server = MockServer::start().await;
        let client = SessionClient::new(
            Url::parse(&server.uri()).expect("mock URL"),
            "default".into(),
            ControllerPlatform::UnifiOs,
            &TransportConfig::default(),
        )
        .expect("client");
        for body in [
            json!({"meta": {"rc": "error", "msg": "denied"}, "data": []}),
            json!({"error": {"code": 403, "message": "denied"}}),
        ] {
            server.reset().await;
            Mock::given(method("PUT"))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
            assert!(matches!(
                client
                    .put_empty(client.site_url_v2("system-log/critical"), &json!({}))
                    .await,
                Err(Error::SessionApi { .. })
            ));
        }
        server.reset().await;
        Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not JSON"))
            .mount(&server)
            .await;
        assert!(matches!(
            client
                .put_empty(client.site_url_v2("system-log/critical"), &json!({}))
                .await,
            Err(Error::Deserialization { .. })
        ));
    }
}
