// Legacy API authentication
//
// Cookie-based session login/logout and controller platform detection.
// The login endpoint sets a session cookie in the client's jar;
// subsequent requests use that cookie automatically.

use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use tracing::debug;
use url::Url;

use crate::auth::ControllerPlatform;
use crate::error::Error;
use crate::legacy::client::LegacyClient;
use crate::legacy::session_cache::{CachedSession, SessionCache, jwt_expiry};

impl LegacyClient {
    /// Authenticate with the controller using username/password.
    ///
    /// On success the session cookie is stored in the client's cookie jar
    /// and used for all subsequent requests. The login endpoint differs
    /// by platform:
    /// - UniFi OS: `POST /api/auth/login`
    /// - Standalone: `POST /api/login`
    pub async fn login(&self, username: &str, password: &SecretString) -> Result<(), Error> {
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
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(Error::Transport)?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Authentication {
                message: format!("login failed (HTTP {status}): {body}"),
            });
        }

        // Capture CSRF token from login response — required for all
        // POST/PUT/DELETE requests through the UniFi OS proxy.
        if let Some(token) = resp
            .headers()
            .get("X-CSRF-Token")
            .or_else(|| resp.headers().get("x-csrf-token"))
            .and_then(|v| v.to_str().ok())
        {
            self.set_csrf_token(token.to_owned());
        }

        debug!("login successful");
        Ok(())
    }

    /// Authenticate, using a cached session if available.
    ///
    /// Tries to restore a cached session first. If no valid cache exists,
    /// performs a fresh login and saves the resulting session to disk.
    pub async fn login_with_cache(
        &self,
        username: &str,
        password: &SecretString,
        cache: &SessionCache,
    ) -> Result<(), Error> {
        // Try cached session first.
        if let Some(session) = cache.load() {
            if session.controller_url == self.base_url().as_str() {
                debug!("restoring session from cache");
                self.restore_session(&session.token, session.csrf_token.as_deref());
                return Ok(());
            }
            debug!("cached session URL mismatch, performing fresh login");
        }

        // Fresh login.
        self.login(username, password).await?;

        // Save the session for next time.
        self.save_session_to_cache(cache);

        Ok(())
    }

    /// Extract the current session state and write it to the cache.
    pub fn save_session_to_cache(&self, cache: &SessionCache) {
        let Some(token) = self.extract_token() else {
            debug!("no TOKEN cookie to cache");
            return;
        };

        let expires_at = jwt_expiry(&token).unwrap_or_else(|| {
            // Fallback: assume 12-hour validity if we can't parse the JWT.
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() + 12 * 3600)
                .unwrap_or(0)
        });

        let session = CachedSession {
            token,
            csrf_token: self.csrf_token(),
            controller_url: self.base_url().to_string(),
            expires_at,
        };

        cache.save(&session);
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
            .map_err(Error::Transport)?;

        debug!("logout complete");
        Ok(())
    }

    /// Auto-detect the controller platform by probing login endpoints.
    ///
    /// Tries the UniFi OS endpoint first (`/api/auth/login`). If it
    /// responds (even with an error), we're on UniFi OS. If the connection
    /// fails or returns 404, falls back to standalone detection.
    pub async fn detect_platform(base_url: &Url) -> Result<ControllerPlatform, Error> {
        let http = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .map_err(Error::Transport)?;

        // Probe UniFi OS endpoint
        let unifi_os_url = base_url
            .join("/api/auth/login")
            .map_err(Error::InvalidUrl)?;

        debug!("probing UniFi OS at {}", unifi_os_url);

        if let Ok(resp) = http.get(unifi_os_url).send().await {
            // UniFi OS returns a response (even 401/405) at this path.
            // Standalone controllers don't have this endpoint at all.
            if resp.status() != reqwest::StatusCode::NOT_FOUND {
                debug!("detected UniFi OS platform");
                return Ok(ControllerPlatform::UnifiOs);
            }
        }
        // Connection error -- might be standalone on a different port

        // Probe standalone endpoint
        let standalone_url = base_url.join("/api/login").map_err(Error::InvalidUrl)?;

        debug!("probing standalone at {}", standalone_url);

        match http.get(standalone_url).send().await {
            Ok(_) => {
                debug!("detected standalone (classic) controller");
                Ok(ControllerPlatform::ClassicController)
            }
            Err(e) => Err(Error::Transport(e)),
        }
    }
}
