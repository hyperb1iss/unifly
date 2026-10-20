// Session API authentication
//
// Cookie-based session login/logout and controller platform detection.
// The login endpoint sets a session cookie in the client's jar;
// subsequent requests use that cookie automatically.
//
// MFA flow: when the controller requires 2FA, login returns HTTP 499
// with `{"errors":["ubic_2fa_token required"]}` and a `token` field
// containing a Set-Cookie value for `UBIC_2FA`. We inject that cookie,
// then retry the login POST with `ubic_2fa_token` in the body.

use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use tracing::debug;
use url::Url;

use crate::auth::ControllerPlatform;
use crate::error::Error;
use crate::session::client::SessionClient;
use crate::session::totp::{TotpSource, is_totp_code};
use crate::transport::TransportConfig;

/// Response body from a 499 MFA challenge.
#[derive(serde::Deserialize)]
struct MfaChallengeResponse {
    /// Set-Cookie value for `UBIC_2FA` (e.g. `UBIC_2FA=eyJ...`).
    token: Option<String>,
}

impl SessionClient {
    /// Authenticate with the controller using username/password.
    ///
    /// If the controller has MFA enabled it returns HTTP 499. When a
    /// `totp_token` is provided, we complete the two-step challenge
    /// automatically. Without one, `Error::TwoFactorRequired` is returned
    /// so the caller can prompt the user.
    pub async fn login(
        &self,
        username: &str,
        password: &SecretString,
        totp_token: Option<&SecretString>,
    ) -> Result<(), Error> {
        self.login_with_totp(username, password, TotpSource::new(totp_token, None))
            .await
    }

    /// [`Self::login`] with the TOTP code taken from `totp`, which is only
    /// consulted if the controller issues an MFA challenge.
    pub(crate) async fn login_with_totp(
        &self,
        username: &str,
        password: &SecretString,
        totp: TotpSource<'_>,
    ) -> Result<(), Error> {
        let login_path = self
            .platform()
            .login_path()
            .ok_or_else(|| Error::Authentication {
                message: "login not supported on cloud platform".into(),
            })?;

        let url = self
            .base_url()
            .join(login_path)
            .map_err(Error::InvalidUrl)?;

        debug!("logging in at {}", url);

        let body = json!({
            "username": username,
            "password": password.expose_secret(),
        });

        let resp = self
            .http()
            .post(url.clone())
            .json(&body)
            .send()
            .await
            .map_err(|e| self.transport_error(e))?;

        let status = resp.status();

        // HTTP 499 = MFA challenge from UniFi SSO. Boxed because the second
        // round trip (plus a possible provider call) would otherwise
        // inflate every login future, and with it every connect future,
        // for a path taken at most once per login.
        if status.as_u16() == 499 {
            return Box::pin(self.handle_mfa_challenge(resp, &url, username, password, totp)).await;
        }

        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Authentication {
                message: format!("login failed (HTTP {status}): {body}"),
            });
        }

        self.capture_csrf(&resp);
        self.note_session_replaced();
        debug!("login successful");
        Ok(())
    }

    /// Handle a 499 MFA challenge: inject the UBIC_2FA cookie and retry
    /// with the TOTP token in the login body.
    async fn handle_mfa_challenge(
        &self,
        resp: reqwest::Response,
        login_url: &Url,
        username: &str,
        password: &SecretString,
        totp: TotpSource<'_>,
    ) -> Result<(), Error> {
        // Read the challenge before asking for a code: a provider can take
        // a while (a password manager prompt), and the unread response
        // would hold its connection the whole time.
        let challenge = resp.json::<MfaChallengeResponse>().await;

        let totp = totp.resolve().await.ok_or(Error::TwoFactorRequired)?;
        let code = totp.expose_secret();

        // Validate TOTP format before sending
        if !is_totp_code(code) {
            return Err(Error::Authentication {
                message: "TOTP token must be exactly 6 digits".into(),
            });
        }

        debug!("MFA challenge received — completing 2FA handshake");

        // Extract the MFA cookie from the challenge response
        let challenge = challenge.map_err(|_| Error::Authentication {
            message: "failed to parse MFA challenge response".into(),
        })?;

        if let Some(cookie_value) = challenge.token {
            if !cookie_value.starts_with("UBIC_2FA=") {
                return Err(Error::Authentication {
                    message: format!(
                        "unexpected MFA cookie format (expected UBIC_2FA=...): {cookie_value}"
                    ),
                });
            }
            self.add_cookie(&cookie_value, login_url)?;
        }

        // Retry login with TOTP token. Once sent, the code is spent
        // whatever the outcome.
        self.note_totp_spent(&totp);
        let body = json!({
            "username": username,
            "password": password.expose_secret(),
            "ubic_2fa_token": code,
        });

        let resp = self
            .http()
            .post(login_url.clone())
            .json(&body)
            .send()
            .await
            .map_err(|e| self.transport_error(e))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Authentication {
                message: format!("MFA login failed (HTTP {status}): {body}"),
            });
        }

        self.capture_csrf(&resp);
        self.note_session_replaced();
        debug!("MFA login successful");
        Ok(())
    }

    /// Extract and store CSRF token from a login response.
    fn capture_csrf(&self, resp: &reqwest::Response) {
        if let Some(token) = resp
            .headers()
            .get("X-CSRF-Token")
            .or_else(|| resp.headers().get("x-csrf-token"))
            .and_then(|v| v.to_str().ok())
        {
            self.set_csrf_token(token.to_owned());
        }
    }

    /// Attempt to restore a session from cache, falling back to fresh login.
    ///
    /// If a cached session is available and the validation probe succeeds,
    /// the cookie and CSRF token are restored without hitting the login
    /// endpoint. Otherwise, performs a normal login and caches the result.
    pub async fn login_with_cache(
        &self,
        username: &str,
        password: &secrecy::SecretString,
        totp_token: Option<&secrecy::SecretString>,
        cache: &super::session_cache::SessionCache,
    ) -> Result<(), Error> {
        self.login_with_cache_and_totp(username, password, TotpSource::new(totp_token, None), cache)
            .await
    }

    /// [`Self::login_with_cache`] with the TOTP code taken from `totp`. A
    /// restored session never consults it, so a provider only runs when a
    /// fresh login actually meets an MFA challenge.
    pub(crate) async fn login_with_cache_and_totp(
        &self,
        username: &str,
        password: &secrecy::SecretString,
        totp: TotpSource<'_>,
        cache: &super::session_cache::SessionCache,
    ) -> Result<(), Error> {
        // Try cached session first
        if let Some((cookie, csrf)) = cache.load() {
            self.add_cookie(&cookie, self.base_url())?;
            if let Some(token) = csrf {
                self.set_csrf_token(token);
            }

            if self.validate_session().await {
                self.note_session_replaced();
                debug!("restored session from cache");
                return Ok(());
            }
            debug!("cached session invalid, performing fresh login");
        }

        // Fresh login
        self.login_with_totp(username, password, totp).await?;
        self.cache_current_session(cache);
        Ok(())
    }

    /// Persist the current cookie and CSRF token to `cache`, with an expiry
    /// derived from the cookie's JWT (minus a safety margin) when possible.
    pub(crate) fn cache_current_session(&self, cache: &super::session_cache::SessionCache) {
        use super::session_cache::{EXPIRY_MARGIN_SECS, fallback_expiry, jwt_expiry};

        if let Some(cookie) = self.cookie_header() {
            let csrf = self.csrf_token_value();
            let expires_at = jwt_expiry(&cookie).map_or_else(fallback_expiry, |exp| {
                exp.saturating_sub(EXPIRY_MARGIN_SECS)
            });
            cache.save(&cookie, csrf.as_deref(), expires_at);
        }
    }

    /// Validate the current session by probing a lightweight endpoint.
    ///
    /// Returns `true` if the session is still alive.
    async fn validate_session(&self) -> bool {
        let prefix = self.platform().session_prefix().unwrap_or("");
        let base = self.base_url().as_str().trim_end_matches('/');
        let prefix = prefix.trim_end_matches('/');
        let probe_url = format!("{base}{prefix}/api/s/{}/self", self.site());

        let Ok(url) = url::Url::parse(&probe_url) else {
            return false;
        };

        match self.http().get(url).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    /// End the current session.
    ///
    /// Platform-specific logout endpoint:
    /// - UniFi OS: `POST /api/auth/logout`
    /// - Standalone: `POST /api/logout`
    pub async fn logout(&self) -> Result<(), Error> {
        let logout_path = self
            .platform()
            .logout_path()
            .ok_or_else(|| Error::Authentication {
                message: "logout not supported on cloud platform".into(),
            })?;

        let url = self
            .base_url()
            .join(logout_path)
            .map_err(Error::InvalidUrl)?;

        debug!("logging out at {}", url);

        let _resp = self
            .http()
            .post(url)
            .send()
            .await
            .map_err(|e| self.transport_error(e))?;

        debug!("logout complete");
        Ok(())
    }

    /// Auto-detect the controller platform with the default transport,
    /// which accepts any certificate. Prefer
    /// [`Self::detect_platform_with_transport`], which honors the TLS mode
    /// and timeout the rest of the session will use.
    pub async fn detect_platform(base_url: &Url) -> Result<ControllerPlatform, Error> {
        Self::detect_platform_with_transport(base_url, &TransportConfig::default()).await
    }

    /// Auto-detect the controller platform by probing login endpoints,
    /// using `transport`'s TLS mode and request timeout.
    ///
    /// Recent standalone controllers can return a misleading 401 from
    /// `/api/auth/login`, so we need both probes before deciding. They run
    /// concurrently.
    ///
    /// Observed patterns:
    /// - Classic standalone: `/api/login` responds with 200/400, while
    ///   `/api/auth/login` is usually 404 and may occasionally be 401.
    /// - UniFi OS: both `/api/login` and `/api/auth/login` can return 401.
    pub async fn detect_platform_with_transport(
        base_url: &Url,
        transport: &TransportConfig,
    ) -> Result<ControllerPlatform, Error> {
        // The probes carry no session, so they get no cookie jar.
        let probe_transport = TransportConfig {
            cookie_jar: None,
            ..transport.clone()
        };
        let http = probe_transport.build_client()?;
        let transport_error = |error| Error::from_transport(error, Some(transport.timeout));

        let standalone_url = base_url.join("/api/login").map_err(Error::InvalidUrl)?;
        debug!("probing standalone at {}", standalone_url);
        let unifi_os_url = base_url
            .join("/api/auth/login")
            .map_err(Error::InvalidUrl)?;
        debug!("probing UniFi OS at {}", unifi_os_url);
        let (standalone_status, unifi_os_status) = tokio::join!(
            async {
                http.get(standalone_url)
                    .send()
                    .await
                    .map(|resp| resp.status())
            },
            async {
                http.get(unifi_os_url)
                    .send()
                    .await
                    .map(|resp| resp.status())
            },
        );

        if matches!(
            standalone_status.as_ref(),
            Ok(&reqwest::StatusCode::OK | &reqwest::StatusCode::BAD_REQUEST)
        ) {
            debug!("detected standalone (classic) controller");
            return Ok(ControllerPlatform::ClassicController);
        }

        if matches!(
            (standalone_status.as_ref(), unifi_os_status.as_ref()),
            (
                Ok(&reqwest::StatusCode::UNAUTHORIZED),
                Ok(&reqwest::StatusCode::UNAUTHORIZED)
            )
        ) {
            debug!("detected UniFi OS controller from dual-401 login probes");
            return Ok(ControllerPlatform::UnifiOs);
        }

        if let Ok(status) = unifi_os_status.as_ref()
            && *status != reqwest::StatusCode::NOT_FOUND
        {
            debug!(?status, "detected UniFi OS controller");
            return Ok(ControllerPlatform::UnifiOs);
        }

        if let Ok(status) = standalone_status.as_ref()
            && *status != reqwest::StatusCode::NOT_FOUND
        {
            debug!(?status, "detected standalone (classic) controller");
            return Ok(ControllerPlatform::ClassicController);
        }

        if let Err(e) = standalone_status {
            return Err(transport_error(e));
        }

        if let Err(e) = unifi_os_status {
            return Err(transport_error(e));
        }

        Err(Error::Authentication {
            message: "could not detect controller platform: both login probes returned 404".into(),
        })
    }
}
