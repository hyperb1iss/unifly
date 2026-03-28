// Legacy API HTTP client
//
// Wraps `reqwest::Client` with UniFi-specific URL construction, envelope
// unwrapping, and platform-aware path prefixing. All endpoint modules
// (devices, clients, etc.) are implemented as inherent methods via
// separate files to keep this module focused on transport mechanics.

use std::sync::{Arc, RwLock};

use reqwest::cookie::{CookieStore, Jar};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tracing::{debug, trace};
use url::Url;

use crate::auth::ControllerPlatform;
use crate::error::Error;
use crate::legacy::models::LegacyResponse;
use crate::transport::TransportConfig;

/// UniFi OS wraps some errors as `{"error":{"code":N,"message":"..."}}` with HTTP 200.
#[derive(serde::Deserialize)]
struct UnifiOsError {
    error: Option<UnifiOsErrorInner>,
}

#[derive(serde::Deserialize)]
struct UnifiOsErrorInner {
    code: u16,
    message: Option<String>,
}

/// Raw HTTP client for the UniFi controller's legacy API.
///
/// Handles the `{ data: [], meta: { rc, msg } }` envelope, site-scoped
/// URL construction, and platform-aware path prefixing. All methods return
/// unwrapped `data` payloads -- the envelope is stripped before the caller
/// sees it.
pub struct LegacyClient {
    http: reqwest::Client,
    base_url: Url,
    site: String,
    platform: ControllerPlatform,
    /// CSRF token for UniFi OS. Required on all POST/PUT/DELETE requests
    /// through the `/proxy/network/` path. Captured from login response
    /// headers and rotated via `X-Updated-CSRF-Token`.
    csrf_token: RwLock<Option<String>>,
    /// Cookie jar reference for extracting session cookies (e.g. for WebSocket auth).
    cookie_jar: Option<Arc<Jar>>,
}

impl LegacyClient {
    /// Create a new legacy client from a `TransportConfig`.
    ///
    /// If the config doesn't already include a cookie jar, one is created
    /// automatically (legacy auth requires cookies). The `base_url` should be
    /// the controller root (e.g. `https://192.168.1.1` for UniFi OS or
    /// `https://controller:8443` for standalone).
    pub fn new(
        base_url: Url,
        site: String,
        platform: ControllerPlatform,
        transport: &TransportConfig,
    ) -> Result<Self, Error> {
        let config = if transport.cookie_jar.is_some() {
            transport.clone()
        } else {
            transport.clone().with_cookie_jar()
        };
        let cookie_jar = config.cookie_jar.clone();
        let http = config.build_client()?;
        Ok(Self {
            http,
            base_url,
            site,
            platform,
            csrf_token: RwLock::new(None),
            cookie_jar,
        })
    }

    /// Create a legacy client with a pre-built `reqwest::Client`.
    ///
    /// Use this when you already have a client with a session cookie in its
    /// jar (e.g. after authenticating via a shared client).
    pub fn with_client(
        http: reqwest::Client,
        base_url: Url,
        site: String,
        platform: ControllerPlatform,
    ) -> Self {
        Self {
            http,
            base_url,
            site,
            platform,
            csrf_token: RwLock::new(None),
            cookie_jar: None,
        }
    }

    /// The current site identifier.
    pub fn site(&self) -> &str {
        &self.site
    }

    /// The underlying HTTP client (for auth flows that need direct access).
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// The controller base URL.
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// The detected controller platform.
    pub fn platform(&self) -> ControllerPlatform {
        self.platform
    }

    /// Extract the session cookie header value for WebSocket auth.
    ///
    /// Returns the `Cookie` header string (e.g. `"TOKEN=abc123"`) if a
    /// cookie jar is available and contains cookies for the controller URL.
    pub fn cookie_header(&self) -> Option<String> {
        let jar = self.cookie_jar.as_ref()?;
        let cookies = jar.cookies(&self.base_url)?;
        cookies.to_str().ok().map(String::from)
    }

    // ── Session cache support ─────────────────────────────────────────

    /// Restore a cached session by injecting the TOKEN cookie into the
    /// cookie jar and setting the CSRF token.
    pub fn restore_session(&self, token: &str, csrf_token: Option<&str>) {
        if let Some(ref jar) = self.cookie_jar {
            let cookie_str = format!("TOKEN={token}");
            jar.add_cookie_str(&cookie_str, &self.base_url);
            debug!("restored TOKEN cookie from cache");
        }
        if let Some(csrf) = csrf_token {
            self.set_csrf_token(csrf.to_owned());
        }
    }

    /// Extract the current TOKEN cookie value from the cookie jar.
    pub fn extract_token(&self) -> Option<String> {
        let jar = self.cookie_jar.as_ref()?;
        let header = jar.cookies(&self.base_url)?;
        let header_str = header.to_str().ok()?;
        // Parse "TOKEN=value" from the cookie header (may contain multiple cookies).
        for part in header_str.split(';') {
            let trimmed = part.trim();
            if let Some(value) = trimmed.strip_prefix("TOKEN=") {
                return Some(value.to_owned());
            }
        }
        None
    }

    /// Read the current CSRF token.
    pub fn csrf_token(&self) -> Option<String> {
        self.csrf_token.read().expect("CSRF lock poisoned").clone()
    }

    // ── CSRF token management ─────────────────────────────────────────

    /// Store a CSRF token (captured from login response headers).
    pub(crate) fn set_csrf_token(&self, token: String) {
        debug!("storing CSRF token");
        *self.csrf_token.write().expect("CSRF lock poisoned") = Some(token);
    }

    /// Update CSRF token if the response contains a rotated value.
    fn update_csrf_from_response(&self, headers: &reqwest::header::HeaderMap) {
        // UniFi OS may rotate tokens — prefer the updated one.
        let new_token = headers
            .get("X-Updated-CSRF-Token")
            .or_else(|| headers.get("x-csrf-token"))
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        if let Some(token) = new_token {
            trace!("CSRF token rotated");
            *self.csrf_token.write().expect("CSRF lock poisoned") = Some(token);
        }
    }

    /// Apply the stored CSRF token to a request builder.
    fn apply_csrf(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let guard = self.csrf_token.read().expect("CSRF lock poisoned");
        match guard.as_deref() {
            Some(token) => builder.header("X-CSRF-Token", token),
            None => builder,
        }
    }

    // ── URL builders ─────────────────────────────────────────────────

    /// Build a full URL for a controller-level API path.
    ///
    /// Applies the platform-specific legacy prefix, then appends `/api/{path}`.
    /// For example, on UniFi OS: `https://host/proxy/network/api/{path}`
    pub(crate) fn api_url(&self, path: &str) -> Url {
        let prefix = self.platform.legacy_prefix().unwrap_or("");
        let base = self.base_url.as_str().trim_end_matches('/');
        let prefix = prefix.trim_end_matches('/');
        let full = format!("{base}{prefix}/api/{path}");
        Url::parse(&full).expect("invalid API URL")
    }

    /// Build a site-scoped URL: `{base}{prefix}/api/s/{site}/{path}`
    ///
    /// Most legacy endpoints are site-scoped: stat/device, cmd/devmgr, etc.
    pub(crate) fn site_url(&self, path: &str) -> Url {
        let prefix = self.platform.legacy_prefix().unwrap_or("");
        let base = self.base_url.as_str().trim_end_matches('/');
        let prefix = prefix.trim_end_matches('/');
        let full = format!("{base}{prefix}/api/s/{}/{path}", self.site);
        Url::parse(&full).expect("invalid site URL")
    }

    // ── Request helpers ──────────────────────────────────────────────

    /// Send a GET request and unwrap the legacy envelope.
    pub(crate) async fn get<T: DeserializeOwned>(&self, url: Url) -> Result<Vec<T>, Error> {
        debug!("GET {}", url);

        let resp = self.http.get(url).send().await.map_err(Error::Transport)?;

        self.parse_envelope(resp).await
    }

    /// Send a POST request with JSON body and unwrap the legacy envelope.
    pub(crate) async fn post<T: DeserializeOwned>(
        &self,
        url: Url,
        body: &(impl Serialize + Sync),
    ) -> Result<Vec<T>, Error> {
        debug!("POST {}", url);

        let builder = self.apply_csrf(self.http.post(url).json(body));
        let resp = builder.send().await.map_err(Error::Transport)?;

        self.parse_envelope(resp).await
    }

    /// Send a PUT request with JSON body and unwrap the legacy envelope.
    #[allow(dead_code)]
    pub(crate) async fn put<T: DeserializeOwned>(
        &self,
        url: Url,
        body: &(impl Serialize + Sync),
    ) -> Result<Vec<T>, Error> {
        debug!("PUT {}", url);

        let builder = self.apply_csrf(self.http.put(url).json(body));
        let resp = builder.send().await.map_err(Error::Transport)?;

        self.parse_envelope(resp).await
    }

    /// Send a DELETE request and unwrap the legacy envelope.
    #[allow(dead_code)]
    pub(crate) async fn delete<T: DeserializeOwned>(&self, url: Url) -> Result<Vec<T>, Error> {
        debug!("DELETE {}", url);

        let builder = self.apply_csrf(self.http.delete(url));
        let resp = builder.send().await.map_err(Error::Transport)?;

        self.parse_envelope(resp).await
    }

    /// Parse the `{ meta, data }` envelope, returning `data` on success
    /// or an `Error::LegacyApi` if `meta.rc != "ok"`.
    ///
    /// Also handles UniFi OS error responses that use a different shape:
    /// `{"error": {"code": 403, "message": "..."}}` (returned with HTTP 200).
    async fn parse_envelope<T: DeserializeOwned>(
        &self,
        resp: reqwest::Response,
    ) -> Result<Vec<T>, Error> {
        let status = resp.status();

        // Capture any CSRF token rotation before consuming the response.
        self.update_csrf_from_response(resp.headers());

        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(Error::Authentication {
                message: "session expired or invalid credentials".into(),
            });
        }

        if status == reqwest::StatusCode::FORBIDDEN {
            return Err(Error::LegacyApi {
                message: "insufficient permissions (HTTP 403)".into(),
            });
        }

        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::LegacyApi {
                message: format!("HTTP {status}: {}", &body[..body.len().min(200)]),
            });
        }

        let body = resp.text().await.map_err(Error::Transport)?;

        // UniFi OS sometimes returns `{"error":{"code":N,"message":"..."}}` with HTTP 200.
        if let Ok(wrapper) = serde_json::from_str::<UnifiOsError>(&body)
            && let Some(err) = wrapper.error
        {
            let msg = err.message.unwrap_or_default();
            return Err(if err.code == 401 {
                Error::Authentication { message: msg }
            } else {
                Error::LegacyApi {
                    message: format!("UniFi OS error {}: {msg}", err.code),
                }
            });
        }

        let envelope: LegacyResponse<T> = serde_json::from_str(&body).map_err(|e| {
            let preview = &body[..body.len().min(200)];
            Error::Deserialization {
                message: format!("{e} (body preview: {preview:?})"),
                body: body.clone(),
            }
        })?;

        match envelope.meta.rc.as_str() {
            "ok" => Ok(envelope.data),
            _ => Err(Error::LegacyApi {
                message: envelope
                    .meta
                    .msg
                    .unwrap_or_else(|| format!("rc={}", envelope.meta.rc)),
            }),
        }
    }
}
