// Persistent session cache for session API auth.
//
// Stores session cookies and CSRF tokens on disk so subsequent CLI
// invocations can skip the login handshake (especially valuable when
// MFA/TOTP is enabled). Cache files live under `$XDG_CACHE_HOME/unifly/`
// and are keyed by `{profile_name}_{host_hash}.json`.
//
// Security: files are created with 0600 permissions on Unix.
// Expiry: parsed from the JWT `exp` claim with a 60-second safety margin.
// Validation: a lightweight probe to `/api/s/{site}/self` confirms the
// session is still alive before trusting the cache.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// Cached session data persisted to disk.
#[derive(Debug, Serialize, Deserialize)]
struct CachedSession {
    /// Session cookie string (e.g. `TOKEN=abc...`).
    cookie: String,
    /// CSRF token for UniFi OS.
    csrf_token: Option<String>,
    /// Unix timestamp when the session expires.
    expires_at: u64,
}

/// Handle for reading/writing a session cache file.
pub struct SessionCache {
    path: PathBuf,
}

impl SessionCache {
    /// Create a new cache handle for the given profile and controller URL.
    ///
    /// Returns `None` if the cache directory can't be determined.
    pub fn new(profile_name: &str, controller_url: &str) -> Option<Self> {
        let cache_dir = cache_dir()?;
        // Hash the URL to avoid path-unsafe characters
        let url_hash = simple_hash(controller_url);
        let filename = format!("{profile_name}_{url_hash}.json");
        Some(Self {
            path: cache_dir.join(filename),
        })
    }

    /// Load a cached session if it exists and hasn't expired.
    ///
    /// Returns `(cookie_header, csrf_token)` on success.
    pub fn load(&self) -> Option<(String, Option<String>)> {
        let data = fs::read_to_string(&self.path).ok()?;
        let session: CachedSession = serde_json::from_str(&data).ok()?;

        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();

        if now >= session.expires_at {
            debug!("cached session expired, removing");
            self.clear();
            return None;
        }

        debug!(
            expires_in_secs = session.expires_at.saturating_sub(now),
            "loaded cached session"
        );
        Some((session.cookie, session.csrf_token))
    }

    /// Save a session to disk with atomic write.
    pub fn save(&self, cookie: &str, csrf_token: Option<&str>, expires_at: u64) {
        let session = CachedSession {
            cookie: cookie.to_owned(),
            csrf_token: csrf_token.map(str::to_owned),
            expires_at,
        };

        let Ok(json) = serde_json::to_string_pretty(&session) else {
            warn!("failed to serialize session cache");
            return;
        };

        if let Err(e) = atomic_write(&self.path, json.as_bytes()) {
            warn!(error = %e, "failed to write session cache");
        } else {
            debug!("session cached to {}", self.path.display());
        }
    }

    /// Remove the cache file.
    pub fn clear(&self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Extract the `exp` claim from a JWT token string.
///
/// Parses the payload (second segment) of the JWT to read the expiry.
/// Returns `None` if the token is malformed or missing `exp`.
pub fn jwt_expiry(token: &str) -> Option<u64> {
    // JWT cookies are "TOKEN=eyJ...", extract just the JWT value
    let jwt = token.split(';').next()?.split('=').nth(1)?;

    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() != 3 {
        return None;
    }

    let payload = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    claims["exp"].as_u64()
}

/// Default fallback expiry: 2 hours from now.
pub fn fallback_expiry() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() + 2 * 3600)
}

/// Safety margin subtracted from JWT expiry (60 seconds).
pub const EXPIRY_MARGIN_SECS: u64 = 60;

// ── Internals ──────────────────────────────────────────────────────

/// Resolve the cache directory.
///
/// Unix (including macOS): `$XDG_CACHE_HOME/unifly/` or `~/.cache/unifly/`
/// Windows: platform-native via `ProjectDirs`
///
/// On macOS, migrates from the old `~/Library/Caches/unifly/` location automatically.
fn cache_dir() -> Option<PathBuf> {
    let dir = platform_cache_dir()?;
    #[cfg(target_os = "macos")]
    migrate_macos_cache(&dir);
    Some(dir)
}

#[cfg(not(windows))]
fn platform_cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("unifly"))
}

#[cfg(windows)]
fn platform_cache_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "unifly").map(|dirs| dirs.cache_dir().to_owned())
}

#[cfg(target_os = "macos")]
fn migrate_macos_cache(new_dir: &Path) {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let Some(old_dir) =
            directories::ProjectDirs::from("", "", "unifly").map(|d| d.cache_dir().to_owned())
        else {
            return;
        };
        if old_dir == *new_dir || !old_dir.exists() || new_dir.exists() {
            return;
        }
        if let Some(parent) = new_dir.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if fs::rename(&old_dir, new_dir).is_ok() {
            debug!(
                "migrated session cache from {} to {}",
                old_dir.display(),
                new_dir.display()
            );
        }
    });
}

/// Simple non-cryptographic hash for URL → filename mapping.
fn simple_hash(s: &str) -> String {
    let mut hash: u64 = 5381;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(u64::from(byte));
    }
    format!("{hash:016x}")
}

/// Atomic write: stage the data in a new file beside `path`, then rename
/// it over the target.
fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let (staged, mut file) = create_staging_file(path)?;
    let written = file.write_all(data).and_then(|()| file.flush());
    drop(file);
    if let Err(error) = written {
        let _ = fs::remove_file(&staged);
        return Err(error);
    }

    // rename replaces an existing target on every platform (MoveFileExW
    // with MOVEFILE_REPLACE_EXISTING on Windows), so the cache file is
    // never missing between the old copy and the new one.
    fs::rename(&staged, path).inspect_err(|_| {
        let _ = fs::remove_file(&staged);
    })
}

/// Create the staging file for a write to `path`.
///
/// The name is unique to this process and call, so concurrent writers
/// never share one, and `create_new` refuses any existing entry (a planted
/// symlink included) instead of following it. On Unix the file is
/// owner-only from the moment it exists, before the cookie is written.
fn create_staging_file(path: &Path) -> std::io::Result<(PathBuf, fs::File)> {
    static NEXT: AtomicU64 = AtomicU64::new(0);

    let name = path
        .file_name()
        .map_or_else(|| "session".into(), |name| name.to_string_lossy());
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());
    let staged = path.with_file_name(format!(
        ".{name}.{}.{}.{nanos}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
    ));

    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&staged)?;
    Ok((staged, file))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jwt_expiry_parses_valid_token() {
        // Build a minimal JWT: header.payload.signature
        let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"HS256"}"#);
        let payload = URL_SAFE_NO_PAD.encode(r#"{"exp":1700000000}"#);
        let token = format!("TOKEN={header}.{payload}.sig");
        assert_eq!(jwt_expiry(&token), Some(1_700_000_000));
    }

    #[test]
    fn jwt_expiry_returns_none_for_garbage() {
        assert_eq!(jwt_expiry("not-a-jwt"), None);
        assert_eq!(jwt_expiry("TOKEN=a.b"), None);
    }

    #[test]
    fn simple_hash_is_deterministic() {
        let a = simple_hash("https://192.168.1.1");
        let b = simple_hash("https://192.168.1.1");
        assert_eq!(a, b);
        assert_ne!(a, simple_hash("https://10.0.0.1"));
    }

    #[test]
    fn session_cache_round_trips() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let cache = SessionCache {
            path: dir.path().join("test.json"),
        };

        cache.save("TOKEN=abc", Some("csrf123"), fallback_expiry());
        let loaded = cache.load().expect("cache should load");
        assert_eq!(loaded.0, "TOKEN=abc");
        assert_eq!(loaded.1.as_deref(), Some("csrf123"));
    }

    #[cfg(unix)]
    #[test]
    fn save_never_writes_through_a_planted_temp_path() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let path = dir.path().join("planted.json");
        let victim = dir.path().join("victim");
        fs::write(&victim, "untouched").expect("victim written");
        std::os::unix::fs::symlink(&victim, path.with_extension("tmp")).expect("symlink");
        let cache = SessionCache { path };

        cache.save("TOKEN=secret", None, fallback_expiry());

        assert_eq!(fs::read_to_string(&victim).expect("victim"), "untouched");
        assert_eq!(cache.load().expect("cache loads").0, "TOKEN=secret");
    }

    #[cfg(unix)]
    #[test]
    fn staging_file_is_owner_only_before_any_write() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tmpdir");
        let (staged, file) = create_staging_file(&dir.path().join("mode.json")).expect("staged");

        let mode = file.metadata().expect("metadata").permissions().mode();
        assert_eq!(
            mode & 0o077,
            0,
            "staging file mode {mode:o} is open to others"
        );
        drop(file);
        fs::remove_file(staged).expect("cleanup");
    }

    #[test]
    fn concurrent_saves_leave_one_whole_file() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let path = dir.path().join("shared.json");

        std::thread::scope(|scope| {
            for writer in 0..8 {
                let cache = SessionCache { path: path.clone() };
                scope.spawn(move || {
                    for round in 0..25 {
                        let cookie = format!("TOKEN={writer}-{round}-{}", "x".repeat(4096));
                        cache.save(&cookie, Some("csrf"), fallback_expiry());
                    }
                });
            }
        });

        let cache = SessionCache { path };
        assert!(cache.load().expect("cache loads").0.starts_with("TOKEN="));
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .filter(|name| name != "shared.json")
            .collect();
        assert!(
            leftovers.is_empty(),
            "staging files left behind: {leftovers:?}"
        );
    }

    #[test]
    fn expired_session_returns_none() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let cache = SessionCache {
            path: dir.path().join("expired.json"),
        };

        cache.save("TOKEN=old", None, 0); // expired at epoch
        assert!(cache.load().is_none());
    }
}
