// Session API HTTP client
//
// Wraps `reqwest::Client` with UniFi-specific URL construction, envelope
// unwrapping, and platform-aware path prefixing. All endpoint modules
// (devices, clients, etc.) are implemented as inherent methods via
// separate files to keep this module focused on transport mechanics.

use std::sync::{Arc, RwLock};

use reqwest::cookie::{CookieStore, Jar};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tracing::{debug, info, trace, warn};
use url::Url;

use crate::auth::ControllerPlatform;
use crate::error::{Error, body_preview};
use crate::session::models::SessionResponse;
use crate::session::totp::{TotpProvider, is_totp_code};
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

/// How this session client authenticates with the controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAuth {
    /// Real session cookie from username/password login.
    Cookie,
    /// API key passed via `X-API-KEY` header (no session cookie).
    /// Some session endpoints (e.g. `stat/event`) are unavailable.
    ApiKey,
}

/// Credentials retained after a cookie login so the client can log in again
/// on its own when the controller reports the session as expired.
///
/// Without this a long-running process (the TUI) silently loses every
/// Session-backed feature roughly an hour in, when the cookie lapses.
///
/// There is deliberately no field for a fixed TOTP code: a code is valid
/// for one 30 s step, so replaying the one used at connect would fail every
/// re-login while still presenting the correct password, which is how
/// accounts get locked. An MFA session renews itself only through
/// `totp_provider`, which is asked for a current code on every attempt.
pub struct ReauthCredentials {
    pub username: String,
    pub password: secrecy::SecretString,
    /// Source of a current TOTP code for each re-login, for accounts with
    /// MFA. When set, an attempt for which it yields no usable code counts
    /// as a failed attempt and sends no login at all.
    pub totp_provider: Option<TotpProvider>,
    /// Session cache to refresh after a successful re-login, if enabled.
    pub cache: Option<super::session_cache::SessionCache>,
}

impl std::fmt::Debug for ReauthCredentials {
    /// Hand-written so the password can never reach a log line; only the
    /// username and which optional parts are attached are shown.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReauthCredentials")
            .field("username", &self.username)
            .field("totp_provider", &self.totp_provider.is_some())
            .field("cache", &self.cache.is_some())
            .finish_non_exhaustive()
    }
}

/// Minimum spacing between re-login attempts after a failure, so a changed
/// password cannot turn every refresh into a login storm.
const REAUTH_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Default)]
struct ReauthSlot {
    credentials: Option<ReauthCredentials>,
    last_attempt: Option<std::time::Instant>,
}

/// Raw HTTP client for the UniFi controller's session API.
///
/// Handles the `{ data: [], meta: { rc, msg } }` envelope, site-scoped
/// URL construction, and platform-aware path prefixing. All methods return
/// unwrapped `data` payloads -- the envelope is stripped before the caller
/// sees it.
pub struct SessionClient {
    http: reqwest::Client,
    base_url: Url,
    site: String,
    platform: ControllerPlatform,
    auth: SessionAuth,
    /// CSRF token for UniFi OS. Required on all POST/PUT/DELETE requests
    /// through the `/proxy/network/` path. Captured from login response
    /// headers and rotated via `X-Updated-CSRF-Token`.
    csrf_token: RwLock<Option<String>>,
    /// Cookie jar reference for extracting session cookies (e.g. for WebSocket auth).
    cookie_jar: Option<Arc<Jar>>,
    /// Self-healing state: see [`ReauthCredentials`].
    reauth: tokio::sync::Mutex<ReauthSlot>,
    /// Bumped on every successful login or cache restore. A request records
    /// it before it is sent, so a 401 that describes an already-replaced
    /// cookie can be told apart from a fresh expiry.
    session_generation: std::sync::atomic::AtomicU64,
    /// The last TOTP code an MFA login sent. It is spent, so a re-login
    /// refuses a provider that hands it back.
    spent_totp: std::sync::Mutex<Option<secrecy::SecretString>>,
    /// Request timeout `http` was built with, when known.
    request_timeout: Option<std::time::Duration>,
}

/// Decode the body of a raw route as JSON.
fn decode_raw_json(body: &[u8]) -> Result<serde_json::Value, Error> {
    serde_json::from_slice(body).map_err(|e| Error::Deserialization {
        message: format!("{e}"),
        body: String::from_utf8_lossy(body).into_owned(),
    })
}

impl SessionClient {
    /// Create a new session client from a `TransportConfig`.
    ///
    /// If the config doesn't already include a cookie jar, one is created
    /// automatically (session auth requires cookies). The `base_url` should be
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
            auth: SessionAuth::Cookie,
            csrf_token: RwLock::new(None),
            cookie_jar,
            reauth: tokio::sync::Mutex::default(),
            session_generation: std::sync::atomic::AtomicU64::new(0),
            spent_totp: std::sync::Mutex::default(),
            request_timeout: Some(config.timeout),
        })
    }

    /// Create a session client with a pre-built `reqwest::Client`.
    ///
    /// The session client keeps no handle on the jar behind `http`, so it
    /// cannot clear or inspect cookies: [`Self::cookie_header`] returns
    /// `None`, and clearing an expired session's cookies and the re-login
    /// path are inert. Use this for [`SessionAuth::ApiKey`]
    /// clients, whose credential is a header and which carry no cookie
    /// state, and for tests. A cookie-authenticated client must come from
    /// [`Self::new`], which always attaches a jar.
    pub fn with_client(
        http: reqwest::Client,
        base_url: Url,
        site: String,
        platform: ControllerPlatform,
        auth: SessionAuth,
    ) -> Self {
        Self {
            http,
            base_url,
            site,
            platform,
            auth,
            csrf_token: RwLock::new(None),
            cookie_jar: None,
            reauth: tokio::sync::Mutex::default(),
            session_generation: std::sync::atomic::AtomicU64::new(0),
            spent_totp: std::sync::Mutex::default(),
            request_timeout: None,
        }
    }

    /// Record the request timeout the `reqwest::Client` given to
    /// [`Self::with_client`] was built with, so timeouts report it.
    pub(crate) fn with_request_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.request_timeout = Some(timeout);
        self
    }

    /// Wrap a reqwest failure, reporting a timeout with the configured limit.
    pub(crate) fn transport_error(&self, error: reqwest::Error) -> Error {
        Error::from_transport(error, self.request_timeout)
    }

    /// Keep credentials so the client can re-authenticate itself when a
    /// request fails with [`Error::SessionExpired`]. Only meaningful for
    /// [`SessionAuth::Cookie`] clients.
    pub async fn enable_reauth(&self, credentials: ReauthCredentials) {
        let mut slot = self.reauth.lock().await;
        slot.credentials = Some(credentials);
    }

    /// Run `op`; if it fails because the session expired and credentials
    /// are available, log in again and run `op` once more.
    ///
    /// Replaying a request is only safe when the request is safe, so this is
    /// reserved for reads. State-changing verbs use
    /// [`Self::with_reauth_no_replay`]: a 401 is normally raised before the
    /// controller dispatches the operation, but nothing in the UniFi API
    /// contract guarantees it, and a replayed `POST` would create a second
    /// record or repeat a device command.
    async fn with_reauth<T, F, Fut>(&self, op: F) -> Result<T, Error>
    where
        F: Fn() -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<T, Error>> + Send,
    {
        let generation = self.session_generation();
        match op().await {
            Err(Error::SessionExpired) if self.try_reauthenticate(generation).await => op().await,
            result => result,
        }
    }

    fn session_generation(&self) -> u64 {
        self.session_generation
            .load(std::sync::atomic::Ordering::Acquire)
    }

    /// Record that a new session cookie is in place. Every successful login
    /// path calls this, so a 401 for the cookie it replaced reads as stale
    /// whether the new session came from re-auth or from a caller.
    pub(super) fn note_session_replaced(&self) {
        self.session_generation
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }

    /// Remember a TOTP code that a login has just sent.
    pub(super) fn note_totp_spent(&self, code: &secrecy::SecretString) {
        *self.spent_totp.lock().expect("TOTP lock poisoned") = Some(code.clone());
    }

    fn is_spent_totp(&self, code: &secrecy::SecretString) -> bool {
        use secrecy::ExposeSecret;
        self.spent_totp
            .lock()
            .expect("TOTP lock poisoned")
            .as_ref()
            .is_some_and(|spent| spent.expose_secret() == code.expose_secret())
    }

    /// Ask `provider` for a code to re-login with. Yields `None`, after a
    /// warning, when it has no code, returns something that is not a code,
    /// or repeats the code the last login sent, which is spent: sending any
    /// of those would fail while presenting the correct password.
    async fn reauth_totp(&self, provider: &TotpProvider) -> Option<secrecy::SecretString> {
        use secrecy::ExposeSecret;
        let retry_in_secs = REAUTH_COOLDOWN.as_secs();
        let Some(code) = provider.fresh_code().await else {
            warn!(
                retry_in_secs,
                "session expired but the TOTP provider gave no code"
            );
            return None;
        };
        if !is_totp_code(code.expose_secret()) {
            warn!(
                retry_in_secs,
                "TOTP provider returned something other than 6 digits"
            );
            return None;
        }
        if self.is_spent_totp(&code) {
            warn!(
                retry_in_secs,
                "TOTP provider repeated the code the last login used; waiting for a fresh one"
            );
            return None;
        }
        Some(code)
    }

    /// Attempt a re-login on behalf of a request sent at `observed`
    /// generation. Serialised through a mutex so concurrent callers that hit
    /// the same expiry share one login; rate-limited after failure.
    ///
    /// Returns `true` when the session is usable again, either because this
    /// call logged in or because another caller already did after `observed`.
    async fn try_reauthenticate(&self, observed: u64) -> bool {
        if self.auth != SessionAuth::Cookie {
            debug!("session expired on an API-key client; nothing to re-authenticate");
            return false;
        }
        let mut slot = self.reauth.lock().await;
        let ReauthSlot {
            credentials: retained,
            last_attempt,
        } = &mut *slot;
        // Debug only: whatever left the slot empty (a fixed TOTP code, or an
        // MFA challenge with no provider) already said so once, and this
        // runs for every request that meets the expired session.
        let Some(credentials) = retained.as_ref() else {
            debug!("session expired but no credentials were retained for re-login");
            return false;
        };
        // The 401 came back for a cookie that has since been replaced. It
        // says nothing about the new session, so retry on that instead of
        // clearing a cookie that works.
        if self.session_generation() != observed {
            return true;
        }
        let now = std::time::Instant::now();
        if last_attempt.is_some_and(|at| now.duration_since(at) < REAUTH_COOLDOWN) {
            debug!("session expired; re-login attempted recently, not retrying yet");
            return false;
        }
        // Provisional, so a future cancelled mid-login still leaves the
        // cooldown armed. Every exit below overwrites it with the instant
        // the attempt actually finished: a login is a network round trip,
        // and timing the windows from before it would let a slow login
        // expire its own cooldown.
        *last_attempt = Some(now);
        // An MFA account gets its code before anything is sent: without
        // one the login cannot complete, so it is not attempted at all.
        let totp = if let Some(provider) = credentials.totp_provider.as_ref() {
            let Some(code) = self.reauth_totp(provider).await else {
                *last_attempt = Some(std::time::Instant::now());
                return false;
            };
            Some(code)
        } else {
            None
        };
        info!("session expired; re-authenticating");
        // Send the login without the dead cookie attached. UniFi OS treats
        // a request that still carries an expired TOKEN differently from a
        // clean login, and a stale cookie left in the jar would shadow the
        // new one.
        let stale = self.cookie_header();
        let generation_before_login = self.session_generation();
        self.clear_session_cookies();
        match self
            .login(&credentials.username, &credentials.password, totp.as_ref())
            .await
        {
            Ok(()) => {
                let fresh = self.cookie_header();
                if self.cookie_jar.is_some() && (fresh.is_none() || fresh == stale) {
                    warn!(
                        got_cookie = fresh.is_some(),
                        retry_in_secs = REAUTH_COOLDOWN.as_secs(),
                        "login succeeded but returned no new session cookie; \
                         re-authentication did not take"
                    );
                    // `login` already counted this as a new session. Take that
                    // back so callers queued on this mutex do not read it as a
                    // successful renewal; only our own bump is undone.
                    let _ = self.session_generation.compare_exchange(
                        generation_before_login + 1,
                        generation_before_login,
                        std::sync::atomic::Ordering::AcqRel,
                        std::sync::atomic::Ordering::Acquire,
                    );
                    *last_attempt = Some(std::time::Instant::now());
                    return false;
                }
                if let Some(cache) = credentials.cache.as_ref() {
                    self.cache_current_session(cache);
                }
                // The cooldown exists to throttle *failed* re-logins. Leaving
                // the attempt timestamp set would block a genuine second
                // expiry that lands inside the cooldown window.
                *last_attempt = None;
                info!("session re-authenticated");
                true
            }
            // The account wants an MFA code and nothing can supply one, so
            // every later attempt would end the same way. Stop trying rather
            // than meet the challenge again every cooldown, indefinitely.
            Err(Error::TwoFactorRequired) if credentials.totp_provider.is_none() => {
                warn!(
                    "session expired and re-login needs an MFA code, but no TOTP \
                     provider is configured; this session can no longer renew itself"
                );
                *retained = None;
                *last_attempt = Some(std::time::Instant::now());
                false
            }
            Err(error) => {
                warn!(
                    %error,
                    retry_in_secs = REAUTH_COOLDOWN.as_secs(),
                    "session re-authentication failed"
                );
                *last_attempt = Some(std::time::Instant::now());
                false
            }
        }
    }

    /// Run `op` once. If the session had expired, re-authenticate so the
    /// *next* request succeeds, but surface the error instead of replaying a
    /// request that the controller may already have applied.
    async fn with_reauth_no_replay<T, F, Fut>(&self, op: F) -> Result<T, Error>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, Error>>,
    {
        let generation = self.session_generation();
        match op().await {
            Err(Error::SessionExpired) => {
                if self.try_reauthenticate(generation).await {
                    warn!(
                        "session expired during a state-changing request; \
                         re-authenticated but did not replay it -- retry the operation"
                    );
                }
                Err(Error::SessionExpired)
            }
            other => other,
        }
    }

    /// The authentication method used by this client.
    pub fn auth(&self) -> SessionAuth {
        self.auth
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

    /// Remove every cookie currently held for the controller by writing an
    /// already-expired replacement for each name. No-op without a jar.
    fn clear_session_cookies(&self) {
        let Some(jar) = self.cookie_jar.as_ref() else {
            return;
        };
        let Some(header) = self.cookie_header() else {
            return;
        };
        let names: Vec<String> = header
            .split(';')
            .filter_map(|pair| pair.trim().split_once('=').map(|(name, _)| name.to_owned()))
            .collect();
        for name in names {
            let expired = format!("{name}=; Path=/; Max-Age=0");
            if let Ok(value) = expired.parse::<reqwest::header::HeaderValue>() {
                jar.set_cookies(&mut std::iter::once(&value), &self.base_url);
            }
        }
        debug!("cleared stale session cookies before re-login");
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

    // ── Cookie injection (for MFA flow) ───────────────────────────────

    /// Inject a `Set-Cookie` header value into the client's cookie jar.
    ///
    /// Used by the MFA flow to inject the `UBIC_2FA` cookie before retrying
    /// login with the TOTP token.
    pub(crate) fn add_cookie(&self, set_cookie_value: &str, url: &Url) -> Result<(), Error> {
        let jar = self
            .cookie_jar
            .as_ref()
            .ok_or_else(|| Error::Authentication {
                message: "no cookie jar available for MFA flow".into(),
            })?;
        let header_value: reqwest::header::HeaderValue =
            set_cookie_value
                .parse()
                .map_err(|_| Error::Authentication {
                    message: "failed to parse MFA cookie value".into(),
                })?;
        jar.set_cookies(&mut std::iter::once(&header_value), url);
        Ok(())
    }

    // ── CSRF token management ─────────────────────────────────────────

    /// Read the current CSRF token value (for session caching).
    pub(crate) fn csrf_token_value(&self) -> Option<String> {
        self.csrf_token.read().expect("CSRF lock poisoned").clone()
    }

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

    /// Classify a session 401 based on the active auth strategy.
    ///
    /// Cookie-backed session clients surface 401s as an expired session.
    /// API-key clients surface the same status as a rejected key.
    fn unauthorized_error(&self) -> Error {
        match self.auth {
            SessionAuth::Cookie => Error::SessionExpired,
            SessionAuth::ApiKey => Error::InvalidApiKey,
        }
    }

    // ── URL builders ─────────────────────────────────────────────────

    /// Build a full URL for a controller-level API path.
    ///
    /// Applies the platform-specific session prefix, then appends `/api/{path}`.
    /// For example, on UniFi OS: `https://host/proxy/network/api/{path}`
    pub(crate) fn api_url(&self, path: &str) -> Url {
        let prefix = self.platform.session_prefix().unwrap_or("");
        let base = self.base_url.as_str().trim_end_matches('/');
        let prefix = prefix.trim_end_matches('/');
        let full = format!("{base}{prefix}/api/{path}");
        Url::parse(&full).expect("invalid API URL")
    }

    /// Build a site-scoped URL: `{base}{prefix}/api/s/{site}/{path}`
    ///
    /// Most session endpoints are site-scoped: stat/device, cmd/devmgr, etc.
    pub(crate) fn site_url(&self, path: &str) -> Url {
        let prefix = self.platform.session_prefix().unwrap_or("");
        let base = self.base_url.as_str().trim_end_matches('/');
        let prefix = prefix.trim_end_matches('/');
        let full = format!("{base}{prefix}/api/s/{}/{path}", self.site);
        Url::parse(&full).expect("invalid site URL")
    }

    /// Build a v2 site-scoped URL: `{base}{prefix}/v2/api/site/{site}/{path}`
    ///
    /// Used by newer endpoints (Network Application 9+) that use the v2 path
    /// format, e.g. traffic-flow-latest-statistics.
    pub(crate) fn site_url_v2(&self, path: &str) -> Url {
        let prefix = self.platform.session_prefix().unwrap_or("");
        let base = self.base_url.as_str().trim_end_matches('/');
        let prefix = prefix.trim_end_matches('/');
        let full = format!("{base}{prefix}/v2/api/site/{}/{path}", self.site);
        Url::parse(&full).expect("invalid v2 site URL")
    }

    // ── Request helpers ──────────────────────────────────────────────

    /// Send a GET request and unwrap the session envelope.
    pub(crate) async fn get<T: DeserializeOwned>(&self, url: Url) -> Result<Vec<T>, Error> {
        self.with_reauth(|| self.get_once(url.clone())).await
    }

    /// One GET attempt; [`Self::get`] wraps it in [`Self::with_reauth`].
    async fn get_once<T: DeserializeOwned>(&self, url: Url) -> Result<Vec<T>, Error> {
        debug!("GET {}", url);

        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| self.transport_error(e))?;

        self.parse_envelope(resp).await
    }

    /// Send a GET request and return the raw JSON response (no envelope unwrapping).
    ///
    /// Used for v2 API endpoints that return plain JSON instead of the
    /// session `{ meta, data }` envelope.
    pub(crate) async fn get_raw(&self, url: Url) -> Result<serde_json::Value, Error> {
        self.with_reauth(|| self.get_raw_once(url.clone())).await
    }

    /// One raw GET attempt; [`Self::get_raw`] adds the re-auth retry.
    async fn get_raw_once(&self, url: Url) -> Result<serde_json::Value, Error> {
        debug!("GET (raw) {}", url);

        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| self.transport_error(e))?;
        let body = self.checked_raw_body(resp).await?;
        decode_raw_json(&body)
    }

    /// Send a POST request with JSON body and unwrap the session envelope.
    pub(crate) async fn post<T: DeserializeOwned>(
        &self,
        url: Url,
        body: &(impl Serialize + Sync),
    ) -> Result<Vec<T>, Error> {
        self.with_reauth_no_replay(|| self.post_once(url.clone(), body))
            .await
    }

    /// One POST attempt; [`Self::post`] re-authenticates without replaying.
    async fn post_once<T: DeserializeOwned>(
        &self,
        url: Url,
        body: &(impl Serialize + Sync),
    ) -> Result<Vec<T>, Error> {
        debug!("POST {}", url);

        let builder = self.apply_csrf(self.http.post(url).json(body));
        let resp = builder.send().await.map_err(|e| self.transport_error(e))?;

        self.parse_envelope(resp).await
    }

    /// Send a PUT request with JSON body and unwrap the session envelope.
    #[allow(dead_code)]
    pub(crate) async fn put<T: DeserializeOwned>(
        &self,
        url: Url,
        body: &(impl Serialize + Sync),
    ) -> Result<Vec<T>, Error> {
        self.with_reauth_no_replay(|| self.put_once(url.clone(), body))
            .await
    }

    /// One PUT attempt; [`Self::put`] re-authenticates without replaying.
    async fn put_once<T: DeserializeOwned>(
        &self,
        url: Url,
        body: &(impl Serialize + Sync),
    ) -> Result<Vec<T>, Error> {
        debug!("PUT {}", url);

        let builder = self.apply_csrf(self.http.put(url).json(body));
        let resp = builder.send().await.map_err(|e| self.transport_error(e))?;

        self.parse_envelope(resp).await
    }

    /// Send a DELETE request and unwrap the session envelope.
    #[allow(dead_code)]
    pub(crate) async fn delete<T: DeserializeOwned>(&self, url: Url) -> Result<Vec<T>, Error> {
        self.with_reauth_no_replay(|| self.delete_once(url.clone()))
            .await
    }

    /// One DELETE attempt; [`Self::delete`] re-authenticates without replaying.
    async fn delete_once<T: DeserializeOwned>(&self, url: Url) -> Result<Vec<T>, Error> {
        debug!("DELETE {}", url);

        let builder = self.apply_csrf(self.http.delete(url));
        let resp = builder.send().await.map_err(|e| self.transport_error(e))?;

        self.parse_envelope(resp).await
    }

    /// Send a raw GET to an arbitrary path (no envelope unwrapping).
    ///
    /// The `path` is appended directly after `{base}{prefix}/`.
    pub async fn raw_get(&self, path: &str) -> Result<serde_json::Value, Error> {
        let prefix = self.platform.session_prefix().unwrap_or("");
        let base = self.base_url.as_str().trim_end_matches('/');
        let prefix = prefix.trim_end_matches('/');
        let url = Url::parse(&format!("{base}{prefix}/{path}")).expect("invalid raw URL");
        self.get_raw(url).await
    }

    /// Send a raw POST to an arbitrary path (no envelope unwrapping).
    pub async fn raw_post(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, Error> {
        self.with_reauth_no_replay(|| self.raw_post_once(path, body))
            .await
    }

    /// One raw POST attempt; [`Self::raw_post`] re-authenticates without replaying.
    async fn raw_post_once(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, Error> {
        let prefix = self.platform.session_prefix().unwrap_or("");
        let base = self.base_url.as_str().trim_end_matches('/');
        let prefix = prefix.trim_end_matches('/');
        let url = Url::parse(&format!("{base}{prefix}/{path}")).expect("invalid raw URL");
        debug!("POST (raw) {}", url);

        let builder = self.apply_csrf(self.http.post(url).json(body));
        let resp = builder.send().await.map_err(|e| self.transport_error(e))?;
        let body = self.checked_raw_body(resp).await?;
        decode_raw_json(&body)
    }

    /// Send a raw PUT to an arbitrary path (no envelope unwrapping).
    pub async fn raw_put(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, Error> {
        self.with_reauth_no_replay(|| self.raw_put_once(path, body))
            .await
    }

    /// One raw PUT attempt; [`Self::raw_put`] re-authenticates without replaying.
    async fn raw_put_once(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, Error> {
        let prefix = self.platform.session_prefix().unwrap_or("");
        let base = self.base_url.as_str().trim_end_matches('/');
        let prefix = prefix.trim_end_matches('/');
        let url = Url::parse(&format!("{base}{prefix}/{path}")).expect("invalid raw URL");
        debug!("PUT (raw) {}", url);

        let builder = self.apply_csrf(self.http.put(url).json(body));
        let resp = builder.send().await.map_err(|e| self.transport_error(e))?;
        let body = self.checked_raw_body(resp).await?;
        decode_raw_json(&body)
    }

    /// Send a raw PATCH to an arbitrary path (no envelope unwrapping).
    pub async fn raw_patch(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, Error> {
        self.with_reauth_no_replay(|| self.raw_patch_once(path, body))
            .await
    }

    /// One raw PATCH attempt; [`Self::raw_patch`] re-authenticates without replaying.
    async fn raw_patch_once(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, Error> {
        let prefix = self.platform.session_prefix().unwrap_or("");
        let base = self.base_url.as_str().trim_end_matches('/');
        let prefix = prefix.trim_end_matches('/');
        let url = Url::parse(&format!("{base}{prefix}/{path}")).expect("invalid raw URL");
        debug!("PATCH (raw) {}", url);

        let builder = self.apply_csrf(self.http.patch(url).json(body));
        let resp = builder.send().await.map_err(|e| self.transport_error(e))?;
        let body = self.checked_raw_body(resp).await?;
        decode_raw_json(&body)
    }

    /// Send a raw DELETE to an arbitrary path (no envelope unwrapping).
    pub async fn raw_delete(&self, path: &str) -> Result<(), Error> {
        self.with_reauth_no_replay(|| self.raw_delete_once(path))
            .await
    }

    /// One raw DELETE attempt; [`Self::raw_delete`] re-authenticates without replaying.
    async fn raw_delete_once(&self, path: &str) -> Result<(), Error> {
        let prefix = self.platform.session_prefix().unwrap_or("");
        let base = self.base_url.as_str().trim_end_matches('/');
        let prefix = prefix.trim_end_matches('/');
        let url = Url::parse(&format!("{base}{prefix}/{path}")).expect("invalid raw URL");
        debug!("DELETE (raw) {}", url);

        let builder = self.apply_csrf(self.http.delete(url));
        let resp = builder.send().await.map_err(|e| self.transport_error(e))?;
        self.checked_raw_body(resp).await?;
        Ok(())
    }

    /// Check a response from a raw (envelope-free) route and return its
    /// body: a 401, a non-2xx status, or a UniFi OS error wrapped in a 200
    /// is an error here just as it is for enveloped routes, and a rotated
    /// CSRF token is captured the same way.
    async fn checked_raw_body(&self, resp: reqwest::Response) -> Result<Vec<u8>, Error> {
        let status = resp.status();
        self.update_csrf_from_response(resp.headers());

        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(self.unauthorized_error());
        }
        if !status.is_success() {
            let body = resp.bytes().await.unwrap_or_default();
            return Err(Error::SessionApi {
                message: format!("HTTP {status}: {}", body_preview(&body)),
            });
        }

        let body = resp.bytes().await.map_err(|e| self.transport_error(e))?;
        if let Some(error) = self.unifi_os_error(&body) {
            return Err(error);
        }
        Ok(body.into())
    }

    /// The error in a UniFi OS `{"error":{"code":N,"message":"..."}}` body,
    /// which UniFi OS sometimes returns with HTTP 200.
    fn unifi_os_error(&self, body: &[u8]) -> Option<Error> {
        // Only an object can be that wrapper. Serde would also read a
        // one-element array as the struct, mistaking `[{"code": 1}]` (a
        // raw route's ordinary list) for an error.
        if body.trim_ascii_start().first() != Some(&b'{') {
            return None;
        }
        let err = serde_json::from_slice::<UnifiOsError>(body).ok()?.error?;
        let msg = err.message.unwrap_or_default();
        Some(if err.code == 401 {
            if msg.is_empty() {
                self.unauthorized_error()
            } else {
                match self.unauthorized_error() {
                    Error::SessionExpired => {
                        debug!(%msg, "controller reports the session expired");
                        Error::SessionExpired
                    }
                    Error::InvalidApiKey => Error::Authentication {
                        message: format!("API key rejected: {msg}"),
                    },
                    other => other,
                }
            }
        } else {
            Error::SessionApi {
                message: format!("UniFi OS error {}: {msg}", err.code),
            }
        })
    }

    /// Parse the `{ meta, data }` envelope, returning `data` on success
    /// or an `Error::SessionApi` if `meta.rc != "ok"`.
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
            return Err(self.unauthorized_error());
        }

        if status == reqwest::StatusCode::FORBIDDEN {
            return Err(Error::SessionApi {
                message: "insufficient permissions (HTTP 403)".into(),
            });
        }

        if !status.is_success() {
            let body = resp.bytes().await.unwrap_or_default();
            return Err(Error::SessionApi {
                message: format!("HTTP {status}: {}", body_preview(&body)),
            });
        }

        let body = resp.bytes().await.map_err(|e| self.transport_error(e))?;

        if let Some(error) = self.unifi_os_error(&body) {
            return Err(error);
        }

        let envelope: SessionResponse<serde_json::Value> =
            serde_json::from_slice(&body).map_err(|e| {
                let preview = body_preview(&body);
                Error::Deserialization {
                    message: format!("{e} (body preview: {preview:?})"),
                    body: String::from_utf8_lossy(&body).into_owned(),
                }
            })?;

        if envelope.meta.rc != "ok" {
            return Err(Error::SessionApi {
                message: envelope
                    .meta
                    .msg
                    .unwrap_or_else(|| format!("rc={}", envelope.meta.rc)),
            });
        }

        // Decode per record so one unexpected field shape (UniFi is fond of
        // `"auto"` where a number is documented) drops that record with a
        // warning instead of blanking the whole collection. If every record
        // fails, surface the first error so single-item lookups still report
        // a real deserialization problem rather than "not found".
        let total = envelope.data.len();
        let mut items = Vec::with_capacity(total);
        let mut first_error = None;
        for (index, value) in envelope.data.into_iter().enumerate() {
            match serde_json::from_value::<T>(value) {
                Ok(item) => items.push(item),
                Err(error) => {
                    warn!(index, %error, "skipping session record that failed to decode");
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }

        if items.is_empty()
            && let Some(error) = first_error
        {
            return Err(Error::Deserialization {
                message: format!("{error} (all {total} records failed to decode)"),
                body: String::from_utf8_lossy(&body).into_owned(),
            });
        }

        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::{ReauthCredentials, SessionAuth, SessionClient, TotpProvider};
    use crate::{ControllerPlatform, Error};

    fn client(auth: SessionAuth) -> SessionClient {
        crate::transport::ensure_crypto_provider();
        SessionClient::with_client(
            reqwest::Client::new(),
            Url::parse("https://controller.example").expect("valid test URL"),
            "default".into(),
            ControllerPlatform::ClassicController,
            auth,
        )
    }

    #[test]
    fn unauthorized_cookie_client_reports_session_expired() {
        assert!(matches!(
            client(SessionAuth::Cookie).unauthorized_error(),
            Error::SessionExpired
        ));
    }

    #[test]
    fn unauthorized_api_key_client_reports_invalid_api_key() {
        assert!(matches!(
            client(SessionAuth::ApiKey).unauthorized_error(),
            Error::InvalidApiKey
        ));
    }

    /// A second expiry shortly after a *successful* re-login must trigger
    /// another login. The cooldown is meant to throttle failed re-logins
    /// only; leaving `last_attempt` set after success locked the client out
    /// for the remainder of the 30 s window with valid credentials.
    #[tokio::test]
    async fn successful_relogin_does_not_arm_the_failure_cooldown() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        crate::transport::ensure_crypto_provider();
        let server = MockServer::start().await;
        let client = SessionClient::with_client(
            reqwest::Client::new(),
            Url::parse(&server.uri()).expect("mock server URL is valid"),
            "default".into(),
            ControllerPlatform::ClassicController,
            SessionAuth::Cookie,
        );
        client
            .enable_reauth(ReauthCredentials {
                username: "admin".into(),
                password: secrecy::SecretString::from("pw".to_string()),
                totp_provider: None,
                cache: None,
            })
            .await;

        let ok =
            || ResponseTemplate::new(200).set_body_json(serde_json::json!({"meta": {"rc": "ok"}}));
        // Expire, recover, then expire a second time.
        for status in [401u16, 200, 401, 200] {
            let response = if status == 200 {
                ok()
            } else {
                ResponseTemplate::new(401)
            };
            Mock::given(method("GET"))
                .and(path("/api/s/default/stat/health"))
                .respond_with(response)
                .up_to_n_times(1)
                .expect(1)
                .mount(&server)
                .await;
        }
        Mock::given(method("POST"))
            .and(path("/api/login"))
            .respond_with(ok())
            .expect(2)
            .mount(&server)
            .await;

        client
            .raw_get("api/s/default/stat/health")
            .await
            .expect("first call recovers via re-login");

        // Immediately after the first recovery, well inside REAUTH_COOLDOWN.
        client
            .raw_get("api/s/default/stat/health")
            .await
            .expect("second expiry must re-login rather than hit the cooldown");

        server.verify().await;
    }

    /// The failure cooldown must be timed from when a login *finished*, not
    /// from before it started. A login is a network round trip; timing from
    /// the start would let a slow failed login outlive its own cooldown.
    #[tokio::test]
    async fn failed_relogin_cooldown_starts_after_the_login_completes() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        const LOGIN_LATENCY: std::time::Duration = std::time::Duration::from_millis(150);

        crate::transport::ensure_crypto_provider();
        let server = MockServer::start().await;
        let client = SessionClient::with_client(
            reqwest::Client::new(),
            Url::parse(&server.uri()).expect("mock server URL is valid"),
            "default".into(),
            ControllerPlatform::ClassicController,
            SessionAuth::Cookie,
        );
        client
            .enable_reauth(ReauthCredentials {
                username: "admin".into(),
                password: secrecy::SecretString::from("wrong".to_string()),
                totp_provider: None,
                cache: None,
            })
            .await;

        Mock::given(method("GET"))
            .and(path("/api/s/default/stat/health"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/login"))
            .respond_with(ResponseTemplate::new(401).set_delay(LOGIN_LATENCY))
            .expect(1)
            .mount(&server)
            .await;

        let before = std::time::Instant::now();
        client
            .raw_get("api/s/default/stat/health")
            .await
            .expect_err("a failed re-login surfaces the expiry");

        let slot = client.reauth.lock().await;
        let recorded = slot
            .last_attempt
            .expect("a failed re-login arms the cooldown");
        assert!(
            recorded.duration_since(before) >= LOGIN_LATENCY,
            "last_attempt was stamped before the login finished: {:?} elapsed, \
             login took at least {LOGIN_LATENCY:?}",
            recorded.duration_since(before),
        );
    }

    /// A login that answers 200 but sets no new cookie is a failed renewal.
    /// Requests queued behind it must see the failure, not a generation bump
    /// that reads as "someone else already renewed".
    #[tokio::test]
    async fn cookieless_relogin_does_not_advance_the_generation() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let base_url = Url::parse(&server.uri()).expect("mock server URL is valid");
        let transport = crate::transport::TransportConfig::default().with_cookie_jar();
        if let Some(jar) = transport.cookie_jar.as_ref() {
            jar.add_cookie_str("TOKEN=stale", &base_url);
        }
        let client = SessionClient::new(
            base_url,
            "default".into(),
            ControllerPlatform::ClassicController,
            &transport,
        )
        .expect("session client builds");
        client
            .enable_reauth(ReauthCredentials {
                username: "admin".into(),
                password: secrecy::SecretString::from("pw".to_string()),
                totp_provider: None,
                cache: None,
            })
            .await;

        Mock::given(method("GET"))
            .and(path("/api/s/default/stat/health"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/login"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"meta": {"rc": "ok"}})),
            )
            .expect(1)
            .mount(&server)
            .await;

        client
            .raw_get("api/s/default/stat/health")
            .await
            .expect_err("a renewal without a cookie surfaces the expiry");

        assert_eq!(client.session_generation(), 0);
        // A request that was waiting on the same expiry now hits the
        // cooldown instead of being told the session was renewed.
        assert!(!client.try_reauthenticate(0).await);

        server.verify().await;
    }

    /// A 401 for a cookie that another caller has already replaced must not
    /// clear the working cookie or start a second login. This is the late
    /// response from a request that went out just before a re-login landed.
    #[tokio::test]
    async fn stale_expiry_after_a_renewal_reuses_the_new_session() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let transport = crate::transport::TransportConfig::default().with_cookie_jar();
        let client = SessionClient::new(
            Url::parse(&server.uri()).expect("mock server URL is valid"),
            "default".into(),
            ControllerPlatform::ClassicController,
            &transport,
        )
        .expect("session client builds");
        client
            .enable_reauth(ReauthCredentials {
                username: "admin".into(),
                password: secrecy::SecretString::from("pw".to_string()),
                totp_provider: None,
                cache: None,
            })
            .await;

        Mock::given(method("GET"))
            .and(path("/api/s/default/stat/health"))
            .respond_with(ResponseTemplate::new(401))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/s/default/stat/health"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"meta": {"rc": "ok"}})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/login"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("set-cookie", "TOKEN=fresh; Path=/")
                    .set_body_json(serde_json::json!({"meta": {"rc": "ok"}})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let stale_generation = client.session_generation();
        client
            .raw_get("api/s/default/stat/health")
            .await
            .expect("first expiry recovers via re-login");

        // A request sent before that re-login now reports its 401.
        assert!(client.try_reauthenticate(stale_generation).await);
        assert_eq!(client.cookie_header().as_deref(), Some("TOKEN=fresh"));

        server.verify().await;
    }

    /// A TOTP provider with no code to give means an MFA login cannot
    /// complete, so no login is sent at all. The attempt still counts as a
    /// failure: the cooldown keeps the next expired request from asking
    /// the provider again straight away.
    #[tokio::test]
    async fn provider_without_a_code_sends_no_login_and_arms_the_cooldown() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        crate::transport::ensure_crypto_provider();
        let server = MockServer::start().await;
        let client = SessionClient::with_client(
            reqwest::Client::new(),
            Url::parse(&server.uri()).expect("mock server URL is valid"),
            "default".into(),
            ControllerPlatform::ClassicController,
            SessionAuth::Cookie,
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        client
            .enable_reauth(ReauthCredentials {
                username: "admin".into(),
                password: secrecy::SecretString::from("pw".to_string()),
                totp_provider: Some(TotpProvider::new(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                    None
                })),
                cache: None,
            })
            .await;

        Mock::given(method("GET"))
            .and(path("/api/s/default/stat/health"))
            .respond_with(ResponseTemplate::new(401))
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/login"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;

        for _ in 0..2 {
            let error = client
                .raw_get("api/s/default/stat/health")
                .await
                .expect_err("without a code the expiry surfaces");
            assert!(matches!(error, Error::SessionExpired), "got {error:?}");
        }

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "second expiry hit the cooldown"
        );
        assert!(
            client.reauth.lock().await.last_attempt.is_some(),
            "a provider with no code arms the cooldown"
        );
        server.verify().await;
    }

    /// A re-login that meets an MFA challenge with no TOTP provider can
    /// never succeed, so the retained credentials are dropped instead of
    /// meeting the challenge again after every cooldown.
    #[tokio::test]
    async fn mfa_challenge_without_a_provider_drops_the_credentials() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        crate::transport::ensure_crypto_provider();
        let server = MockServer::start().await;
        let client = SessionClient::with_client(
            reqwest::Client::new(),
            Url::parse(&server.uri()).expect("mock server URL is valid"),
            "default".into(),
            ControllerPlatform::ClassicController,
            SessionAuth::Cookie,
        );
        client
            .enable_reauth(ReauthCredentials {
                username: "admin".into(),
                password: secrecy::SecretString::from("pw".to_string()),
                totp_provider: None,
                cache: None,
            })
            .await;

        Mock::given(method("GET"))
            .and(path("/api/s/default/stat/health"))
            .respond_with(ResponseTemplate::new(401))
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/login"))
            .respond_with(ResponseTemplate::new(499).set_body_json(serde_json::json!({
                "errors": ["ubic_2fa_token required"],
                "token": "UBIC_2FA=challenge"
            })))
            .expect(1)
            .mount(&server)
            .await;

        client
            .raw_get("api/s/default/stat/health")
            .await
            .expect_err("an unanswerable MFA challenge surfaces the expiry");
        {
            let mut slot = client.reauth.lock().await;
            assert!(slot.credentials.is_none(), "credentials were kept");
            // Even once the cooldown has passed, nothing is left to retry.
            slot.last_attempt = None;
        }
        client
            .raw_get("api/s/default/stat/health")
            .await
            .expect_err("the session stays expired");

        server.verify().await;
    }

    /// A state-changing request must never be replayed after an expiry. A
    /// 401 is normally raised before the controller dispatches the write,
    /// but the API makes no such guarantee, and a replayed POST would create
    /// a second record. The client re-authenticates so the next call works,
    /// then surfaces the error.
    #[tokio::test]
    async fn expired_mutation_reauthenticates_without_replaying() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        crate::transport::ensure_crypto_provider();
        let server = MockServer::start().await;
        let client = SessionClient::with_client(
            reqwest::Client::new(),
            Url::parse(&server.uri()).expect("mock server URL is valid"),
            "default".into(),
            ControllerPlatform::ClassicController,
            SessionAuth::Cookie,
        );
        client
            .enable_reauth(ReauthCredentials {
                username: "admin".into(),
                password: secrecy::SecretString::from("pw".to_string()),
                totp_provider: None,
                cache: None,
            })
            .await;

        // The write is attempted exactly once, never a second time.
        Mock::given(method("POST"))
            .and(path("/api/s/default/rest/firewallgroup"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;
        // The re-login still happens, so the next request would succeed.
        Mock::given(method("POST"))
            .and(path("/api/login"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"meta": {"rc": "ok"}})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let error = client
            .raw_post(
                "api/s/default/rest/firewallgroup",
                &serde_json::json!({"name": "test"}),
            )
            .await
            .expect_err("an expired mutation must surface the error");
        assert!(matches!(error, Error::SessionExpired));

        server.verify().await;
    }
}
