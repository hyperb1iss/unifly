// Persistent session cache for legacy API authentication.
//
// Caches the TOKEN cookie and CSRF token to disk so that subsequent CLI
// invocations can skip the login flow (including MFA). The cached session
// is keyed by profile name + controller URL and stored under the user's
// cache directory.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// Margin subtracted from the JWT `exp` to avoid using a session that is
/// about to expire between the time we read it and the time the request
/// actually reaches the controller.
const EXPIRY_MARGIN_SECS: u64 = 60;

#[derive(Debug, Serialize, Deserialize)]
pub struct CachedSession {
    pub token: String,
    pub csrf_token: Option<String>,
    pub controller_url: String,
    /// Unix timestamp (seconds) when the session expires.
    pub expires_at: u64,
}

impl CachedSession {
    /// Returns `true` if the cached session has not yet expired
    /// (with a safety margin).
    pub fn is_valid(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();
        self.expires_at > now + EXPIRY_MARGIN_SECS
    }
}

/// Manages reading/writing cached sessions to disk.
pub struct SessionCache {
    path: PathBuf,
}

impl SessionCache {
    /// Create a cache handle for the given profile name and controller URL.
    ///
    /// The cache key is derived from both so the same profile used with
    /// different tunnel ports doesn't collide.
    pub fn new(profile_name: &str, controller_url: &str) -> Option<Self> {
        let cache_dir = directories::ProjectDirs::from("", "", "unifly")?
            .cache_dir()
            .join("sessions");

        // Build a filename from profile + URL hash to avoid filesystem-unsafe chars.
        let key = format!("{profile_name}_{controller_url}");
        let hash = simple_hash(&key);
        let filename = format!("{profile_name}_{hash:016x}.json");

        Some(Self {
            path: cache_dir.join(filename),
        })
    }

    /// Try to load a cached session from disk. Returns `None` if the file
    /// doesn't exist, can't be parsed, or the session has expired.
    pub fn load(&self) -> Option<CachedSession> {
        let data = fs::read_to_string(&self.path).ok()?;
        let session: CachedSession = serde_json::from_str(&data).ok()?;

        if session.is_valid() {
            debug!(path = %self.path.display(), "loaded cached session");
            Some(session)
        } else {
            debug!("cached session expired, removing");
            self.clear();
            None
        }
    }

    /// Save a session to disk. Creates the parent directory if needed and
    /// sets file permissions to owner-only (0600) on Unix.
    pub fn save(&self, session: &CachedSession) {
        if let Some(parent) = self.path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                warn!("failed to create session cache directory: {e}");
                return;
            }
        }

        let json = match serde_json::to_string_pretty(session) {
            Ok(j) => j,
            Err(e) => {
                warn!("failed to serialize session cache: {e}");
                return;
            }
        };

        // Write atomically via a temp file to avoid partial reads.
        let tmp_path = self.path.with_extension("tmp");
        let result = (|| {
            let mut file = fs::File::create(&tmp_path)?;
            file.write_all(json.as_bytes())?;
            file.sync_all()?;

            // Restrict permissions before moving into place.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600))?;
            }

            fs::rename(&tmp_path, &self.path)
        })();

        match result {
            Ok(()) => debug!(path = %self.path.display(), "saved session cache"),
            Err(e) => {
                warn!("failed to write session cache: {e}");
                let _ = fs::remove_file(&tmp_path);
            }
        }
    }

    /// Remove the cached session file.
    pub fn clear(&self) {
        if self.path.exists() {
            let _ = fs::remove_file(&self.path);
            debug!(path = %self.path.display(), "cleared session cache");
        }
    }
}

/// Extract the `exp` (expiration) claim from a JWT token without
/// verifying the signature. Returns a Unix timestamp in seconds.
pub fn jwt_expiry(token: &str) -> Option<u64> {
    // JWTs are three base64url-encoded segments separated by dots.
    let payload = token.split('.').nth(1)?;

    // base64url decoding: replace URL-safe chars and add padding.
    let b64 = payload.replace('-', "+").replace('_', "/");
    let padded = match b64.len() % 4 {
        2 => format!("{b64}=="),
        3 => format!("{b64}="),
        _ => b64,
    };

    let decoded = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &padded)
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    claims.get("exp")?.as_u64()
}

/// Simple non-cryptographic hash for building cache filenames.
fn simple_hash(s: &str) -> u64 {
    let mut hash: u64 = 5381;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(u64::from(byte));
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jwt_expiry_parses_valid_token() {
        // Minimal JWT: header.payload.signature
        // payload = {"exp": 1735689600}
        let payload = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            b"{\"exp\":1735689600}",
        );
        let token = format!("eyJhbGciOiJIUzI1NiJ9.{payload}.signature");
        assert_eq!(jwt_expiry(&token), Some(1_735_689_600));
    }

    #[test]
    fn jwt_expiry_returns_none_for_garbage() {
        assert_eq!(jwt_expiry("not-a-jwt"), None);
        assert_eq!(jwt_expiry("a.b.c"), None);
    }

    #[test]
    fn cached_session_validity() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_secs();

        let valid = CachedSession {
            token: "t".into(),
            csrf_token: None,
            controller_url: "https://localhost".into(),
            expires_at: now + 3600,
        };
        assert!(valid.is_valid());

        let expired = CachedSession {
            token: "t".into(),
            csrf_token: None,
            controller_url: "https://localhost".into(),
            expires_at: now - 1,
        };
        assert!(!expired.is_valid());
    }
}
