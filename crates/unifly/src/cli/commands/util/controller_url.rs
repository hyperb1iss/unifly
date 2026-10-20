use crate::cli::error::CliError;
use crate::config::redact_url_userinfo;

/// Require an `http` or `https` controller or Site Manager URL.
///
/// Any other scheme parses as a URL, so it would reach the HTTP client and
/// fail there as an opaque "builder error"; checking first reports it as
/// the configuration mistake it is.
pub fn require_http_url(url: &str) -> Result<(), CliError> {
    let shown = redact_url_userinfo(url);
    let parsed = url::Url::parse(url).map_err(|err| CliError::InvalidConfig {
        message: format!("invalid URL '{shown}': {err}"),
    })?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        scheme => Err(CliError::InvalidConfig {
            message: format!("unsupported URL scheme '{scheme}' in {shown}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::require_http_url;
    use crate::cli::error::{CliError, exit_code};

    #[test]
    fn only_http_schemes_pass() {
        assert!(require_http_url("https://192.168.1.1").is_ok());
        assert!(require_http_url("http://unifi.local:8443/").is_ok());
        for url in ["ftp://192.0.2.1", "foo:bar", "not a url"] {
            let err = require_http_url(url).expect_err(url);
            assert!(
                matches!(err, CliError::InvalidConfig { .. }),
                "{url}: {err:?}"
            );
            assert_eq!(err.exit_code(), exit_code::USAGE);
        }
    }
}
