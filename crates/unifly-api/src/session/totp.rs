// TOTP code sources for MFA logins
//
// A TOTP code is valid for one 30 s step. A login made long after connect
// (an automatic re-login when the session cookie expires) therefore needs
// a code that is current at that moment, which only a provider can supply.

use std::sync::Arc;

use secrecy::SecretString;
use tracing::warn;

/// A source of current TOTP codes for MFA logins.
///
/// The session client calls it once per login that needs a code, so each
/// call must return the code for the current time step, or `None` when it
/// cannot produce one. Calls run on Tokio's blocking pool, so a provider
/// may block, for example while running a password manager's CLI.
///
/// Only wrap a source that genuinely yields a fresh code on every call. A
/// closure that returns a value captured once (a command-line flag, or an
/// environment variable read by this process) is a static code in
/// disguise: the session client refuses to send a code it has already
/// used, so such a provider never renews a session.
#[derive(Clone)]
pub struct TotpProvider(Arc<dyn Fn() -> Option<SecretString> + Send + Sync>);

impl TotpProvider {
    /// Wrap a closure that returns the current TOTP code.
    pub fn new<F>(provider: F) -> Self
    where
        F: Fn() -> Option<SecretString> + Send + Sync + 'static,
    {
        Self(Arc::new(provider))
    }

    /// Ask for the current code on the blocking pool, so a slow provider
    /// never stalls an async worker. A provider that panics yields `None`.
    pub(crate) async fn fresh_code(&self) -> Option<SecretString> {
        let provider = Arc::clone(&self.0);
        match tokio::task::spawn_blocking(move || provider()).await {
            Ok(code) => code,
            Err(error) => {
                warn!(%error, "TOTP provider failed");
                None
            }
        }
    }
}

impl std::fmt::Debug for TotpProvider {
    /// A closure has no useful representation, and printing what it
    /// returns would leak a code.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TotpProvider(..)")
    }
}

/// Where a login gets its TOTP code. The code is only resolved if the
/// controller actually issues an MFA challenge, so a login that does not
/// need one never runs the provider.
#[derive(Clone, Copy)]
pub(crate) enum TotpSource<'a> {
    None,
    Code(&'a SecretString),
    Provider(&'a TotpProvider),
}

impl<'a> TotpSource<'a> {
    /// An explicit code wins over a provider.
    pub(crate) fn new(code: Option<&'a SecretString>, provider: Option<&'a TotpProvider>) -> Self {
        match (code, provider) {
            (Some(code), _) => Self::Code(code),
            (None, Some(provider)) => Self::Provider(provider),
            (None, None) => Self::None,
        }
    }

    pub(super) async fn resolve(self) -> Option<SecretString> {
        match self {
            Self::None => None,
            Self::Code(code) => Some(code.clone()),
            Self::Provider(provider) => provider.fresh_code().await,
        }
    }
}

/// Whether `code` has the shape of a TOTP code: exactly six ASCII digits.
pub(crate) fn is_totp_code(code: &str) -> bool {
    code.len() == 6 && code.chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use super::{TotpProvider, is_totp_code};
    use crate::session::ReauthCredentials;

    #[test]
    fn debug_output_never_shows_a_code() {
        let provider = TotpProvider::new(|| Some(SecretString::from("482913".to_owned())));
        let credentials = ReauthCredentials {
            username: "admin".into(),
            password: SecretString::from("hunter2".to_owned()),
            totp_provider: Some(provider.clone()),
            cache: None,
        };

        let rendered = format!("{provider:?} {credentials:?}");
        assert!(!rendered.contains("482913"), "{rendered}");
        assert!(!rendered.contains("hunter2"), "{rendered}");
        assert!(rendered.contains("totp_provider: true"), "{rendered}");
    }

    #[test]
    fn only_six_ascii_digits_pass_as_a_code() {
        assert!(is_totp_code("012345"));
        for bad in ["", "12345", "1234567", "12345a", "١٢٣٤٥٦"] {
            assert!(!is_totp_code(bad), "{bad:?} accepted");
        }
    }
}
