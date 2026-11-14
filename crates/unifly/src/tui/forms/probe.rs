//! The connection test the setup wizard and the Settings screen run before
//! they save: it connects with exactly what the form holds, and what it
//! tested is what gets saved and applied.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use secrecy::{ExposeSecret, SecretString};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;
use unifly_api::session::session_cache::{
    EXPIRY_MARGIN_SECS, SessionCache, fallback_expiry, jwt_expiry,
};
use unifly_api::{AuthCredentials, Controller, ControllerConfig, CoreError, SiteHint};

use super::controller_profile::{AuthMode, DraftField, SavePlan};
use crate::config::resolve::ConnectionOverrides;
use crate::tui::action::Action;

/// Numbers every test started in this process, across both screens, so a
/// result can only ever be claimed by the test that produced it.
static GENERATION: AtomicU64 = AtomicU64::new(1);

/// What to connect with.
pub(crate) struct ProbeRequest {
    pub(crate) profile: crate::config::Profile,
    pub(crate) profile_name: String,
    pub(crate) overrides: ConnectionOverrides,
    /// A one-time MFA code for the sign-in.
    pub(crate) totp: Option<SecretString>,
}

/// A test that connected.
#[derive(Debug, Clone)]
pub struct ProbePass {
    /// The connection that was tested, as the session will run it.
    pub config: Box<ControllerConfig>,
    /// The sites on the controller.
    pub sites: Vec<SiteHint>,
}

/// The class of a failed test, which decides what the form offers next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    Tls,
    Credentials,
    Mfa,
    Site,
    Unreachable,
    Timeout,
    Unsupported,
    Save,
    Other,
}

/// A failed test, explained: what went wrong, what to do about it, and
/// the field to do it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeFailure {
    pub kind: FailureKind,
    pub summary: String,
    pub next_step: String,
    /// The error as the controller or the network reported it.
    pub detail: Option<String>,
    /// The controller's sites, when the failure was a site it does not have.
    pub sites: Vec<SiteHint>,
    pub field: Option<DraftField>,
}

impl ProbeFailure {
    fn new(kind: FailureKind, summary: impl Into<String>, next_step: impl Into<String>) -> Self {
        Self {
            kind,
            summary: summary.into(),
            next_step: next_step.into(),
            detail: None,
            sites: Vec::new(),
            field: None,
        }
    }

    fn at(mut self, field: DraftField) -> Self {
        self.field = Some(field);
        self
    }

    fn with_detail(mut self, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        if detail != self.summary {
            self.detail = Some(detail);
        }
        self
    }

    /// A save that failed after the connection passed.
    pub(crate) fn save(message: impl Into<String>) -> Self {
        Self::new(
            FailureKind::Save,
            message,
            "Nothing was changed. Fix the cause and press Enter to try again.",
        )
    }

    /// The failure as paragraphs to show, most important first. `private`
    /// says only what class of failure it was: the full text names the
    /// controller, and the raw error can name more.
    pub(crate) fn paragraphs(&self, private: bool) -> Vec<(Tone, &str)> {
        if private {
            return vec![(Tone::Error, self.kind.label())];
        }
        let mut out = vec![(Tone::Error, self.summary.as_str())];
        if !self.next_step.is_empty() {
            out.push((Tone::Advice, self.next_step.as_str()));
        }
        if let Some(detail) = self.detail.as_deref() {
            out.push((Tone::Detail, detail));
        }
        out
    }
}

impl FailureKind {
    /// The failure class in a sentence that names nothing.
    fn label(self) -> &'static str {
        match self {
            Self::Tls => "The controller's certificate could not be verified.",
            Self::Credentials => "The controller did not accept the credentials.",
            Self::Mfa => "The sign-in needs a current one-time code.",
            Self::Site => "The controller has no site with that name.",
            Self::Unreachable => "The controller could not be reached.",
            Self::Timeout => "The controller did not answer in time.",
            Self::Unsupported => "The controller does not support this auth mode.",
            Self::Save => "The connection worked, but the profile could not be saved.",
            Self::Other => "The connection test failed.",
        }
    }
}

/// How a paragraph of a message is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Error,
    Advice,
    Detail,
    Notice,
}

pub(crate) type ProbeResult = Result<ProbePass, ProbeFailure>;

/// What a test in flight was started with, kept so a pass saves exactly
/// that and not the form as it stands when the result arrives.
pub(crate) struct Tested {
    pub(crate) plan: SavePlan,
    pub(crate) profile_name: String,
}

/// The one test a form has in flight. Starting another, cancelling, or
/// dropping the form stops it, and a stopped test reports nothing.
#[derive(Default)]
pub(crate) struct ProbeSlot {
    generation: u64,
    cancel: Option<CancellationToken>,
    tested: Option<Tested>,
    /// The session a sign-in test opened, kept for the dashboard.
    handover: HandoverSlot,
}

impl ProbeSlot {
    /// Start a test of `request`, replacing any still running. Its result
    /// arrives as [`Action::SetupProbeFinished`] carrying the generation
    /// returned here.
    pub(crate) fn start(
        &mut self,
        tx: Option<&UnboundedSender<Action>>,
        request: ProbeRequest,
        plan: SavePlan,
    ) -> u64 {
        self.cancel();
        self.generation = GENERATION.fetch_add(1, Ordering::Relaxed);
        self.tested = Some(Tested {
            plan,
            profile_name: request.profile_name.clone(),
        });
        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        launch(
            tx,
            self.generation,
            request,
            cancel,
            Arc::clone(&self.handover),
        );
        self.generation
    }

    /// Stop the running test. Whatever it finds is discarded, and nothing
    /// it would have saved is saved.
    pub(crate) fn cancel(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        self.tested = None;
    }

    /// The generation of the test started last.
    #[cfg(test)]
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// Whether a test is running.
    #[cfg(test)]
    pub(crate) fn is_running(&self) -> bool {
        self.cancel.is_some()
    }

    /// Claim the result of test `generation`: what it was started with,
    /// or `None` when that test was cancelled or replaced since.
    pub(crate) fn claim(&mut self, generation: u64) -> Option<Tested> {
        if generation != self.generation || self.cancel.is_none() {
            return None;
        }
        self.cancel = None;
        self.tested.take()
    }

    /// Save what a claimed test was started with, and with it hand the
    /// session that test opened to `live`, the connection it passed with.
    /// The result arrives as [`Action::SetupSaveFinished`] for the same
    /// generation.
    pub(crate) fn save(
        &self,
        tx: Option<&UnboundedSender<Action>>,
        tested: Tested,
        live: &ControllerConfig,
    ) {
        launch_save(
            tx,
            self.generation,
            tested,
            live.clone(),
            Arc::clone(&self.handover),
        );
    }

    /// Whether `generation` is the save this slot is waiting on.
    pub(crate) fn owns(&self, generation: u64) -> bool {
        generation == self.generation
    }
}

impl Drop for ProbeSlot {
    fn drop(&mut self) {
        self.cancel();
        // A session nothing was saved for is nobody's to keep.
        if let Some(handover) = self.handover.lock().ok().and_then(|mut slot| slot.take()) {
            handover.discard();
        }
    }
}

/// What a test signed in as.
#[derive(Clone)]
struct SignIn {
    url: String,
    username: String,
    password: SecretString,
}

impl SignIn {
    /// The sign-in `config` makes, when it makes one.
    fn of(config: &ControllerConfig) -> Option<Self> {
        match &config.auth {
            AuthCredentials::Credentials { username, password }
            | AuthCredentials::Hybrid {
                username, password, ..
            } => Some(Self {
                url: config.url.as_str().to_owned(),
                username: username.clone(),
                password: password.clone(),
            }),
            AuthCredentials::ApiKey(_) | AuthCredentials::Cloud { .. } => None,
        }
    }

    fn same_as(&self, other: &Self) -> bool {
        self.url == other.url
            && self.username == other.username
            && self.password.expose_secret() == other.password.expose_secret()
    }
}

/// The session a test signed in with, parked in a session cache of its
/// own until the profile is saved.
///
/// A test must sign in with what was typed, so it cannot share the
/// profile's cached cookie. But the dashboard should not have to sign in
/// again right after (a one-time MFA code could not do it twice), and a
/// test that is never saved must not leave its cookie in the profile's
/// cache. So the cookie waits here: a save that succeeds moves it into
/// the profile's cache, and anything else drops it.
struct Handover {
    /// The cache name the test signs in under, in place of the profile's.
    name: String,
    sign_in: SignIn,
}

type HandoverSlot = Arc<Mutex<Option<Handover>>>;

/// Numbers the private caches this process makes.
static HANDOVERS: AtomicU64 = AtomicU64::new(1);

impl Handover {
    /// The cache name a test making `sign_in` for `profile` signs in
    /// under. A repeat of the sign-in the last test made reuses its cache,
    /// so the retest rides the session it opened instead of spending
    /// another code; a different sign-in starts from an empty one.
    fn cache_name(slot: &HandoverSlot, profile: &str, sign_in: SignIn) -> Option<String> {
        let mut slot = slot.lock().ok()?;
        if let Some(current) = slot.as_ref()
            && current.sign_in.same_as(&sign_in)
        {
            return Some(current.name.clone());
        }
        if let Some(stale) = slot.take() {
            stale.discard();
        }
        let name = format!(
            "{profile}.setup-{}-{}",
            std::process::id(),
            HANDOVERS.fetch_add(1, Ordering::Relaxed)
        );
        *slot = Some(Self {
            name: name.clone(),
            sign_in,
        });
        Some(name)
    }

    fn cache(&self) -> Option<SessionCache> {
        // Unit tests keep out of the user's cache directory.
        (!cfg!(test))
            .then(|| SessionCache::new(&self.name, &self.sign_in.url))
            .flatten()
    }

    /// Drop the parked session.
    fn discard(&self) {
        if let Some(cache) = self.cache() {
            cache.clear();
        }
    }

    /// Move the parked session into the cache `live` reads, so its first
    /// connect restores it.
    fn promote(&self, live: &ControllerConfig) {
        let Some(parked) = self.cache() else {
            return;
        };
        if let Some((cookie, csrf)) = parked.load()
            && let Some(name) = live.profile_name.as_deref()
            && let Some(cache) = SessionCache::new(name, live.url.as_str())
        {
            let expires_at = jwt_expiry(&cookie).map_or_else(fallback_expiry, |expiry| {
                expiry.saturating_sub(EXPIRY_MARGIN_SECS)
            });
            cache.save(&cookie, csrf.as_deref(), expires_at);
        }
        parked.clear();
    }
}

/// Run the test in the background. Unit tests never connect anywhere or
/// write the user's keyring and config: they hand the forms results
/// directly, so in a test build nothing is started.
fn launch(
    tx: Option<&UnboundedSender<Action>>,
    generation: u64,
    request: ProbeRequest,
    cancel: CancellationToken,
    handover: HandoverSlot,
) {
    let Some(tx) = tx.cloned().filter(|_| !cfg!(test)) else {
        return;
    };
    tokio::spawn(async move {
        if let Some(result) = run(request, cancel, handover).await {
            let _ = tx.send(Action::SetupProbeFinished {
                generation,
                result: Box::new(result),
            });
        }
    });
}

fn launch_save(
    tx: Option<&UnboundedSender<Action>>,
    generation: u64,
    tested: Tested,
    live: ControllerConfig,
    handover: HandoverSlot,
) {
    let Some(tx) = tx.cloned().filter(|_| !cfg!(test)) else {
        return;
    };
    // The keyring can block on an unlock prompt, so it stays off the UI
    // loop.
    let save = tokio::task::spawn_blocking(move || {
        let result = super::controller_profile::save_to(
            tested.plan,
            &crate::config::OsKeyring,
            &crate::config::config_path(),
            &tested.profile_name,
        );
        // Only a saved profile gets the session its test opened. After a
        // failed save it stays parked, for the retest to ride.
        if result.is_ok()
            && let Some(handover) = handover.lock().ok().and_then(|mut slot| slot.take())
        {
            handover.promote(&live);
        }
        result
    });
    tokio::spawn(async move {
        // The form waits on this result with no key to leave by, so a
        // save that died still has to report.
        let result = save.await.unwrap_or_else(|_| {
            Err("The save stopped unexpectedly; nothing more was written".to_owned())
        });
        let _ = tx.send(Action::SetupSaveFinished { generation, result });
    });
}

/// Connect with `request` and report how it went, or `None` when the test
/// was cancelled.
async fn run(
    request: ProbeRequest,
    cancel: CancellationToken,
    handover: HandoverSlot,
) -> Option<ProbeResult> {
    let profile_name = request.profile_name.clone();
    let site = request.profile.site.clone();
    let totp = request.totp.clone();

    // Resolving credentials reads the keyring, which can block.
    let built = tokio::task::spawn_blocking(move || {
        let defaults = crate::config::load_config()
            .map(|cfg| cfg.defaults)
            .unwrap_or_default();
        crate::config::resolve::resolve_session_connection(
            &request.profile,
            &request.profile_name,
            &request.overrides,
            &defaults,
        )
    })
    .await;
    let mut live = match built {
        Ok(Ok(config)) => config,
        Ok(Err(error)) => return Some(Err(explain_config(&error))),
        Err(_) => {
            return Some(Err(ProbeFailure::new(
                FailureKind::Other,
                "Could not read the stored credentials",
                "",
            )));
        }
    };
    // A sign-in test parks its session for the dashboard, unless the
    // launch asked for no session cache. A typed one-time code overrules
    // that: it can sign in once, so without the hand-over the dashboard
    // could not sign in at all.
    let mut probe = probe_config(&live);
    probe.no_session_cache = true;
    if let Some(sign_in) = SignIn::of(&live)
        && (totp.is_some() || !live.no_session_cache)
        && let Some(cache_name) = Handover::cache_name(&handover, &profile_name, sign_in)
    {
        probe.profile_name = Some(cache_name);
        probe.no_session_cache = false;
        live.no_session_cache = false;
    }
    if totp.is_some() {
        probe.totp_token.clone_from(&totp);
        live.totp_token = totp;
    }
    // What the connection signs in with once launch flags are applied,
    // which can differ from the mode the profile names.
    let mode = match &live.auth {
        AuthCredentials::ApiKey(_) => AuthMode::ApiKey,
        AuthCredentials::Credentials { .. } => AuthMode::Session,
        AuthCredentials::Hybrid { .. } => AuthMode::Hybrid,
        AuthCredentials::Cloud { .. } => AuthMode::Cloud,
    };
    let context = Context {
        mode,
        target: crate::config::redact_url_userinfo(live.url.as_str()),
        site,
        verifies_tls: !matches!(live.tls, unifly_api::TlsVerification::DangerAcceptInvalid),
    };
    let controller = Controller::new(probe);
    let connected = tokio::select! {
        () = cancel.cancelled() => {
            controller.disconnect().await;
            return None;
        }
        connected = controller.connect_inventory() => connected,
    };
    let result = match connected {
        Ok(()) => {
            let warnings = controller.take_warnings().await;
            if let Some(failure) = half_connected(mode, &warnings) {
                Err(failure)
            } else {
                Ok(ProbePass {
                    sites: sites_of(&controller),
                    config: Box::new(live),
                })
            }
        }
        Err(error) => Err(explain(&error, &context)),
    };
    controller.disconnect().await;
    (!cancel.is_cancelled()).then_some(result)
}

/// The sites the connected `controller` read.
fn sites_of(controller: &Controller) -> Vec<SiteHint> {
    controller
        .sites_snapshot()
        .iter()
        .map(|site| SiteHint {
            internal_reference: site.internal_name.clone(),
            display_name: site.name.clone(),
        })
        .collect()
}

/// Why a connect that returned without an error still fails the test: a
/// hybrid connect carries on without its session login and only reports
/// that as a warning, which would let a wrong password pass as tested.
fn half_connected(mode: AuthMode, warnings: &[String]) -> Option<ProbeFailure> {
    if mode != AuthMode::Hybrid {
        return None;
    }
    let warning = warnings.first()?;
    let failure = second_factor_problem(&warning.to_lowercase()).unwrap_or_else(|| {
        ProbeFailure::new(
            FailureKind::Credentials,
            "The API key works, but the username and password were not accepted.",
            "Correct them, or switch the auth mode to API Key if you do not need live events.",
        )
        .at(DraftField::Password)
    });
    Some(failure.with_detail(warning.clone()))
}

/// `live` as the test runs it: one connect and one read, no background
/// refresh, no event stream.
fn probe_config(live: &ControllerConfig) -> ControllerConfig {
    let mut probe = live.clone();
    probe.refresh_interval_secs = 0;
    probe.polling_interval_secs = 0;
    probe.websocket_enabled = false;
    probe
}

/// What a failure is explained against.
#[derive(Debug, Clone)]
pub(crate) struct Context {
    pub(crate) mode: AuthMode,
    /// The controller address, safe to show.
    pub(crate) target: String,
    pub(crate) site: String,
    pub(crate) verifies_tls: bool,
}

/// A profile that could not be turned into a connection at all.
fn explain_config(error: &crate::config::ConfigError) -> ProbeFailure {
    use crate::config::ConfigError;

    match error {
        ConfigError::NoCredentials { .. } => ProbeFailure::new(
            FailureKind::Credentials,
            "This profile has no stored credentials to connect with.",
            "Type them in and press Enter. If they are meant to come from the keyring, \
             check that it is unlocked.",
        ),
        ConfigError::Validation { field, reason } => {
            let at = match field.as_str() {
                "controller" => Some(DraftField::Url),
                "host_id" => Some(DraftField::HostId),
                "auth_mode" => Some(DraftField::AuthMode),
                _ => None,
            };
            let mut failure =
                ProbeFailure::new(FailureKind::Other, format!("{field}: {reason}"), "");
            failure.field = at;
            failure
        }
        other => ProbeFailure::new(FailureKind::Other, other.to_string(), ""),
    }
}

/// Turn a connect error into what went wrong and what to do next.
pub(crate) fn explain(error: &CoreError, context: &Context) -> ProbeFailure {
    let raw = match error {
        // The summary already names the controller; the cause is what the
        // raw error adds.
        CoreError::ConnectionFailed { reason, .. } => reason.clone(),
        other => other.to_string(),
    };
    let failure = match error {
        CoreError::ConnectionFailed { reason, .. } => explain_unreachable(reason, context),
        CoreError::Timeout { timeout_secs } => explain_timeout(*timeout_secs, context),
        CoreError::AuthenticationFailed { message } => explain_rejected(message, context),
        CoreError::SiteNotFound { name, available } => explain_site(
            format!("There is no site \"{name}\" on this controller."),
            available,
        ),
        CoreError::SiteAmbiguous { name, matches } => {
            explain_site(format!("More than one site is called \"{name}\"."), matches)
        }
        CoreError::Unsupported { .. } => ProbeFailure::new(
            FailureKind::Unsupported,
            "This controller does not offer the Integration API that API keys use.",
            "Pick Username / Password as the auth mode (older self-hosted controllers), or \
             update the controller.",
        )
        .at(DraftField::AuthMode),
        CoreError::Api {
            status: Some(401 | 403),
            ..
        } => explain_rejected(&raw, context),
        CoreError::Api { message, .. } if message.contains("NoSiteContext") => ProbeFailure::new(
            FailureKind::Site,
            format!("There is no site \"{}\" on this controller.", context.site),
            "Check the site name. Most controllers have a single site named \"default\".",
        )
        .at(DraftField::Site),
        CoreError::Config { .. } => ProbeFailure::new(
            FailureKind::Unreachable,
            "The address is not one a controller can be reached at.",
            format!("Use the controller's base address, for example {EXAMPLE}."),
        )
        .at(DraftField::Url),
        _ => ProbeFailure::new(
            FailureKind::Other,
            "The controller answered, but the test did not get through.",
            "Check the auth mode and the site, then press Enter to try again.",
        ),
    };
    failure.with_detail(raw)
}

const EXAMPLE: &str = super::controller_profile::EXAMPLE_URL;

fn explain_site(summary: String, sites: &[SiteHint]) -> ProbeFailure {
    let next_step = if sites.is_empty() {
        "Check the site name. Most controllers have a single site named \"default\".".to_owned()
    } else {
        "Pick one of its sites.".to_owned()
    };
    let mut failure = ProbeFailure::new(FailureKind::Site, summary, next_step).at(DraftField::Site);
    failure.sites = sites.to_vec();
    failure
}

fn explain_timeout(timeout_secs: u64, context: &Context) -> ProbeFailure {
    let waited = if timeout_secs == 0 {
        String::new()
    } else {
        format!(" within {timeout_secs} seconds")
    };
    ProbeFailure::new(
        FailureKind::Timeout,
        format!("{} did not answer{waited}.", context.target),
        "Check the address and that this machine can reach it. A slow or remote controller \
         may need a longer timeout.",
    )
    .at(DraftField::Url)
}

fn explain_unreachable(reason: &str, context: &Context) -> ProbeFailure {
    let lower = reason.to_lowercase();
    let target = &context.target;
    if lower.contains("ca cert") || lower.contains("invalid pem") {
        return ProbeFailure::new(
            FailureKind::Tls,
            "The CA certificate file could not be used.",
            "Point CA certificate at a PEM file holding the certificate that signed the \
             controller's, or clear the field.",
        )
        .at(DraftField::CaCert);
    }
    if lower.contains("certificate") || lower.contains("unknownissuer") {
        let next_step = if context.mode == AuthMode::Cloud {
            "Check this machine's clock and its trusted certificates."
        } else if context.verifies_tls {
            "UniFi consoles ship with a self-signed certificate. Turn on Skip TLS \
             verification, or set a CA certificate that signed it."
        } else {
            "The controller's TLS setup was refused even without verification. Check the \
             address and port."
        };
        return ProbeFailure::new(
            FailureKind::Tls,
            format!("The certificate {target} presented could not be verified."),
            next_step,
        )
        .at(DraftField::Insecure);
    }
    if lower.contains("offline") {
        return ProbeFailure::new(
            FailureKind::Unreachable,
            "The cloud console is offline or unreachable.",
            "Check the console's connection in unifi.ui.com and that the Host ID is the \
             right one.",
        )
        .at(DraftField::HostId);
    }
    let (summary, next_step) = if lower.contains("dns")
        || lower.contains("lookup")
        || lower.contains("name or service not known")
    {
        (
            format!("No host could be found for {target}."),
            "Check the address for typos, or use the controller's IP address.",
        )
    } else if lower.contains("refused") {
        (
            format!("Nothing is accepting connections at {target}."),
            "Check the port: UniFi OS consoles use 443, self-hosted controllers 8443.",
        )
    } else {
        (
            format!("Could not reach {target}."),
            "Check the address and that this machine is on a network that can reach the \
             controller.",
        )
    };
    ProbeFailure::new(FailureKind::Unreachable, summary, next_step).at(DraftField::Url)
}

/// A sign-in that failed on its second factor, told from the lowercased
/// error: a code was needed and none given, or the one given was refused.
fn second_factor_problem(lower: &str) -> Option<ProbeFailure> {
    let refused = ["totp", "mfa login failed", "mfa challenge", "mfa cookie"];
    let failure = if lower.contains("two-factor") {
        ProbeFailure::new(
            FailureKind::Mfa,
            "This account signs in with a second factor.",
            "Enter the current 6-digit code from your authenticator app in One-time code and \
             press Enter.",
        )
    } else if refused.iter().any(|sign| lower.contains(sign)) {
        ProbeFailure::new(
            FailureKind::Mfa,
            "The one-time code was not accepted.",
            "Codes change every 30 seconds and work once. Enter a fresh one and press Enter.",
        )
    } else {
        return None;
    };
    Some(failure.at(DraftField::Totp))
}

fn explain_rejected(message: &str, context: &Context) -> ProbeFailure {
    let lower = message.to_lowercase();
    if let Some(failure) = second_factor_problem(&lower) {
        return failure;
    }
    if lower.contains("detect controller platform") {
        // Something answered, but neither login endpoint is there.
        return ProbeFailure::new(
            FailureKind::Unreachable,
            format!(
                "{} answered, but not as a UniFi controller.",
                context.target
            ),
            "Check the address and port: UniFi OS consoles use 443, self-hosted controllers \
             8443.",
        )
        .at(DraftField::Url);
    }
    if lower.contains("cloud console") {
        return ProbeFailure::new(
            FailureKind::Credentials,
            "This API key may not use that cloud console.",
            "Check the Host ID, and that the key was created by an account that owns the \
             console.",
        )
        .at(DraftField::HostId);
    }
    if context.mode == AuthMode::Session {
        return ProbeFailure::new(
            FailureKind::Credentials,
            "The controller did not accept the username and password.",
            "Check both. Use a local admin account: a Ubiquiti cloud (SSO) account cannot \
             sign in here.",
        )
        .at(DraftField::Password);
    }
    ProbeFailure::new(
        FailureKind::Credentials,
        "The controller did not accept the API key.",
        if context.mode == AuthMode::Cloud {
            "Create a Site Manager key at unifi.ui.com under API, and paste it here."
        } else {
            "Create one on the controller under Settings > Control Plane > Integrations, and \
             paste it here."
        },
    )
    .at(DraftField::ApiKey)
}

#[cfg(test)]
mod tests {
    use unifly_api::{CoreError, SiteHint};

    use super::{Context, FailureKind, ProbeSlot, Tone, explain, half_connected};
    use crate::tui::forms::controller_profile::{AuthMode, ControllerProfileDraft, DraftField};

    fn context(mode: AuthMode) -> Context {
        Context {
            mode,
            target: "https://10.0.0.1/".into(),
            site: "default".into(),
            verifies_tls: true,
        }
    }

    fn unreachable(reason: &str) -> CoreError {
        CoreError::ConnectionFailed {
            url: "https://10.0.0.1/".into(),
            reason: reason.into(),
        }
    }

    #[test]
    fn a_certificate_failure_points_at_the_tls_choice() {
        let failure = explain(
            &unreachable(
                "error sending request: client error (Connect): invalid peer certificate: \
                 UnknownIssuer",
            ),
            &context(AuthMode::ApiKey),
        );

        assert_eq!(failure.kind, FailureKind::Tls);
        assert_eq!(failure.field, Some(DraftField::Insecure));
        assert!(failure.next_step.contains("Skip TLS verification"));
        assert!(
            failure
                .detail
                .as_deref()
                .is_some_and(|d| d.contains("UnknownIssuer")),
            "{failure:?}"
        );
    }

    #[test]
    fn unreachable_controllers_say_which_way_they_are_unreachable() {
        for (reason, summary, field) in [
            (
                "tcp connect error: Connection refused (os error 111)",
                "Nothing is accepting",
                DraftField::Url,
            ),
            (
                "dns error: failed to lookup address information",
                "No host could be found",
                DraftField::Url,
            ),
            ("No route to host", "Could not reach", DraftField::Url),
            (
                "TLS error: failed to read CA cert: No such file",
                "CA certificate file",
                DraftField::CaCert,
            ),
            (
                "cloud console offline or unreachable",
                "cloud console is offline",
                DraftField::HostId,
            ),
        ] {
            let failure = explain(&unreachable(reason), &context(AuthMode::ApiKey));
            assert!(failure.summary.contains(summary), "{reason}: {failure:?}");
            assert_eq!(failure.field, Some(field), "{reason}");
            assert!(!failure.next_step.is_empty(), "{reason}");
        }
    }

    #[test]
    fn a_timeout_never_claims_zero_seconds() {
        let failure = explain(
            &CoreError::Timeout { timeout_secs: 0 },
            &context(AuthMode::ApiKey),
        );
        assert_eq!(failure.kind, FailureKind::Timeout);
        assert_eq!(failure.summary, "https://10.0.0.1/ did not answer.");

        let failure = explain(
            &CoreError::Timeout { timeout_secs: 30 },
            &context(AuthMode::ApiKey),
        );
        assert!(failure.summary.contains("within 30 seconds"), "{failure:?}");
    }

    #[test]
    fn rejected_sign_ins_name_the_credential_at_fault() {
        let rejected = |message: &str, mode| {
            explain(
                &CoreError::AuthenticationFailed {
                    message: message.into(),
                },
                &context(mode),
            )
        };

        let mfa = rejected(
            "Two-factor authentication token required",
            AuthMode::Session,
        );
        assert_eq!(mfa.kind, FailureKind::Mfa);
        assert_eq!(mfa.field, Some(DraftField::Totp));

        let code = rejected("TOTP token must be exactly 6 digits", AuthMode::Hybrid);
        assert_eq!(code.field, Some(DraftField::Totp));
        // What the controller says when it refuses the code itself.
        let refused = rejected(
            "MFA login failed (HTTP 401 Unauthorized): {\"code\":\"AUTHENTICATION_FAILED\"}",
            AuthMode::Session,
        );
        assert_eq!(refused.kind, FailureKind::Mfa);
        assert_eq!(refused.field, Some(DraftField::Totp));
        assert!(refused.summary.contains("not accepted"), "{refused:?}");

        let password = rejected("api.err.Invalid", AuthMode::Session);
        assert_eq!(password.kind, FailureKind::Credentials);
        assert_eq!(password.field, Some(DraftField::Password));
        assert!(password.next_step.contains("local admin"));

        // Not a credential problem at all: nothing UniFi lives there.
        let elsewhere = rejected(
            "could not detect controller platform: both login probes returned 404",
            AuthMode::ApiKey,
        );
        assert_eq!(elsewhere.kind, FailureKind::Unreachable);
        assert_eq!(elsewhere.field, Some(DraftField::Url));

        let key = rejected("Invalid API key", AuthMode::ApiKey);
        assert_eq!(key.field, Some(DraftField::ApiKey));
        assert!(key.next_step.contains("Integrations"));
    }

    #[test]
    fn a_missing_site_carries_the_sites_to_pick_from() {
        let sites = vec![
            SiteHint {
                internal_reference: "default".into(),
                display_name: "Home".into(),
            },
            SiteHint {
                internal_reference: "lab".into(),
                display_name: "Lab".into(),
            },
        ];
        let failure = explain(
            &CoreError::SiteNotFound {
                name: "office".into(),
                available: sites.clone(),
            },
            &context(AuthMode::ApiKey),
        );

        assert_eq!(failure.kind, FailureKind::Site);
        assert_eq!(failure.field, Some(DraftField::Site));
        assert_eq!(failure.sites, sites);
        assert!(failure.summary.contains("\"office\""));
    }

    /// A hybrid connect carries on without its session login and only
    /// warns, so the warning has to fail the test or a wrong password
    /// would be saved as tested.
    #[test]
    fn a_hybrid_connect_that_only_half_worked_fails_the_test() {
        let warning = ["Session login failed: Authentication failed".to_owned()];

        let failure = half_connected(AuthMode::Hybrid, &warning)
            .expect("a failed session login is a failed test");
        assert_eq!(failure.kind, FailureKind::Credentials);
        assert_eq!(failure.field, Some(DraftField::Password));
        assert!(failure.detail.is_some());

        // A missing second factor is not a wrong password.
        let mfa = ["Session login failed: Two-factor authentication token required".to_owned()];
        let failure = half_connected(AuthMode::Hybrid, &mfa).expect("fails");
        assert_eq!(failure.kind, FailureKind::Mfa);
        assert_eq!(failure.field, Some(DraftField::Totp));

        // Cloud always notes what it cannot do; that is not a failure.
        assert!(half_connected(AuthMode::Cloud, &warning).is_none());
        assert!(half_connected(AuthMode::Hybrid, &[]).is_none());
    }

    #[test]
    fn a_private_failure_names_only_its_class() {
        let failure = explain(
            &unreachable("Connection refused"),
            &context(AuthMode::ApiKey),
        );

        let tones = |private| -> Vec<Tone> {
            failure
                .paragraphs(private)
                .iter()
                .map(|(tone, _)| *tone)
                .collect()
        };
        assert_eq!(tones(false), [Tone::Error, Tone::Advice, Tone::Detail]);
        assert_eq!(tones(true), [Tone::Error]);
        let shown = failure.paragraphs(true)[0].1;
        assert!(!shown.contains("10.0.0.1"), "{shown}");
    }

    fn sign_in(username: &str, password: &str) -> super::SignIn {
        super::SignIn {
            url: "https://10.0.0.1/".into(),
            username: username.into(),
            password: secrecy::SecretString::from(password.to_owned()),
        }
    }

    /// A retest of the same sign-in reuses the session the last test
    /// opened (a one-time code cannot sign in twice); changing what is
    /// signed in with starts over, so nothing typed goes untested.
    #[test]
    fn a_retest_rides_its_own_session_and_a_new_sign_in_starts_over() {
        let slot = super::HandoverSlot::default();
        let name = |who: super::SignIn| super::Handover::cache_name(&slot, "home", who);

        let first = name(sign_in("admin", "pw")).expect("name");
        assert!(first.starts_with("home.setup-"), "{first}");
        assert_ne!(first, "home", "never the profile's own cache");
        assert_eq!(
            name(sign_in("admin", "pw")).as_deref(),
            Some(first.as_str())
        );

        let retyped = name(sign_in("admin", "other")).expect("name");
        assert_ne!(retyped, first, "a new password signs in afresh");
        let another = name(sign_in("root", "other")).expect("name");
        assert_ne!(another, retyped, "so does a new user");
    }

    fn request(name: &str) -> (super::ProbeRequest, super::SavePlan) {
        let draft = ControllerProfileDraft::default();
        (
            super::ProbeRequest {
                profile: draft.to_profile(),
                profile_name: name.into(),
                overrides: crate::config::resolve::ConnectionOverrides::default(),
                totp: None,
            },
            draft.plan_save(name),
        )
    }

    /// Only the test that is still the current one may claim its result:
    /// a cancelled or replaced test saves nothing.
    #[test]
    fn only_the_current_test_can_claim_its_result() {
        let mut slot = ProbeSlot::default();
        let (probe, plan) = request("home");
        let first = slot.start(None, probe, plan);
        assert!(slot.is_running());

        let (probe, plan) = request("work");
        let second = slot.start(None, probe, plan);
        assert_ne!(first, second);
        assert!(slot.claim(first).is_none(), "replaced by the second test");
        assert!(slot.is_running(), "a stale result leaves the test running");

        let tested = slot.claim(second).expect("the current test");
        assert_eq!(tested.profile_name, "work");
        assert!(!slot.is_running());
        assert!(slot.claim(second).is_none(), "a result is claimed once");

        let (probe, plan) = request("home");
        let third = slot.start(None, probe, plan);
        slot.cancel();
        assert!(slot.claim(third).is_none(), "cancelled");
    }
}
