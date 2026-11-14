use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unifly_api::SiteHint;

use super::{OnboardingScreen, TestPhase, WizardField, WizardStep};
use crate::tui::action::Action;
use crate::tui::component::Component;
use crate::tui::forms::controller_profile::{AuthMode, DraftField, save_to, write_profile};
use crate::tui::forms::probe::{FailureKind, ProbeFailure, ProbePass, ProbeResult};
use crate::tui::forms::session::Session;
use crate::tui::table::render_lines;
use crate::tui::widgets::click::click_label;

/// In-memory stand-in for the OS keyring.
#[derive(Default)]
struct MemoryStore(std::cell::RefCell<std::collections::HashMap<String, String>>);

impl MemoryStore {
    fn secret(&self, key: &str) -> Option<String> {
        self.0.borrow().get(key).cloned()
    }
}

impl crate::config::SecretStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>, crate::config::SecretStoreError> {
        Ok(self.secret(key))
    }

    fn set(&self, key: &str, secret: &str) -> Result<(), crate::config::SecretStoreError> {
        self.0
            .borrow_mut()
            .insert(key.to_owned(), secret.to_owned());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), crate::config::SecretStoreError> {
        self.0.borrow_mut().remove(key);
        Ok(())
    }
}

fn wizard() -> OnboardingScreen {
    OnboardingScreen::in_session("default", Session::default())
}

fn press(screen: &mut OnboardingScreen, code: KeyCode) -> Option<Action> {
    screen.handle_key_input(KeyEvent::new(code, KeyModifiers::NONE))
}

fn ctrl(screen: &mut OnboardingScreen, ch: char) {
    screen.handle_key_input(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL));
}

fn type_text(screen: &mut OnboardingScreen, text: &str) {
    for ch in text.chars() {
        press(screen, KeyCode::Char(ch));
    }
}

fn frame(screen: &OnboardingScreen, width: u16, height: u16) -> Vec<String> {
    render_lines(width, height, |frame, area| screen.render(frame, area))
}

fn sites(names: &[&str]) -> Vec<SiteHint> {
    names
        .iter()
        .map(|name| SiteHint {
            internal_reference: (*name).into(),
            display_name: (*name).into(),
        })
        .collect()
}

fn pass(site: &str, names: &[&str]) -> ProbePass {
    ProbePass {
        config: Box::new(unifly_api::ControllerConfig {
            site: site.into(),
            ..unifly_api::ControllerConfig::default()
        }),
        sites: sites(names),
    }
}

fn failure(kind: FailureKind, field: DraftField) -> ProbeFailure {
    ProbeFailure {
        kind,
        summary: "The test failed.".into(),
        next_step: "Do the next thing.".into(),
        detail: None,
        sites: Vec::new(),
        field: Some(field),
    }
}

/// Deliver `result` as the outcome of the test the wizard is running.
fn finish(screen: &mut OnboardingScreen, result: ProbeResult) {
    let generation = screen.probe.generation();
    screen.apply_action(&Action::SetupProbeFinished {
        generation,
        result: Box::new(result),
    });
}

fn saved(screen: &mut OnboardingScreen, result: Result<Vec<String>, String>) {
    let generation = screen.probe.generation();
    screen.apply_action(&Action::SetupSaveFinished { generation, result });
}

/// A wizard on the Testing step with an API key profile under test.
fn testing() -> OnboardingScreen {
    let mut screen = wizard();
    press(&mut screen, KeyCode::Enter);
    press(&mut screen, KeyCode::Enter);
    type_text(&mut screen, "https://10.0.0.1");
    press(&mut screen, KeyCode::Enter);
    type_text(&mut screen, "typed-key");
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.step, WizardStep::Testing);
    assert_eq!(screen.phase, TestPhase::Testing);
    screen
}

/// The wizard also opens for a profile that exists but cannot connect
/// (an unset `api_key_env`, a locked keyring). Finishing it keeps the
/// settings the wizard does not show.
#[test]
fn setup_for_an_existing_profile_keeps_what_the_wizard_does_not_show() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[profiles.work]\n\
         controller = \"https://10.0.0.1\"\n\
         site = \"lab\"\n\
         api_key_env = \"WORK_KEY\"\n\
         ca_cert = \"/etc/ssl/work-ca.pem\"\n\
         timeout = 45\n\
         totp_command = [\"op\", \"item\", \"get\", \"work\", \"--otp\"]\n",
    )
    .expect("write config");
    let cfg = crate::config::load_config_from(&path).expect("load config");
    let store = MemoryStore::default();

    let mut screen = OnboardingScreen::for_existing_profile(
        "work",
        &cfg.profiles["work"],
        &cfg.defaults,
        &store,
    );
    screen.draft.url.set("https://10.0.0.2");
    let save = screen.draft.plan_save(&screen.profile_name);
    save_to(save, &store, &path, &screen.profile_name).expect("save");

    let cfg = crate::config::load_config_from(&path).expect("load saved config");
    let work = &cfg.profiles["work"];
    assert_eq!(work.controller, "https://10.0.0.2");
    assert_eq!(
        work.ca_cert.as_deref(),
        Some(std::path::Path::new("/etc/ssl/work-ca.pem"))
    );
    assert_eq!(work.timeout, Some(45));
    assert_eq!(work.api_key_env.as_deref(), Some("WORK_KEY"));
    assert_eq!(work.site, "lab");
    assert_eq!(
        work.totp_command.as_deref(),
        Some(&["op", "item", "get", "work", "--otp"].map(String::from)[..])
    );
}

/// The wizard starts on the profile's auth mode, so stepping through it
/// keeps that mode instead of resetting it to the first one.
#[test]
fn setup_for_an_existing_profile_starts_on_its_auth_mode() {
    let profile: crate::config::Profile = toml::from_str(
        "controller = \"https://10.0.0.1\"\nauth_mode = \"session\"\nusername = \"admin\"\n",
    )
    .expect("profile parses");
    let mut screen = OnboardingScreen::for_existing_profile(
        "work",
        &profile,
        &crate::config::Defaults::default(),
        &MemoryStore::default(),
    );
    screen.step = WizardStep::AuthMode;

    screen.advance();

    assert_eq!(screen.draft.auth_mode, AuthMode::Session);
    assert_eq!(screen.draft.username.text(), "admin");
}

/// `unifly tui -p work` opens the wizard when `work` is missing.
/// Finishing it adds `work` and leaves the working `default` profile,
/// its keyring secret, and the default pointer as they were.
#[test]
fn setup_for_a_named_profile_leaves_the_other_profiles_alone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "default_profile = \"default\"\n\n\
         [profiles.default]\ncontroller = \"https://10.0.0.1\"\n",
    )
    .expect("write config");
    let store = MemoryStore::default();
    crate::config::SecretStore::set(&store, "default/api-key", "default-secret")
        .expect("seed keyring");

    let mut screen = OnboardingScreen::in_session("work", Session::default());
    screen.draft.url.set("https://192.168.1.1");
    screen.draft.api_key.set("work-secret");
    let save = screen.draft.plan_save(&screen.profile_name);
    save_to(save, &store, &path, &screen.profile_name).expect("save");

    let cfg = crate::config::load_config_from(&path).expect("load saved config");
    assert_eq!(cfg.default_profile.as_deref(), Some("default"));
    assert_eq!(cfg.profiles["default"].controller, "https://10.0.0.1");
    assert_eq!(cfg.profiles["work"].controller, "https://192.168.1.1");
    assert_eq!(
        store.secret("default/api-key").as_deref(),
        Some("default-secret")
    );
    assert_eq!(store.secret("work/api-key").as_deref(), Some("work-secret"));
}

/// Onboarding also opens when the selected profile is missing or fails
/// to resolve, so the file it saves into can hold other profiles and
/// settings. Finishing the wizard adds its profile and keeps the rest,
/// including a default that still names a saved profile.
#[test]
fn save_profile_keeps_the_rest_of_the_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "default_profile = \"work\"\n\n\
         [defaults]\ntimeout = 45\ntheme = \"nord\"\n\n\
         [profiles.work]\ncontroller = \"https://10.0.0.1\"\n\n\
         [demo]\nredact_names = [\"Bliss\"]\n",
    )
    .expect("write config");

    let mut screen = wizard();
    screen.draft.url.set("https://192.168.1.1");
    screen.draft.api_key.set("api-key");
    write_profile(&path, "default", screen.draft.to_profile()).expect("save");

    let cfg = crate::config::load_config_from(&path).expect("load saved config");
    assert_eq!(cfg.default_profile.as_deref(), Some("work"));
    assert_eq!(cfg.profiles["default"].controller, "https://192.168.1.1");
    assert_eq!(cfg.profiles["work"].controller, "https://10.0.0.1");
    assert_eq!(cfg.defaults.timeout, 45);
    assert_eq!(cfg.defaults.theme.as_deref(), Some("nord"));
    assert_eq!(cfg.demo.redact_names, ["Bliss"]);
}

/// On a first run the config has no saved profile to be the default, so
/// the wizard's profile becomes it.
#[test]
fn save_profile_makes_a_first_profile_the_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");

    write_profile(&path, "work", wizard().draft.to_profile()).expect("save");

    let cfg = crate::config::load_config_from(&path).expect("load saved config");
    assert_eq!(cfg.default_profile.as_deref(), Some("work"));
    assert!(cfg.profiles.contains_key("work"));
}

/// A config that fails to load must fail the save and stay as it was.
#[test]
fn save_profile_leaves_a_config_that_fails_to_load_untouched() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    let broken = "[defaults]\ntimeout = \"thirty\"\n";
    std::fs::write(&path, broken).expect("write config");

    let result = write_profile(&path, "default", wizard().draft.to_profile());

    assert!(result.is_err(), "the save should fail");
    assert_eq!(std::fs::read_to_string(&path).expect("read config"), broken);
}

/// The auth mode decides whether there is an address to ask for at all,
/// so it is asked first, and Cloud goes straight to its credentials.
#[test]
fn the_auth_mode_is_asked_before_the_address_and_cloud_has_none() {
    let mut screen = wizard();
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.step, WizardStep::AuthMode);
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.step, WizardStep::Url);

    press(&mut screen, KeyCode::Esc);
    for _ in 0..3 {
        press(&mut screen, KeyCode::Down);
    }
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.draft.auth_mode, AuthMode::Cloud);
    assert_eq!(screen.step, WizardStep::Credentials);
    assert_eq!(
        screen.fields(),
        [
            WizardField::Text(DraftField::ApiKey),
            WizardField::Text(DraftField::HostId)
        ]
    );
    assert_eq!(
        screen.draft.to_profile().controller,
        crate::config::DEFAULT_CLOUD_CONTROLLER_URL
    );

    // Back out of Cloud: nothing it implied sticks to the draft.
    press(&mut screen, KeyCode::Esc);
    assert_eq!(screen.step, WizardStep::AuthMode);
    for _ in 0..3 {
        press(&mut screen, KeyCode::Up);
    }
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.step, WizardStep::Url);
    assert!(screen.draft.url.is_empty());
    assert!(!screen.draft.insecure);
}

/// The example address is a hint, not text: typing starts from an empty
/// field, and an empty field is not accepted as the example.
#[test]
fn the_example_address_is_a_placeholder_typing_replaces() {
    let mut screen = wizard();
    press(&mut screen, KeyCode::Enter);
    press(&mut screen, KeyCode::Enter);
    let rows = frame(&screen, 80, 24);
    assert!(
        rows.iter().any(|row| row.contains("https://192.168.1.1")),
        "{rows:?}"
    );

    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.step, WizardStep::Url, "an empty address is refused");
    assert!(screen.error.is_some());

    type_text(&mut screen, "10.0.0.9");
    assert_eq!(screen.draft.url.text(), "10.0.0.9");
    assert_eq!(screen.error, None, "typing clears the complaint");

    // No scheme: the wizard offers the https:// form and waits.
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.step, WizardStep::Url);
    assert_eq!(screen.draft.url.text(), "https://10.0.0.9");
    assert!(
        screen
            .notice
            .as_deref()
            .is_some_and(|n| n.contains("https://"))
    );
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.step, WizardStep::Credentials);
}

/// TLS verification is the user's call: the wizard shows the choice and
/// saves what was picked, verification on unless they turn it off.
#[test]
fn skipping_tls_verification_is_an_explicit_choice() {
    let mut screen = wizard();
    press(&mut screen, KeyCode::Enter);
    press(&mut screen, KeyCode::Enter);
    let rows = frame(&screen, 80, 24);
    assert!(
        rows.iter().any(|row| row.contains("Skip TLS verify   [ ]")),
        "{rows:?}"
    );
    assert_eq!(screen.draft.to_profile().insecure, Some(false));

    press(&mut screen, KeyCode::Tab);
    press(&mut screen, KeyCode::Char(' '));
    assert_eq!(screen.draft.to_profile().insecure, Some(true));
    assert!(screen.draft.url.is_empty(), "the space toggled, not typed");
}

#[test]
fn fields_move_both_ways_and_a_problem_takes_the_focus() {
    let mut screen = wizard();
    screen.auth_mode_index = AuthMode::Hybrid.index();
    screen.step = WizardStep::AuthMode;
    press(&mut screen, KeyCode::Enter);
    type_text(&mut screen, "https://10.0.0.1");
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.active_text_field(), Some(DraftField::ApiKey));

    press(&mut screen, KeyCode::Down);
    assert_eq!(screen.active_text_field(), Some(DraftField::Username));
    press(&mut screen, KeyCode::BackTab);
    assert_eq!(screen.active_text_field(), Some(DraftField::ApiKey));
    press(&mut screen, KeyCode::Up);
    assert_eq!(screen.active_text_field(), Some(DraftField::Totp), "wraps");
    press(&mut screen, KeyCode::Tab);
    assert_eq!(screen.active_text_field(), Some(DraftField::ApiKey));

    // Enter from the first field: the complaint lands on the field at fault.
    type_text(&mut screen, "key");
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.step, WizardStep::Credentials);
    assert_eq!(screen.active_text_field(), Some(DraftField::Username));
    assert_eq!(screen.error.as_deref(), Some("Username cannot be empty"));
}

#[test]
fn fields_edit_like_a_line_editor_and_chords_type_nothing() {
    let mut screen = wizard();
    press(&mut screen, KeyCode::Enter);
    press(&mut screen, KeyCode::Enter);

    type_text(&mut screen, "https://10.0.0.1");
    press(&mut screen, KeyCode::Home);
    press(&mut screen, KeyCode::Delete);
    press(&mut screen, KeyCode::Char('H'));
    assert_eq!(screen.draft.url.text(), "Https://10.0.0.1");
    ctrl(&mut screen, 'x');
    assert_eq!(screen.draft.url.text(), "Https://10.0.0.1", "no stray x");
    press(&mut screen, KeyCode::End);
    ctrl(&mut screen, 'u');
    assert_eq!(screen.draft.url.text(), "", "Ctrl+U clears to the start");

    // A pasted value with a trailing newline goes in whole and submits
    // nothing.
    assert!(screen.captures_input());
    screen.handle_paste("https://10.0.0.7\n");
    assert_eq!(screen.draft.url.text(), "https://10.0.0.7");
    assert_eq!(screen.step, WizardStep::Url);
}

#[test]
fn a_secret_is_shown_only_while_revealed() {
    let mut screen = wizard();
    press(&mut screen, KeyCode::Enter);
    press(&mut screen, KeyCode::Enter);
    type_text(&mut screen, "https://10.0.0.1");
    press(&mut screen, KeyCode::Enter);
    type_text(&mut screen, "s3cret-key");

    let hidden = frame(&screen, 80, 24).join("\n");
    assert!(!hidden.contains("s3cret-key"), "{hidden}");
    assert!(hidden.contains("Ctrl+T show"), "{hidden}");

    ctrl(&mut screen, 't');
    let shown = frame(&screen, 80, 24).join("\n");
    assert!(shown.contains("s3cret-key"), "{shown}");
    assert_eq!(screen.draft.api_key.text(), "s3cret-key", "t was not typed");
}

/// A cancelled test must stay cancelled: its result, arriving later, may
/// not move the wizard, save, or put an error on the step now showing.
#[test]
fn a_cancelled_test_changes_nothing_when_its_result_arrives() {
    let mut screen = testing();
    let cancelled = screen.probe.generation();

    press(&mut screen, KeyCode::Esc);
    assert_eq!(screen.step, WizardStep::Credentials);
    assert_eq!(screen.phase, TestPhase::Idle);

    for result in [
        Ok(pass("default", &["default"])),
        Err(failure(FailureKind::Credentials, DraftField::ApiKey)),
    ] {
        screen.apply_action(&Action::SetupProbeFinished {
            generation: cancelled,
            result: Box::new(result),
        });
        assert_eq!(screen.step, WizardStep::Credentials);
        assert_eq!(screen.phase, TestPhase::Idle, "nothing is being saved");
        assert!(screen.failure.is_none());
        assert!(screen.tested.is_none());
    }
}

/// Esc then Enter starts a second test; the first one's result is stale
/// and only the second may finish the step.
#[test]
fn only_the_latest_test_finishes_the_step() {
    let mut screen = testing();
    let first = screen.probe.generation();
    press(&mut screen, KeyCode::Esc);
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.phase, TestPhase::Testing);
    assert_ne!(screen.probe.generation(), first);

    screen.apply_action(&Action::SetupProbeFinished {
        generation: first,
        result: Box::new(Err(failure(FailureKind::Timeout, DraftField::Url))),
    });
    assert_eq!(
        screen.phase,
        TestPhase::Testing,
        "the stale failure is dropped"
    );
    assert!(screen.failure.is_none());

    finish(&mut screen, Ok(pass("default", &["default"])));
    assert_eq!(screen.phase, TestPhase::Saving);
}

/// What Done connects with is the connection that was tested, and what
/// the save reported is on screen before the user moves on.
#[test]
fn done_applies_the_tested_connection_and_shows_what_the_save_reported() {
    let mut screen = testing();
    finish(&mut screen, Ok(pass("default", &["default"])));
    assert_eq!(screen.phase, TestPhase::Saving);
    assert!(press(&mut screen, KeyCode::Esc).is_none());
    assert_eq!(screen.phase, TestPhase::Saving, "a save runs to its end");

    saved(
        &mut screen,
        Ok(vec![
            "No usable keyring (locked); saved the API key to config.toml in plaintext".into(),
        ]),
    );
    assert_eq!(screen.step, WizardStep::Done);
    let rows = frame(&screen, 80, 24).join("\n");
    assert!(rows.contains("Connected and saved"), "{rows}");
    assert!(rows.contains("plaintext"), "{rows}");

    // Editing after the fact cannot change what gets applied.
    screen.draft.site.set("elsewhere");
    assert!(
        press(&mut screen, KeyCode::Esc).is_none(),
        "nothing to go back to"
    );
    assert_eq!(screen.step, WizardStep::Done);
    let Some(Action::ApplyConnection {
        profile_name,
        config,
        notices,
        ..
    }) = press(&mut screen, KeyCode::Enter)
    else {
        panic!("Done connects");
    };
    assert_eq!(profile_name, "default");
    assert_eq!(config.site, "default");
    assert_eq!(notices.len(), 1);
}

#[test]
fn a_failed_save_is_reported_and_enter_tests_again() {
    let mut screen = testing();
    finish(&mut screen, Ok(pass("default", &["default"])));
    saved(
        &mut screen,
        Err("Connected, but failed to save config: disk full".into()),
    );

    assert_eq!(screen.step, WizardStep::Testing);
    assert_eq!(screen.phase, TestPhase::Idle);
    let rows = frame(&screen, 80, 24).join("\n");
    assert!(rows.contains("Connected, but not saved"), "{rows}");
    assert!(rows.contains("disk full"), "{rows}");
    assert_eq!(rows.matches("disk full").count(), 1, "shown once: {rows}");

    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.phase, TestPhase::Testing);
}

/// A failed test explains itself once, in full, and its keys work: Enter
/// tries again, and Esc goes to the field the failure is about.
#[test]
fn a_failed_test_is_explained_and_can_be_retried_or_fixed() {
    let mut screen = testing();
    let mut refused = failure(FailureKind::Unreachable, DraftField::Url);
    refused.summary = "Nothing is accepting connections at https://10.0.0.1/.".into();
    refused.next_step = "Check the port: UniFi OS consoles use 443.".into();
    refused.detail = Some("tcp connect error: Connection refused (os error 111)".into());
    finish(&mut screen, Err(refused));

    let rows = frame(&screen, 80, 24).join("\n");
    assert!(rows.contains("Could not connect"), "{rows}");
    assert_eq!(rows.matches("Nothing is accepting").count(), 1, "{rows}");
    assert!(rows.contains("Check the port"), "{rows}");
    assert!(rows.contains("Connection refused"), "{rows}");
    assert!(rows.contains("Enter retry"), "{rows}");
    assert!(!rows.contains("cancel"), "{rows}");

    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.phase, TestPhase::Testing, "Enter retries");

    finish(
        &mut screen,
        Err(failure(FailureKind::Unreachable, DraftField::Url)),
    );
    press(&mut screen, KeyCode::Esc);
    assert_eq!(screen.step, WizardStep::Url);
    assert_eq!(screen.active_text_field(), Some(DraftField::Url));
}

/// A certificate that cannot be verified is the one failure the user may
/// simply accept: the wizard offers it, and accepting is what turns
/// verification off.
#[test]
fn an_unverified_certificate_can_be_accepted_from_the_failure() {
    let mut screen = testing();
    finish(
        &mut screen,
        Err(failure(FailureKind::Tls, DraftField::Insecure)),
    );
    let rows = frame(&screen, 80, 24).join("\n");
    assert!(rows.contains("accept the certificate and retry"), "{rows}");
    assert!(!screen.draft.insecure);

    press(&mut screen, KeyCode::Char('t'));
    assert!(screen.draft.insecure);
    assert_eq!(screen.phase, TestPhase::Testing);

    // Already skipping verification: there is nothing left to accept.
    finish(
        &mut screen,
        Err(failure(FailureKind::Tls, DraftField::Insecure)),
    );
    assert!(!screen.offers_trust());
    press(&mut screen, KeyCode::Char('t'));
    assert_eq!(screen.phase, TestPhase::Idle);
}

/// With several sites on the controller the wizard asks which one, and
/// does not test twice when the answer is the one that already passed.
#[test]
fn several_sites_are_offered_and_the_tested_one_saves_without_a_retest() {
    let mut screen = testing();
    let tested = screen.probe.generation();
    finish(&mut screen, Ok(pass("default", &["default", "lab"])));

    assert_eq!(screen.step, WizardStep::Site);
    let rows = frame(&screen, 80, 24).join("\n");
    assert!(rows.contains("\u{25B8} default"), "{rows}");
    assert!(rows.contains("lab"), "{rows}");

    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.phase, TestPhase::Saving);
    assert_eq!(screen.probe.generation(), tested, "no second test");
}

#[test]
fn picking_another_site_tests_that_site() {
    let mut screen = testing();
    let first = screen.probe.generation();
    finish(&mut screen, Ok(pass("default", &["default", "lab"])));

    press(&mut screen, KeyCode::Down);
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.draft.site.text(), "lab");
    assert_eq!(screen.phase, TestPhase::Testing);
    assert_ne!(screen.probe.generation(), first);

    // The answer was given: a pass now saves instead of asking again.
    finish(&mut screen, Ok(pass("lab", &["default", "lab"])));
    assert_eq!(screen.phase, TestPhase::Saving);
}

/// A site the controller does not have is not a dead end: its sites come
/// back with the failure and become the choices.
#[test]
fn a_missing_site_turns_into_a_choice_of_the_sites_there_are() {
    let mut screen = testing();
    let mut missing = failure(FailureKind::Site, DraftField::Site);
    missing.summary = "There is no site \"default\" on this controller.".into();
    missing.sites = sites(&["home", "lab"]);
    finish(&mut screen, Err(missing));

    assert_eq!(screen.step, WizardStep::Site);
    assert!(screen.picks_site());
    let rows = frame(&screen, 80, 24).join("\n");
    assert!(rows.contains("There is no site"), "{rows}");
    assert!(rows.contains("\u{25B8} home"), "{rows}");

    press(&mut screen, KeyCode::Down);
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.draft.site.text(), "lab");
    assert_eq!(screen.phase, TestPhase::Testing);
}

#[test]
fn the_spinner_asks_for_a_frame_on_every_tick_while_it_runs() {
    let mut screen = testing();
    assert!(matches!(
        screen.apply_action(&Action::Tick),
        Some(Action::Invalidate)
    ));
    press(&mut screen, KeyCode::Esc);
    assert!(screen.apply_action(&Action::Tick).is_none());
}

/// A profile that exists but could not connect opens the wizard too, and
/// it says so instead of claiming there is no configuration.
#[test]
fn a_repair_says_why_the_wizard_opened() {
    let profile: crate::config::Profile =
        toml::from_str("controller = \"https://10.0.0.1\"\napi_key_env = \"WORK_KEY\"\n")
            .expect("profile parses");
    let mut screen = OnboardingScreen::for_existing_profile(
        "work",
        &profile,
        &crate::config::Defaults::default(),
        &MemoryStore::default(),
    );
    screen.session.setup_reason = Some("no credentials configured for profile 'work'".into());

    let rows = frame(&screen, 80, 24).join("\n");
    assert!(rows.contains("\"work\" could not connect"), "{rows}");
    assert!(rows.contains("no credentials configured"), "{rows}");
    assert!(!rows.contains("No configuration found"), "{rows}");

    let fresh = frame(&wizard(), 80, 24).join("\n");
    assert!(fresh.contains("No configuration found"), "{fresh}");
}

/// A code given at launch is offered for the sign-in, and a typed one
/// goes with the test.
#[test]
fn a_one_time_code_reaches_the_sign_in() {
    let session = Session {
        launch_totp: Some(secrecy::SecretString::from("123456".to_owned())),
        ..Session::default()
    };
    let mut screen = OnboardingScreen::in_session("default", session);
    screen.auth_mode_index = AuthMode::Session.index();
    screen.step = WizardStep::AuthMode;
    press(&mut screen, KeyCode::Enter);
    type_text(&mut screen, "https://10.0.0.1");
    press(&mut screen, KeyCode::Enter);

    assert_eq!(
        screen.fields(),
        [
            WizardField::Text(DraftField::Username),
            WizardField::Text(DraftField::Password),
            WizardField::Text(DraftField::Totp)
        ]
    );
    assert_eq!(screen.draft.totp.text(), "123456");
    assert!(screen.draft.totp_code().is_some());

    // Only digits go into the code.
    press(&mut screen, KeyCode::BackTab);
    ctrl(&mut screen, 'u');
    type_text(&mut screen, "65x43 21");
    assert_eq!(screen.draft.totp.text(), "654321");
}

/// Every field of the tallest step stays on screen and usable when the
/// terminal is short, and no size makes the wizard panic.
#[test]
fn the_wizard_fits_a_short_terminal() {
    let mut screen = wizard();
    screen.auth_mode_index = AuthMode::Hybrid.index();
    screen.step = WizardStep::AuthMode;
    press(&mut screen, KeyCode::Enter);
    type_text(&mut screen, "https://10.0.0.1");
    press(&mut screen, KeyCode::Enter);
    screen.error = Some("Username cannot be empty".into());

    for (width, height) in [(80, 24), (80, 16), (60, 13)] {
        let rows = frame(&screen, width, height).join("\n");
        for label in ["API key", "Username", "Password", "One-time code"] {
            assert!(rows.contains(label), "{label} at {width}x{height}:\n{rows}");
        }
        assert!(rows.contains("Username cannot be empty"), "{rows}");
        assert!(rows.contains("Enter test"), "{rows}");
    }

    // Shorter than its fields: the rows follow the focus, and the hints
    // keep the keys that move between steps.
    screen.error = None;
    for (index, label) in ["API key", "Username", "Password", "One-time code"]
        .iter()
        .enumerate()
    {
        screen.field = index;
        let rows = frame(&screen, 40, 10).join("\n");
        assert!(
            rows.contains(&format!("\u{25B8} {label}")),
            "{label}:\n{rows}"
        );
        assert!(rows.contains("Esc back"), "{rows}");
    }

    for step in [
        WizardStep::Welcome,
        WizardStep::AuthMode,
        WizardStep::Url,
        WizardStep::Credentials,
        WizardStep::Testing,
        WizardStep::Site,
        WizardStep::Done,
    ] {
        screen.step = step;
        for (width, height) in [(40, 10), (20, 5), (4, 2), (1, 1)] {
            frame(&screen, width, height);
        }
    }

    // The auth list keeps every mode when it has to drop the descriptions.
    screen.step = WizardStep::AuthMode;
    let rows = frame(&screen, 80, 14).join("\n");
    assert!(rows.contains("Cloud"), "{rows}");
}

#[test]
fn hints_and_list_rows_take_clicks() {
    let mut screen = wizard();
    press(&mut screen, KeyCode::Enter);
    let rows = frame(&screen, 80, 24);

    let action = screen.handle_mouse_input(click_label(&rows, "Enter next", "back"));
    assert!(
        matches!(action, Some(Action::PressKey(key)) if key.code == KeyCode::Esc),
        "{action:?}"
    );

    screen.handle_mouse_input(click_label(&rows, "Hybrid", "Hybrid"));
    assert_eq!(screen.auth_mode_index, AuthMode::Hybrid.index());
}

/// Launch flags are answers filled in ahead of time: the steps show them
/// and test exactly what they show. A key from a flag or the environment
/// is not copied into the profile, and once the wizard is done the flags
/// have been asked for again, so they stop overriding.
#[test]
fn launch_flags_prefill_the_steps_and_then_stand_down() {
    use crate::config::resolve::ConnectionOverrides;

    let session = Session {
        profile: Some("default".into()),
        overrides: ConnectionOverrides {
            controller: Some("https://10.9.9.9".into()),
            site: Some("lab".into()),
            api_key: Some("key-from-the-environment".into()),
            insecure: Some(true),
            ..ConnectionOverrides::default()
        },
        ..Session::default()
    };
    let mut screen = OnboardingScreen::in_session("default", session);

    assert_eq!(screen.draft.url.text(), "https://10.9.9.9");
    assert_eq!(screen.draft.site.text(), "lab");
    assert!(screen.draft.insecure);
    assert!(screen.draft.api_key.is_empty());
    let rows = {
        press(&mut screen, KeyCode::Enter);
        press(&mut screen, KeyCode::Enter);
        frame(&screen, 80, 24).join("\n")
    };
    assert!(rows.contains("https://10.9.9.9"), "{rows}");
    assert!(rows.contains("[\u{2713}]"), "{rows}");

    press(&mut screen, KeyCode::Enter);
    type_text(&mut screen, "typed-key");
    press(&mut screen, KeyCode::Enter);
    finish(&mut screen, Ok(pass("lab", &["lab"])));
    saved(&mut screen, Ok(Vec::new()));
    let action = press(&mut screen, KeyCode::Enter);
    assert!(
        matches!(
            action,
            Some(Action::ApplyConnection {
                keep_flags: false,
                ..
            })
        ),
        "{action:?}"
    );
}

/// A repaired profile with a CA certificate can fix or clear it in the
/// wizard: a failure that points at the certificate must have a field to
/// point at.
#[test]
fn a_repair_can_fix_the_ca_certificate() {
    let profile: crate::config::Profile = toml::from_str(
        "controller = \"https://10.0.0.1\"\napi_key = \"k\"\nca_cert = \"/gone/ca.pem\"\n",
    )
    .expect("profile parses");
    let mut screen = OnboardingScreen::for_existing_profile(
        "work",
        &profile,
        &crate::config::Defaults::default(),
        &MemoryStore::default(),
    );
    press(&mut screen, KeyCode::Enter);
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.step, WizardStep::Url);
    assert_eq!(
        screen.fields(),
        [
            WizardField::Text(DraftField::Url),
            WizardField::Insecure,
            WizardField::Text(DraftField::CaCert)
        ]
    );

    press(&mut screen, KeyCode::Enter);
    press(&mut screen, KeyCode::Enter);
    finish(
        &mut screen,
        Err(failure(FailureKind::Tls, DraftField::CaCert)),
    );
    press(&mut screen, KeyCode::Esc);
    assert_eq!(screen.step, WizardStep::Url);
    assert_eq!(screen.active_text_field(), Some(DraftField::CaCert));
    ctrl(&mut screen, 'u');
    assert_eq!(screen.draft.to_profile().ca_cert, None);

    // A first-run wizard has no certificate to show.
    let mut fresh = wizard();
    press(&mut fresh, KeyCode::Enter);
    press(&mut fresh, KeyCode::Enter);
    assert_eq!(fresh.fields().len(), 2);
}

/// A wizard opened from a running session for a new profile can be left
/// from its first step; a first-run wizard has nowhere to go back to.
#[test]
fn only_a_wizard_opened_from_a_session_can_be_left() {
    let mut added = OnboardingScreen::for_new_profile("lab");
    assert!(matches!(
        press(&mut added, KeyCode::Esc),
        Some(Action::SetupCancelled)
    ));
    let rows = frame(&added, 80, 24).join("\n");
    assert!(rows.contains("\"lab\""), "{rows}");

    assert!(press(&mut wizard(), KeyCode::Esc).is_none());
}
