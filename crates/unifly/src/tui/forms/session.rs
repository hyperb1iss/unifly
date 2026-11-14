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
    pub(crate) launch_totp: Option<String>,
    /// Why the launch could not connect and opened the wizard instead.
    pub(crate) setup_reason: Option<String>,
}

impl Session {
    /// The override a launch flag puts on `field`, as the flag and its
    /// value, for the form to show next to the field it overrides.
    pub(crate) fn override_on(&self, field: Overridable) -> Option<String> {
        let overrides = &self.overrides;
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
        }
    }

    /// The session after it moves to `profile`. Launch flags were given
    /// for the profile the launch selected, so they stay only with it.
    fn moved_to(&self, profile: &str) -> Self {
        if self.profile.as_deref() == Some(profile) {
            return Self {
                setup_reason: None,
                launch_totp: None,
                ..self.clone()
            };
        }
        Self {
            profile: Some(profile.to_owned()),
            ..Self::default()
        }
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
/// connected with.
pub(crate) fn switch_to(profile: &str) {
    if let Ok(mut slot) = SESSION.write() {
        let moved = slot.clone().unwrap_or_default().moved_to(profile);
        *slot = Some(moved);
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
            launch_totp: Some("123456".into()),
            setup_reason: Some("no API key".into()),
        }
    }

    #[test]
    fn launch_flags_stay_while_the_session_keeps_its_profile() {
        let session = launched().moved_to("home");

        assert_eq!(session.overrides.site.as_deref(), Some("lab"));
        assert_eq!(
            session.override_on(Overridable::Site).as_deref(),
            Some("--site lab")
        );
        assert_eq!(
            session.override_on(Overridable::Insecure).as_deref(),
            Some("--insecure=true")
        );
        assert_eq!(session.override_on(Overridable::Url), None);
        assert_eq!(session.launch_totp, None, "a launch code is spent");
    }

    #[test]
    fn switching_profiles_leaves_the_launch_flags_behind() {
        let session = launched().moved_to("work");

        assert_eq!(session.profile.as_deref(), Some("work"));
        assert_eq!(session.overrides.site, None);
        assert_eq!(session.override_on(Overridable::Insecure), None);
    }
}
