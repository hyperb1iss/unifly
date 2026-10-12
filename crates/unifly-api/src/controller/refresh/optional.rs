use tracing::{debug, warn};

/// Turn an optional endpoint's fetch result into its refresh snapshot field.
///
/// `Some` is authoritative: the store upserts it and prunes everything else,
/// so `Some(empty)` clears the collection. `None` tells the store to leave
/// the collection exactly as it is, which keeps one timeout or 5xx from
/// wiping data the controller still has.
///
/// A 404 also maps to `None`. The refresh snapshot is the only writer of
/// these collections, so when an endpoint is genuinely missing on this
/// firmware the collection was never populated and keeping it is the same
/// as clearing it. When a 404 turns out to be transient, keeping the last
/// known data is the right call. Mapping 404 to `None` is never worse.
pub(super) fn keep_on_failure<T>(endpoint: &str, result: Result<T, crate::Error>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) if error.is_not_found() => {
            debug!(
                endpoint,
                error = %error,
                "endpoint not available (404); keeping last known data"
            );
            None
        }
        Err(error) => {
            warn!(
                endpoint,
                error = %error,
                "optional fetch failed; keeping last known data"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::keep_on_failure;
    use crate::Error;

    fn integration_error(status: u16) -> Error {
        Error::Integration {
            message: "boom".into(),
            code: None,
            status,
        }
    }

    fn session_error(message: &str) -> Error {
        Error::SessionApi {
            message: message.into(),
        }
    }

    #[test]
    fn success_is_authoritative_even_when_empty() {
        assert_eq!(keep_on_failure("test", Ok(vec![1, 2])), Some(vec![1, 2]));
        assert_eq!(
            keep_on_failure("test", Ok(Vec::<u32>::new())),
            Some(Vec::new())
        );
    }

    #[test]
    fn errors_keep_last_known_data() {
        for error in [
            integration_error(500),
            integration_error(503),
            session_error("HTTP 500 Internal Server Error: oops"),
            Error::Timeout { timeout_secs: 30 },
        ] {
            assert_eq!(keep_on_failure::<Vec<u32>>("test", Err(error)), None);
        }
    }

    #[test]
    fn not_found_keeps_last_known_data() {
        for error in [
            integration_error(404),
            session_error("HTTP 404 Not Found: {}"),
            session_error("api.err.NotFound"),
        ] {
            assert!(error.is_not_found());
            assert_eq!(keep_on_failure::<Vec<u32>>("test", Err(error)), None);
        }
    }
}
