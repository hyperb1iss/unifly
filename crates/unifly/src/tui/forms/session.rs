//! What this TUI session runs on: the profile, and the launch flags that
//! override it. The setup wizard and the Settings screen read it when they
//! open, so they edit the profile in use and show where a flag overrides
//! what they hold.

use std::sync::RwLock;

use crate::config::resolve::ConnectionOverrides;

/// The session's connection choices.
#[derive(Debug, Clone, Default)]
pub(crate) struct Session {
    /// The profile the session connects with. `None` until the launch
    /// records one, which then means the config's default.
    pub(crate) profile: Option<String>,
    /// Launch flags that override the profile for as long as the session
    /// stays on it.
    pub(crate) overrides: ConnectionOverrides,
    /// A one-time code given at launch (`--totp`), offered to the setup
    /// wizard so an MFA account can finish it.
    pub(crate) launch_totp: Option<secrecy::SecretString>,
    /// Why the launch could not connect and opened the wizard instead.
    pub(crate) setup_reason: Option<String>,
}

impl Session {
    /// The override a launch flag puts on `field`, as the flag and its
    /// value, for the form to show next to the field it overrides.
    pub(crate) fn override_on(&self, field: Overridable) -> Option<String> {
        describe(&self.overrides, field)
    }

    /// The session once it runs on `profile` with `flags`, the launch
    /// flags the form that connected it left in force. A code given at
    /// launch and the reason setup opened belong to the launch alone.
    fn moved_to(profile: &str, flags: ConnectionOverrides) -> Self {
        Self {
            profile: Some(profile.to_owned()),
            overrides: flags,
            ..Self::default()
        }
    }
}

/// The override `overrides` puts on `field`, as the flag and its value.
pub(crate) fn describe(overrides: &ConnectionOverrides, field: Overridable) -> Option<String> {
    match field {
        Overridable::Url => overrides
            .controller
            .as_ref()
            .map(|url| format!("--controller {}", crate::config::redact_url_userinfo(url))),
        Overridable::Site => overrides.site.as_ref().map(|site| format!("--site {site}")),
        Overridable::ApiKey => overrides.api_key.as_ref().map(|_| "--api-key".to_owned()),
        Overridable::HostId => overrides
            .host_id
            .as_ref()
            .map(|host| format!("--host-id {host}")),
        Overridable::Insecure => overrides
            .insecure
            .map(|insecure| format!("--insecure={insecure}")),
        Overridable::Timeout => overrides.timeout.map(|secs| format!("--timeout {secs}")),
        // A key given at launch is what the session signs in with,
        // whatever mode the profile names.
        Overridable::AuthMode => overrides.api_key.as_ref().map(|_| "--api-key".to_owned()),
    }
}

/// A profile setting a launch flag can override.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Overridable {
    Url,
    Site,
    ApiKey,
    HostId,
    Insecure,
    Timeout,
    AuthMode,
}

static SESSION: RwLock<Option<Session>> = RwLock::new(None);

/// Record what the launch selected.
pub fn begin(session: Session) {
    if let Ok(mut slot) = SESSION.write() {
        *slot = Some(session);
    }
}

/// The session as it stands.
pub(crate) fn current() -> Session {
    SESSION
        .read()
        .ok()
        .and_then(|slot| slot.clone())
        .unwrap_or_default()
}

/// Move the session to `profile`, the one a wizard or Settings just
/// connected with, under `flags`: the launch flags that form tested the
/// connection with. A flag the form asked about or changed has had its
/// answer saved in the profile, and is not among them.
pub(crate) fn switch_to(profile: &str, flags: ConnectionOverrides) {
    if let Ok(mut slot) = SESSION.write() {
        *slot = Some(Session::moved_to(profile, flags));
    }
}

#[cfg(test)]
mod tests {
    use super::{Overridable, Session};
    use crate::config::resolve::ConnectionOverrides;

    fn launched() -> Session {
        Session {
            profile: Some("home".into()),
            overrides: ConnectionOverrides {
                site: Some("lab".into()),
                insecure: Some(true),
                ..ConnectionOverrides::default()
            },
            launch_totp: Some(secrecy::SecretString::from("123456".to_owned())),
            setup_reason: Some("no API key".into()),
        }
    }

    /// The form that connected the session says which launch flags it
    /// tested under; the session keeps exactly those, and nothing else
    /// of the launch.
    #[test]
    fn the_session_keeps_the_flags_its_connection_was_tested_under() {
        let launch = launched();
        let kept = ConnectionOverrides {
            site: launch.overrides.site.clone(),
            no_cache: true,
            ..ConnectionOverrides::default()
        };

        let session = Session::moved_to("home", kept);

        assert_eq!(session.profile.as_deref(), Some("home"));
        assert_eq!(
            session.override_on(Overridable::Site).as_deref(),
            Some("--site lab")
        );
        assert_eq!(session.override_on(Overridable::Insecure), None);
        assert!(session.overrides.no_cache);
        assert!(session.launch_totp.is_none(), "a launch code is spent");
        assert!(session.setup_reason.is_none());
    }
}
