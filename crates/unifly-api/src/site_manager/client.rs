use reqwest::header::{HeaderMap, HeaderValue};
use secrecy::ExposeSecret;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tracing::debug;
use url::Url;

use super::types::{
    CloudDevice, FleetPage, FleetSite, Host, IspMetric, IspMetricInterval, SdWanConfig, SdWanStatus,
};
use crate::Error;
use crate::error::body_preview;

#[derive(serde::Deserialize)]
struct FleetEnvelope {
    data: Value,
    #[serde(rename = "traceId")]
    trace_id: Option<String>,
    #[serde(rename = "nextToken")]
    next_token: Option<String>,
    status: Option<String>,
}

#[derive(serde::Deserialize)]
struct ErrorResponse {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    code: Option<String>,
}

pub struct SiteManagerClient {
    http: reqwest::Client,
    base_url: Url,
}

impl SiteManagerClient {
    pub fn from_api_key(
        base_url: &str,
        api_key: &secrecy::SecretString,
        transport: &crate::TransportConfig,
    ) -> Result<Self, Error> {
        let mut headers = HeaderMap::new();
        let mut key_value = HeaderValue::from_str(api_key.expose_secret()).map_err(|error| {
            Error::Authentication {
                message: format!("invalid API key header value: {error}"),
            }
        })?;
        key_value.set_sensitive(true);
        headers.insert("X-API-KEY", key_value);

        let http = transport.build_client_with_headers(headers)?;
        let base_url = Self::normalize_base_url(base_url)?;

        Ok(Self { http, base_url })
    }

    pub fn from_reqwest(base_url: &str, http: reqwest::Client) -> Result<Self, Error> {
        let base_url = Self::normalize_base_url(base_url)?;
        Ok(Self { http, base_url })
    }

    fn normalize_base_url(raw: &str) -> Result<Url, Error> {
        let mut url = Url::parse(raw)?;
        let path = url.path().trim_end_matches('/').to_owned();
        if path.ends_with("/v1") {
            url.set_path(&format!("{path}/"));
        } else {
            url.set_path(&format!("{path}/v1/"));
        }
        Ok(url)
    }

    fn url(&self, path: &str) -> Url {
        self.base_url
            .join(path)
            .expect("site manager path should be a valid relative URL")
    }

    pub async fn list_hosts(&self) -> Result<Vec<Host>, Error> {
        Ok(self.paginate("hosts", Vec::new()).await?.data)
    }

    pub async fn get_host(&self, host_id: &str) -> Result<Host, Error> {
        let envelope = self.get_envelope(&format!("hosts/{host_id}"), &[]).await?;
        Self::decode_single(envelope)
    }

    pub async fn list_sites(&self) -> Result<Vec<FleetSite>, Error> {
        Ok(self.paginate("sites", Vec::new()).await?.data)
    }

    pub async fn list_devices(&self, host_ids: &[String]) -> Result<Vec<CloudDevice>, Error> {
        let params = host_ids
            .iter()
            .map(|host_id| ("hostIds", host_id.clone()))
            .collect();
        Ok(self.paginate("devices", params).await?.data)
    }

    pub async fn get_isp_metrics(
        &self,
        interval: IspMetricInterval,
    ) -> Result<FleetPage<IspMetric>, Error> {
        self.paginate(
            &format!("isp-metrics/{}", interval.as_path_segment()),
            Vec::new(),
        )
        .await
    }

    pub async fn query_isp_metrics(
        &self,
        interval: IspMetricInterval,
        site_ids: &[String],
    ) -> Result<FleetPage<IspMetric>, Error> {
        let body = serde_json::json!({ "siteIds": site_ids });
        let envelope = self
            .post_envelope(
                &format!("isp-metrics/{}/query", interval.as_path_segment()),
                &body,
            )
            .await?;
        Self::decode_list(envelope)
    }

    pub async fn list_sdwan_configs(&self) -> Result<Vec<SdWanConfig>, Error> {
        Ok(self.paginate("sd-wan-configs", Vec::new()).await?.data)
    }

    pub async fn get_sdwan_config(&self, config_id: &str) -> Result<SdWanConfig, Error> {
        let envelope = self
            .get_envelope(&format!("sd-wan-configs/{config_id}"), &[])
            .await?;
        Self::decode_single(envelope)
    }

    pub async fn get_sdwan_status(&self, config_id: &str) -> Result<SdWanStatus, Error> {
        let envelope = self
            .get_envelope(&format!("sd-wan-configs/{config_id}/status"), &[])
            .await?;
        Self::decode_single(envelope)
    }

    async fn paginate<T>(
        &self,
        path: &str,
        params: Vec<(&str, String)>,
    ) -> Result<FleetPage<T>, Error>
    where
        T: DeserializeOwned,
    {
        let mut data = Vec::new();
        let mut next_token: Option<String> = None;
        let mut trace_id: Option<String> = None;
        let mut status: Option<String> = None;

        loop {
            let mut page_params = params.clone();
            if let Some(ref token) = next_token {
                page_params.push(("nextToken", token.clone()));
            }

            let envelope = self.get_envelope(path, &page_params).await?;
            let page = Self::decode_list(envelope)?;
            trace_id = page.trace_id.clone().or(trace_id);
            status = page.status.clone().or(status);
            next_token.clone_from(&page.next_token);
            data.extend(page.data);

            if next_token.is_none() {
                break;
            }
        }

        Ok(FleetPage {
            data,
            next_token,
            trace_id,
            status,
        })
    }

    async fn get_envelope(
        &self,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<FleetEnvelope, Error> {
        let url = self.url(path);
        debug!("GET {url} params={params:?}");
        let response = self.http.get(url).query(params).send().await?;
        self.handle_response(response).await
    }

    async fn post_envelope<B: Serialize + Sync>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<FleetEnvelope, Error> {
        let url = self.url(path);
        debug!("POST {url}");
        let response = self.http.post(url).json(body).send().await?;
        self.handle_response(response).await
    }

    async fn handle_response(&self, response: reqwest::Response) -> Result<FleetEnvelope, Error> {
        let status = response.status();
        if status.is_success() {
            let body = response.text().await?;
            serde_json::from_str(&body).map_err(|error| {
                let preview = body_preview(body.as_bytes());
                Error::Deserialization {
                    message: format!("{error} (body preview: {preview:?})"),
                    body,
                }
            })
        } else {
            Err(self.parse_error(status, response).await)
        }
    }

    async fn parse_error(&self, status: reqwest::StatusCode, response: reqwest::Response) -> Error {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Error::InvalidApiKey;
        }

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Error::RateLimited {
                retry_after_secs: retry_after_secs(&response).unwrap_or(5),
            };
        }

        let raw = response.text().await.unwrap_or_default();
        if let Ok(error) = serde_json::from_str::<ErrorResponse>(&raw) {
            return Error::Integration {
                status: status.as_u16(),
                message: error.message.unwrap_or_else(|| status.to_string()),
                code: error.code,
            };
        }

        Error::Integration {
            status: status.as_u16(),
            message: if raw.is_empty() {
                status.to_string()
            } else {
                raw
            },
            code: None,
        }
    }

    fn decode_list<T>(envelope: FleetEnvelope) -> Result<FleetPage<T>, Error>
    where
        T: DeserializeOwned,
    {
        let items = match envelope.data {
            Value::Array(items) => items,
            Value::Null => Vec::new(),
            item => vec![item],
        };

        let mut decoded = Vec::with_capacity(items.len());
        for item in items {
            decoded.push(decode_item(&item)?);
        }

        Ok(FleetPage {
            data: decoded,
            next_token: envelope.next_token,
            trace_id: envelope.trace_id,
            status: envelope.status,
        })
    }

    fn decode_single<T>(envelope: FleetEnvelope) -> Result<T, Error>
    where
        T: DeserializeOwned,
    {
        match envelope.data {
            Value::Array(mut items) => {
                let item = items.pop().ok_or_else(|| Error::Deserialization {
                    message: "expected a single Site Manager record, got an empty list".into(),
                    body: "[]".into(),
                })?;
                decode_item(&item)
            }
            item => decode_item(&item),
        }
    }
}

fn decode_item<T>(item: &Value) -> Result<T, Error>
where
    T: DeserializeOwned,
{
    serde_json::from_value(item.clone()).map_err(|error| Error::Deserialization {
        message: error.to_string(),
        body: item.to_string(),
    })
}

fn retry_after_secs(response: &reqwest::Response) -> Option<u64> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_retry_after)
}

fn parse_retry_after(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    let numeric = trimmed.strip_suffix('s').unwrap_or(trimmed);
    if let Ok(seconds) = numeric.parse::<u64>() {
        return Some(seconds);
    }

    let (whole, fractional) = numeric.split_once('.')?;
    let whole = whole.parse::<u64>().ok()?;
    let has_fraction = fractional.chars().any(|ch| ch != '0');
    Some(whole + u64::from(has_fraction))
}
