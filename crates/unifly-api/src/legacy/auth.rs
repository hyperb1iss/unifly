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

impl LegacyClient {
    /// Authenticate with the controller using username/password.
    ///
    /// On success the session cookie is stored in the client's cookie jar
    /// and used for all subsequent requests. The login endpoint differs
    /// by platform:
    /// - UniFi OS: `POST /api/auth/login`
    /// - Standalone: `POST /api/login`
    pub async fn login(
        &self,
        username: &str,
        password: &SecretString,
        totp_token: Option<&str>,
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
            .map_err(Error::Transport)?;

        let status = resp.status();
        if !status.is_success() {
            let resp_body = resp.text().await.unwrap_or_default();

            // HTTP 499 with MFA_AUTH_REQUIRED — attempt TOTP completion
            if status.as_u16() == 499 && resp_body.contains("MFA_AUTH_REQUIRED") {
                let totp = totp_token.ok_or(Error::TwoFactorRequired)?;

                // Extract the UBIC_2FA cookie from the MFA challenge response
                let mfa_cookie: String = serde_json::from_str::<serde_json::Value>(&resp_body)
                    .ok()
                    .and_then(|v| v["data"]["mfaCookie"].as_str().map(String::from))
                    .ok_or_else(|| Error::Authentication {
                        message: "MFA challenge response missing mfaCookie".into(),
                    })?;

                debug!("MFA required, retrying with TOTP token");

                let mfa_body = json!({
                    "username": username,
                    "password": password.expose_secret(),
                    "token": totp,
                });

                // Inject the UBIC_2FA cookie into the shared cookie jar.
                // The mfaCookie from the challenge response is formatted as
                // a Set-Cookie header value (e.g. "UBIC_2FA=eyJ...").
                self.add_cookie(&mfa_cookie, &url);

                let mfa_resp = self
                    .http()
                    .post(url)
                    .json(&mfa_body)
                    .send()
                    .await
                    .map_err(Error::Transport)?;

                let mfa_status = mfa_resp.status();
                if !mfa_status.is_success() {
                    let err_body = mfa_resp.text().await.unwrap_or_default();
                    return Err(Error::Authentication {
                        message: format!("MFA login failed (HTTP {mfa_status}): {err_body}"),
                    });
                }

                // Capture CSRF token from MFA login response
                if let Some(token) = mfa_resp
                    .headers()
                    .get("X-CSRF-Token")
                    .or_else(|| mfa_resp.headers().get("x-csrf-token"))
                    .and_then(|v| v.to_str().ok())
                {
                    self.set_csrf_token(token.to_owned());
                }

                debug!("MFA login successful");
                return Ok(());
            }

            return Err(Error::Authentication {
                message: format!("login failed (HTTP {status}): {resp_body}"),
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
