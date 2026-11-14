use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEventKind};
use unifly_api::SiteHint;

use super::{Message, SettingsField, SettingsScreen, SettingsState};
use crate::config::resolve::ConnectionOverrides;
use crate::tui::action::Action;
use crate::tui::component::Component;
use crate::tui::forms::controller_profile::{AuthMode, DraftField, save_to};
use crate::tui::forms::probe::{FailureKind, ProbeFailure, ProbePass, ProbeResult};
use crate::tui::forms::session::Session;
use crate::tui::table::render_lines;
use crate::tui::widgets::click::{click_label, left_click, mouse};

/// In-memory stand-in for the OS keyring.
#[derive(Default)]
struct MemoryStore(std::cell::RefCell<std::collections::HashMap<String, String>>);

impl crate::config::SecretStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>, crate::config::SecretStoreError> {
        Ok(self.0.borrow().get(key).cloned())
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

const TWO_PROFILES: &str = "default_profile = \"home\"\n\n\
     [profiles.home]\ncontroller = \"https://10.0.0.1\"\napi_key = \"home-key\"\n\n\
     [profiles.work]\ncontroller = \"https://10.0.9.1\"\nauth_mode = \"session\"\n\
     username = \"admin\"\npassword = \"work-password\"\ninsecure = true\n";

/// A screen showing `profile` from a config file holding `toml`. The
/// directory has to outlive the screen, which reads the file again when
/// it switches profiles.
fn screen_for(toml: &str, profile: &str) -> (SettingsScreen, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    std::fs::write(&path, toml).expect("write config");
    let cfg = crate::config::load_config_from(&path).expect("load config");
    let mut screen = SettingsScreen::blank();
    screen.load(&cfg, Some(profile));
    screen.config_path = Some(path);
    (screen, dir)
}

fn test_screen() -> SettingsScreen {
    let mut screen = SettingsScreen::blank();
    screen.draft.url.set("https://console.example.com");
    screen.draft.api_key.set("api-key");
    screen.draft.host_id.set("console-123");
    screen.draft.username.set("bliss");
    screen.draft.password.set("hunter2");
    screen.original = screen.draft.clone();
    screen
}

fn press(screen: &mut SettingsScreen, code: KeyCode) -> Option<Action> {
    screen.handle_key_input(KeyEvent::new(code, KeyModifiers::NONE))
}

fn ctrl(screen: &mut SettingsScreen, ch: char) {
    screen.handle_key_input(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL));
}

fn type_text(screen: &mut SettingsScreen, text: &str) {
    for ch in text.chars() {
        press(screen, KeyCode::Char(ch));
    }
}

fn frame(screen: &SettingsScreen, width: u16, height: u16) -> Vec<String> {
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

fn pass(site: &str) -> ProbePass {
    ProbePass {
        config: Box::new(unifly_api::ControllerConfig {
            site: site.into(),
            ..unifly_api::ControllerConfig::default()
        }),
        sites: sites(&["default", "lab"]),
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

fn finish(screen: &mut SettingsScreen, result: ProbeResult) -> Option<Action> {
    let generation = screen.probe.generation();
    screen.apply_action(&Action::SetupProbeFinished {
        generation,
        result: Box::new(result),
    })
}

fn saved(screen: &mut SettingsScreen, result: Result<Vec<String>, String>) -> Option<Action> {
    let generation = screen.probe.generation();
    screen.apply_action(&Action::SetupSaveFinished { generation, result })
}

/// The row a field is drawn on, with the focus marker when it has focus.
fn field_row(rows: &[String], label: &str) -> Option<String> {
    rows.iter().find(|row| row.contains(label)).cloned()
}

/// After `unifly tui -p work`, Settings edits `work`. A key typed there
/// lands in `work/api-key` and leaves the default profile's secret
/// alone.
#[test]
fn settings_edit_the_profile_the_session_uses() {
    use crate::config::SecretStore;

    let (mut screen, dir) = screen_for(
        "default_profile = \"home\"\n\n\
         [profiles.home]\ncontroller = \"https://10.0.0.1\"\n\n\
         [profiles.work]\ncontroller = \"https://10.0.9.1\"\n",
        "work",
    );
    let path = dir.path().join("config.toml");
    let store = MemoryStore::default();
    store
        .set("home/api-key", "home-secret")
        .expect("seed keyring");
    assert_eq!(screen.profile_name, "work");
    assert_eq!(screen.draft.url.text(), "https://10.0.9.1");

    screen.draft.api_key.set("work-secret");
    let plan = screen.draft.plan_save(&screen.profile_name);
    save_to(plan, &store, &path, &screen.profile_name).expect("save");

    assert_eq!(store.get("home/api-key"), Ok(Some("home-secret".into())));
    assert_eq!(store.get("work/api-key"), Ok(Some("work-secret".into())));
    let cfg = crate::config::load_config_from(&path).expect("reload");
    assert_eq!(cfg.profiles["home"].controller, "https://10.0.0.1");
}

/// A session profile the config does not hold yet is still the one
/// Settings saves to, and the one its title names.
#[test]
fn settings_name_a_session_profile_that_is_not_saved_yet() {
    let mut screen = SettingsScreen::blank();
    screen.load(&crate::config::Config::default(), Some("work"));

    assert_eq!(screen.profile_name, "work");
    assert_eq!(screen.profiles, ["work"]);
    let rows = frame(&screen, 120, 40).join("\n");
    assert!(rows.contains("Settings \u{00B7} work"), "{rows}");
}

#[test]
fn the_close_hint_presses_esc_so_touch_can_leave_settings() {
    let mut screen = test_screen();
    let rows = frame(&screen, 120, 40);
    let action = screen.handle_mouse_input(click_label(&rows, "test & save", "close"));

    assert!(
        matches!(action, Some(Action::PressKey(key)) if key.code == KeyCode::Esc),
        "{action:?}"
    );
}

#[test]
fn field_layout_follows_the_auth_mode() {
    let mut screen = test_screen();
    screen.draft.auth_mode = AuthMode::Session;
    let fields = screen.visible_fields();
    assert!(!fields.contains(&SettingsField::ApiKey));
    for field in [
        SettingsField::Username,
        SettingsField::Password,
        SettingsField::Totp,
        SettingsField::CaCert,
        SettingsField::Timeout,
    ] {
        assert!(fields.contains(&field), "{field:?}");
    }

    // Cloud goes through the connector: no address, no TLS settings.
    screen.draft.auth_mode = AuthMode::Cloud;
    let fields = screen.visible_fields();
    assert!(fields.contains(&SettingsField::ApiKey));
    assert!(fields.contains(&SettingsField::HostId));
    for field in [
        SettingsField::Url,
        SettingsField::Username,
        SettingsField::Insecure,
        SettingsField::CaCert,
    ] {
        assert!(!fields.contains(&field), "{field:?}");
    }
}

/// Every field of every auth mode can be reached and seen at 80x24, the
/// smallest terminal the dashboard supports, and at a size far below it.
#[test]
fn every_field_is_reachable_and_drawn_in_a_small_terminal() {
    for (width, height) in [(80, 24), (60, 12), (120, 34)] {
        for mode in AuthMode::ALL {
            let mut screen = test_screen();
            screen.draft.auth_mode = mode;
            screen.focus(SettingsField::Profile);
            let fields = screen.visible_fields();

            for expected in &fields {
                assert_eq!(screen.active_field, *expected, "{mode:?}");
                let rows = frame(&screen, width, height);
                let label = expected.label();
                let marked = format!("\u{25B8} {label}");
                assert!(
                    rows.iter().any(|row| row.contains(&marked)),
                    "{label} in {mode:?} at {width}x{height}:\n{}",
                    rows.join("\n")
                );
                assert!(
                    rows.iter().any(|row| row.contains("Esc close")),
                    "the hints stay on screen:\n{}",
                    rows.join("\n")
                );
                press(&mut screen, KeyCode::Tab);
            }
            assert_eq!(screen.active_field, SettingsField::Profile, "Tab wraps");
        }
    }

    // Far too small to be useful, but never a panic.
    let screen = test_screen();
    for (width, height) in [(40, 10), (20, 6), (5, 3), (1, 1)] {
        frame(&screen, width, height);
    }
}

/// The whole form fits at 80x24 in every auth mode while nothing is
/// being reported, so nothing has to scroll in the common case.
#[test]
fn the_form_fits_80x24_without_scrolling() {
    for mode in AuthMode::ALL {
        let mut screen = test_screen();
        screen.draft.auth_mode = mode;
        screen.focus(SettingsField::AuthMode);
        let rows = frame(&screen, 80, 24).join("\n");
        for field in screen.visible_fields() {
            assert!(
                rows.contains(field.label()),
                "{field:?} in {mode:?}:\n{rows}"
            );
        }
        assert!(!rows.contains('\u{25BC}'), "{mode:?} scrolls:\n{rows}");
    }
}

#[test]
fn the_wheel_scrolls_a_form_that_does_not_fit_and_a_key_brings_focus_back() {
    let mut screen = test_screen();
    screen.draft.auth_mode = AuthMode::Hybrid;
    screen.focus(SettingsField::Profile);
    let rows = frame(&screen, 80, 14);
    assert!(field_row(&rows, "Profile").is_some());
    assert!(field_row(&rows, "Sponsor button").is_none());
    assert!(rows.join("\n").contains('\u{25BC}'), "marks more below");

    for _ in 0..30 {
        screen.handle_mouse_input(mouse(MouseEventKind::ScrollDown, 40, 5));
    }
    let rows = frame(&screen, 80, 14);
    assert!(field_row(&rows, "Sponsor button").is_some(), "{rows:?}");
    assert!(rows.join("\n").contains('\u{25B2}'), "marks more above");
    assert_eq!(screen.active_field, SettingsField::Profile);

    // Any key that moves the focus scrolls it back into view.
    press(&mut screen, KeyCode::Down);
    let rows = frame(&screen, 80, 14);
    assert!(
        rows.iter()
            .any(|row| row.contains("\u{25B8} Controller URL")),
        "{rows:?}"
    );
}

/// A click only ever lands on a row the last frame drew, over the
/// columns it was drawn in.
#[test]
fn clicks_only_hit_the_rows_that_were_drawn() {
    let mut screen = test_screen();
    screen.draft.auth_mode = AuthMode::Hybrid;
    screen.focus(SettingsField::Profile);
    let rows = frame(&screen, 80, 14);
    let insecure = screen.draft.insecure();
    let donate = screen.show_donate;

    // The toggles are scrolled out of view: no cell of this frame flips
    // them, wherever the click lands.
    for row in 0..14 {
        for column in [0, 1, 40, 79] {
            screen.handle_mouse_input(left_click(column, row));
        }
    }
    assert_eq!(screen.draft.insecure(), insecure);
    assert_eq!(screen.show_donate, donate);
    assert!(screen.theme_selector.borrow().is_none());

    // A drawn row takes the click on its own columns only.
    let url = click_label(&rows, "Controller URL", "Controller URL");
    screen.handle_mouse_input(left_click(0, url.row));
    assert_ne!(screen.active_field, SettingsField::Url, "outside the panel");
    screen.handle_mouse_input(url);
    assert_eq!(screen.active_field, SettingsField::Url);
}

#[test]
fn a_click_toggles_a_checkbox_and_steps_a_picker_by_its_arrows() {
    let mut screen = test_screen();
    let rows = frame(&screen, 120, 40);

    screen.handle_mouse_input(click_label(&rows, "Skip TLS verify", "[ ]"));
    assert!(screen.draft.insecure());
    assert_eq!(screen.active_field, SettingsField::Insecure);

    let rows = frame(&screen, 120, 40);
    screen.handle_mouse_input(click_label(&rows, "Auth mode", "\u{25C2}"));
    assert_eq!(screen.draft.auth_mode, AuthMode::Cloud, "previous wraps");
    let rows = frame(&screen, 120, 40);
    let auth = rows
        .iter()
        .position(|row| row.contains("Auth mode"))
        .expect("auth row");
    let next = rows[auth]
        .chars()
        .collect::<Vec<_>>()
        .iter()
        .rposition(|ch| *ch == '\u{25B8}')
        .expect("next arrow");
    screen.handle_mouse_input(left_click(
        u16::try_from(next).expect("column"),
        u16::try_from(auth).expect("row"),
    ));
    assert_eq!(screen.draft.auth_mode, AuthMode::ApiKey);
}

/// Passing through Cloud while cycling the auth mode must leave the
/// address and the TLS choice as they were, on screen and on disk.
#[test]
fn cycling_through_cloud_leaves_the_profile_as_it_was() {
    let (mut screen, _dir) = screen_for(TWO_PROFILES, "work");
    let before = screen.draft.clone();
    assert!(screen.draft.insecure());
    screen.focus(SettingsField::AuthMode);

    for _ in 0..AuthMode::ALL.len() {
        press(&mut screen, KeyCode::Right);
    }

    assert_eq!(screen.draft, before);
    assert!(!screen.is_dirty());
    let rows = frame(&screen, 120, 40).join("\n");
    assert!(rows.contains("Skip TLS verify   [\u{2713}]"), "{rows}");
    assert_eq!(screen.draft.to_profile().insecure, Some(true));
}

#[test]
fn up_and_down_move_between_fields_and_never_change_one() {
    let mut screen = test_screen();
    screen.focus(SettingsField::Url);

    press(&mut screen, KeyCode::Down);
    assert_eq!(screen.active_field, SettingsField::AuthMode);
    press(&mut screen, KeyCode::Down);
    assert_eq!(screen.active_field, SettingsField::ApiKey);
    assert_eq!(
        screen.draft.auth_mode,
        AuthMode::ApiKey,
        "Down did not cycle"
    );
    press(&mut screen, KeyCode::Up);
    press(&mut screen, KeyCode::BackTab);
    assert_eq!(screen.active_field, SettingsField::Url);
}

#[test]
fn text_fields_edit_like_a_line_editor_and_take_a_paste() {
    let mut screen = test_screen();
    screen.focus(SettingsField::Url);

    press(&mut screen, KeyCode::Home);
    for _ in 0..8 {
        press(&mut screen, KeyCode::Delete);
    }
    type_text(&mut screen, "http://");
    assert_eq!(screen.draft.url.text(), "http://console.example.com");
    ctrl(&mut screen, 'x');
    ctrl(&mut screen, 'r');
    assert_eq!(screen.draft.url.text(), "http://console.example.com");
    press(&mut screen, KeyCode::End);
    ctrl(&mut screen, 'w');
    assert_eq!(screen.draft.url.text(), "", "Ctrl+W took the word");

    // A pasted key with a trailing newline goes in whole; the newline
    // must not start a test.
    screen.focus(SettingsField::ApiKey);
    screen.draft.api_key.clear();
    assert!(screen.captures_input());
    screen.handle_paste("pasted-key\r\n");
    assert_eq!(screen.draft.api_key.text(), "pasted-key");
    assert_eq!(screen.state, SettingsState::Editing);

    // A picker is not a text field: a paste there goes nowhere.
    screen.focus(SettingsField::AuthMode);
    let before = screen.draft.clone();
    screen.handle_paste("cloud");
    assert_eq!(screen.draft, before);
}

/// A value longer than its row keeps the end being typed and the cursor
/// on screen.
#[test]
fn a_long_value_keeps_its_cursor_in_view() {
    let mut screen = test_screen();
    screen.draft.auth_mode = AuthMode::Cloud;
    screen
        .draft
        .host_id
        .set("70A7419783ED0000000006797F060000000006C722C700000000624D3F9C:1234567");
    screen.focus(SettingsField::HostId);

    let rows = frame(&screen, 80, 24);
    let row = field_row(&rows, "\u{25B8} Host ID").expect("host id row");
    assert!(row.contains("1234567\u{2588}"), "{row}");
}

#[test]
fn secrets_are_dots_until_revealed_and_say_where_they_live() {
    let (mut screen, _dir) = screen_for(
        "[profiles.home]\ncontroller = \"https://10.0.0.1\"\nauth_mode = \"hybrid\"\n\
         username = \"admin\"\napi_key_env = \"UNIFLY_UNSET_KEY\"\n",
        "home",
    );
    screen.draft.note_keyring(false, true);
    let rows = frame(&screen, 120, 40);
    let key = field_row(&rows, "API key").expect("api key row");
    assert!(key.contains("$UNIFLY_UNSET_KEY"), "{key}");
    assert!(key.contains("not set"), "{key}");
    let password = field_row(&rows, "Password").expect("password row");
    assert!(password.contains("stored in keyring"), "{password}");

    screen.focus(SettingsField::ApiKey);
    type_text(&mut screen, "s3cret");
    let key = field_row(&frame(&screen, 120, 40), "API key").expect("row");
    assert!(!key.contains("s3cret"), "{key}");
    ctrl(&mut screen, 't');
    let key = field_row(&frame(&screen, 120, 40), "API key").expect("row");
    assert!(key.contains("s3cret"), "{key}");
    assert_eq!(screen.draft.api_key.text(), "s3cret");
}

#[test]
fn enter_with_a_problem_moves_focus_to_the_field_at_fault() {
    let mut screen = test_screen();
    screen.draft.auth_mode = AuthMode::Session;
    screen.draft.username.clear();
    screen.focus(SettingsField::Url);

    press(&mut screen, KeyCode::Enter);

    assert_eq!(screen.state, SettingsState::Editing);
    assert_eq!(screen.active_field, SettingsField::Username);
    assert_eq!(
        screen.message,
        Some(Message::Error("Username cannot be empty".into()))
    );

    // An address without a scheme is corrected and offered, not refused,
    // and so is one pasted from the browser with its page still on it.
    screen.draft.username.set("admin");
    for (typed, fixed) in [
        ("10.0.0.5:8443", "https://10.0.0.5:8443"),
        (
            "https://10.0.0.5/network/default/dashboard",
            "https://10.0.0.5",
        ),
    ] {
        screen.draft.url.set(typed);
        press(&mut screen, KeyCode::Enter);
        assert_eq!(screen.state, SettingsState::Editing, "{typed}");
        assert_eq!(screen.draft.url.text(), fixed);
        assert!(matches!(screen.message, Some(Message::Notice(_))));
    }
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.state, SettingsState::Testing);
}

/// A cancelled test stays cancelled: its result may not save, reconnect,
/// or put an error on the form the user went back to.
#[test]
fn a_cancelled_test_never_saves_or_applies() {
    let mut screen = test_screen();
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.state, SettingsState::Testing);
    let cancelled = screen.probe.generation();

    press(&mut screen, KeyCode::Esc);
    assert_eq!(screen.state, SettingsState::Editing);
    type_text(&mut screen, "/edited");

    for result in [
        Ok(pass("default")),
        Err(failure(FailureKind::Timeout, DraftField::Url)),
    ] {
        let follow_up = screen.apply_action(&Action::SetupProbeFinished {
            generation: cancelled,
            result: Box::new(result),
        });
        assert!(!matches!(follow_up, Some(Action::ApplyConnection { .. })));
        assert_eq!(screen.state, SettingsState::Editing);
        assert!(screen.tested.is_none());
        assert!(
            matches!(screen.message, None | Some(Message::Notice(_))),
            "{:?}",
            screen.message
        );
    }
    // Nor does a save result for it, should one ever turn up.
    assert!(!matches!(
        saved(&mut screen, Ok(Vec::new())),
        Some(Action::ApplyConnection { .. })
    ));
}

#[test]
fn only_the_latest_test_counts() {
    let mut screen = test_screen();
    press(&mut screen, KeyCode::Enter);
    let first = screen.probe.generation();
    press(&mut screen, KeyCode::Esc);
    press(&mut screen, KeyCode::Enter);
    assert_ne!(screen.probe.generation(), first);

    screen.apply_action(&Action::SetupProbeFinished {
        generation: first,
        result: Box::new(Ok(pass("stale"))),
    });
    assert_eq!(
        screen.state,
        SettingsState::Testing,
        "the stale pass is dropped"
    );

    finish(&mut screen, Ok(pass("default")));
    assert_eq!(screen.state, SettingsState::Saving);
}

/// What reconnects is the connection that was tested, carried through
/// the save, together with what the save reported.
#[test]
fn a_passed_test_saves_and_then_applies_what_was_tested() {
    let mut screen = test_screen();
    press(&mut screen, KeyCode::Enter);
    let follow_up = finish(&mut screen, Ok(pass("default")));
    assert!(matches!(follow_up, Some(Action::Invalidate)));
    assert_eq!(screen.state, SettingsState::Saving);
    assert!(
        press(&mut screen, KeyCode::Esc).is_none(),
        "a save runs out"
    );
    assert_eq!(screen.state, SettingsState::Saving);

    let applied = saved(
        &mut screen,
        Ok(vec!["saved the API key to config.toml in plaintext".into()]),
    );
    let Some(Action::ApplyConnection {
        profile_name,
        config,
        notices,
        flags,
    }) = applied
    else {
        panic!("a saved profile is applied: {applied:?}");
    };
    assert_eq!(profile_name, "default");
    assert_eq!(config.site, "default");
    assert_eq!(notices.len(), 1);
    assert_eq!(flags.site, None, "this launch gave no flags to keep");
}

#[test]
fn a_failed_save_is_shown_and_nothing_is_applied() {
    let mut screen = test_screen();
    press(&mut screen, KeyCode::Enter);
    finish(&mut screen, Ok(pass("default")));
    let follow_up = saved(
        &mut screen,
        Err("Connected, but failed to save config: read-only file system".into()),
    );

    assert!(!matches!(follow_up, Some(Action::ApplyConnection { .. })));
    assert_eq!(screen.state, SettingsState::Editing);
    let rows = frame(&screen, 80, 24).join("\n");
    assert!(rows.contains("read-only file system"), "{rows}");
}

/// A failure is explained in full: wrapped over as many rows as it
/// needs, with what to do next, and focus on the field it is about.
#[test]
fn a_failed_test_is_explained_in_full_under_the_form() {
    let mut screen = test_screen();
    press(&mut screen, KeyCode::Enter);
    let mut refused = failure(FailureKind::Tls, DraftField::Insecure);
    refused.summary =
        "The certificate https://console.example.com/ presented could not be verified.".into();
    refused.next_step = "UniFi consoles ship with a self-signed certificate. Turn on Skip TLS \
                         verification, or set a CA certificate that signed it."
        .into();
    refused.detail = Some("invalid peer certificate: UnknownIssuer".into());
    finish(&mut screen, Err(refused));

    assert_eq!(screen.state, SettingsState::Editing);
    assert_eq!(screen.active_field, SettingsField::Insecure);
    for (width, height) in [(120, 34), (80, 24)] {
        let rows = frame(&screen, width, height).join("\n");
        for part in ["presented", "self-signed", "UnknownIssuer"] {
            assert!(rows.contains(part), "{part} at {width}x{height}:\n{rows}");
        }
        assert!(
            rows.contains("\u{25B8} Skip TLS verify"),
            "the field at fault stays in view:\n{rows}"
        );
    }

    // Editing the form answers the failure, so it goes away.
    press(&mut screen, KeyCode::Char(' '));
    assert_eq!(screen.message, None);
}

#[test]
fn a_missing_site_offers_the_sites_the_controller_has() {
    let mut screen = test_screen();
    screen.draft.site.set("office");
    press(&mut screen, KeyCode::Enter);
    let mut missing = failure(FailureKind::Site, DraftField::Site);
    missing.sites = sites(&["default", "lab"]);
    finish(&mut screen, Err(missing));

    assert_eq!(screen.active_field, SettingsField::Site);
    assert!(screen.picks_site());
    let row = field_row(&frame(&screen, 120, 40), "\u{25B8} Site").expect("site row");
    assert!(row.contains("not on this controller"), "{row}");

    press(&mut screen, KeyCode::Right);
    assert_eq!(screen.draft.site.text(), "default");
    press(&mut screen, KeyCode::Right);
    assert_eq!(screen.draft.site.text(), "lab");
    let row = field_row(&frame(&screen, 120, 40), "\u{25B8} Site").expect("site row");
    assert!(row.contains("\u{25C2} lab \u{25B8} 2 of 2"), "{row}");

    // Typing takes over from the list, for a site it does not show.
    type_text(&mut screen, "lobby");
    assert!(!screen.picks_site());
    assert_eq!(screen.draft.site.text(), "lobby");
}

/// A failed test against another controller says nothing about its
/// sites, so the ones read from the previous controller stay withheld.
#[test]
fn a_failed_test_never_offers_another_controllers_sites() {
    let mut screen = test_screen();
    screen.set_known_sites(sites(&["default", "lab"]));
    screen.focus(SettingsField::Url);
    type_text(&mut screen, ":8443");

    press(&mut screen, KeyCode::Enter);
    finish(
        &mut screen,
        Err(failure(FailureKind::Unreachable, DraftField::Url)),
    );

    assert!(!screen.picks_site());
}

/// A key given at launch decides how the session signs in, whatever the
/// form's auth mode says, so the form says so where the mode is picked.
#[test]
fn a_launch_key_is_shown_on_the_auth_mode_it_overrides() {
    let (mut screen, _dir) = screen_for(TWO_PROFILES, "work");
    screen.session = Session {
        profile: Some("work".into()),
        overrides: ConnectionOverrides {
            api_key: Some("flag-key".into()),
            ..ConnectionOverrides::default()
        },
        ..Session::default()
    };
    screen.focus(SettingsField::AuthMode);

    let rows = frame(&screen, 120, 40);
    let mode = field_row(&rows, "Auth mode").expect("auth row");
    assert!(mode.contains("--api-key"), "{mode}");
    let all = rows.join("\n");
    assert!(all.contains("Launched with --api-key"), "{all}");
    assert!(!all.contains("flag-key"), "{all}");
}

/// The sites read from one controller are not offered once the form
/// points at another.
#[test]
fn known_sites_stop_being_offered_when_the_address_changes() {
    let mut screen = test_screen();
    screen.set_known_sites(sites(&["default", "lab"]));
    assert!(screen.picks_site());

    screen.focus(SettingsField::Url);
    type_text(&mut screen, ":8443");
    assert!(!screen.picks_site());
    screen.focus(SettingsField::Site);
    assert!(screen.captures_input(), "the site is typed again");
}

#[test]
fn the_spinner_asks_for_a_frame_on_every_tick_while_it_runs() {
    let mut screen = test_screen();
    assert!(screen.apply_action(&Action::Tick).is_none());

    press(&mut screen, KeyCode::Enter);
    assert!(matches!(
        screen.apply_action(&Action::Tick),
        Some(Action::Invalidate)
    ));
    let rows = frame(&screen, 80, 24).join("\n");
    assert!(rows.contains("Testing connection"), "{rows}");
    assert!(rows.contains("Esc cancel"), "{rows}");
}

/// Esc on a form with edits hands the question to the app's confirm
/// dialog, which answers only to y, n and Esc: Enter, the key that tests
/// and saves, can never drop the edits. On a form with none it just
/// closes.
#[test]
fn esc_asks_before_dropping_edits() {
    let mut screen = test_screen();
    assert!(matches!(
        press(&mut screen, KeyCode::Esc),
        Some(Action::CloseSettings)
    ));

    screen.focus(SettingsField::Url);
    type_text(&mut screen, "/edited");
    let asked = press(&mut screen, KeyCode::Esc);
    let Some(Action::ShowConfirm(confirm)) = asked else {
        panic!("edits are asked about: {asked:?}");
    };
    assert_eq!(confirm.prompt, "Close Settings?");
    assert!(
        confirm
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("dropped")),
        "{confirm:?}"
    );
    assert!(matches!(*confirm.action, Action::CloseSettings));

    // The form itself is unchanged and still editing: nothing closes
    // until the dialog's yes arrives.
    assert_eq!(screen.state, SettingsState::Editing);
    assert_eq!(
        screen.draft.url.text(),
        "https://console.example.com/edited",
        "the edits are kept"
    );
}

/// Launch flags keep overriding the profile after Settings reconnects,
/// and the form says which fields they override.
#[test]
fn launch_flags_survive_and_are_shown_on_the_fields_they_override() {
    let (mut screen, _dir) = screen_for(TWO_PROFILES, "home");
    screen.session = Session {
        profile: Some("home".into()),
        overrides: ConnectionOverrides {
            site: Some("lab".into()),
            insecure: Some(true),
            ..ConnectionOverrides::default()
        },
        ..Session::default()
    };

    assert_eq!(screen.overrides().site.as_deref(), Some("lab"));
    let rows = frame(&screen, 120, 40);
    let site = field_row(&rows, "Site").expect("site row");
    assert!(site.contains("--site lab"), "{site}");
    let tls = field_row(&rows, "Skip TLS verify").expect("tls row");
    assert!(tls.contains("--insecure=true"), "{tls}");
    screen.focus(SettingsField::Site);
    let rows = frame(&screen, 120, 40).join("\n");
    assert!(rows.contains("Launched with --site lab"), "{rows}");

    // They were given for the launch's profile; another one connects
    // exactly as it is saved.
    screen.focus(SettingsField::Profile);
    press(&mut screen, KeyCode::Right);
    assert_eq!(screen.profile_name, "work");
    assert_eq!(screen.overrides().site, None);
    let site = field_row(&frame(&screen, 120, 40), "Site").expect("site row");
    assert!(!site.contains("--site"), "{site}");

    // Back on the launch's profile, a save under them keeps them for
    // the session.
    press(&mut screen, KeyCode::Left);
    assert_eq!(screen.profile_name, "home");
    press(&mut screen, KeyCode::Enter);
    finish(&mut screen, Ok(pass("lab")));
    let applied = saved(&mut screen, Ok(Vec::new()));
    let Some(Action::ApplyConnection { flags, .. }) = applied else {
        panic!("applied: {applied:?}");
    };
    assert_eq!(flags.site.as_deref(), Some("lab"));
    assert_eq!(flags.insecure, Some(true));
}

/// A value saved as tested must have been tested. A launch flag that
/// would keep the test from using what the form now holds stands down:
/// the test connects with what is typed, the form stops claiming the
/// override, and the session goes on without that flag.
#[test]
fn a_launch_flag_stands_down_when_the_form_changes_what_it_covers() {
    let (mut screen, _dir) = screen_for(TWO_PROFILES, "work");
    screen.session = Session {
        profile: Some("work".into()),
        overrides: ConnectionOverrides {
            controller: Some("https://10.7.7.7".into()),
            site: Some("lab".into()),
            api_key: Some("flag-key".into()),
            timeout: Some(7),
            no_cache: true,
            ..ConnectionOverrides::default()
        },
        ..Session::default()
    };
    let effective = |screen: &SettingsScreen| screen.overrides().apply(&screen.draft.to_profile());

    // Untouched, the launch decides: its key signs in, with no password.
    assert_eq!(effective(&screen).auth_mode, "integration");
    assert_eq!(effective(&screen).controller, "https://10.7.7.7");

    // A retyped password is only tested by a sign-in, so the key that
    // would skip the sign-in is set aside. The other flags still stand.
    screen.focus(SettingsField::Password);
    ctrl(&mut screen, 'u');
    type_text(&mut screen, "retyped");
    let tested = effective(&screen);
    assert_eq!(tested.auth_mode, "session");
    assert_eq!(tested.password.as_deref(), Some("retyped"));
    assert_eq!(tested.api_key, None);
    assert_eq!(tested.controller, "https://10.7.7.7");
    assert_eq!(tested.site, "lab");
    screen.focus(SettingsField::AuthMode);
    let rows = frame(&screen, 120, 40);
    let mode = field_row(&rows, "Auth mode").expect("auth row");
    assert!(!mode.contains("--api-key"), "{mode}");
    let all = rows.join("\n");
    assert!(all.contains("Launched with --api-key, but"), "{all}");

    // So is the address flag once the address is edited, and the site
    // flag once another site is typed.
    screen.focus(SettingsField::Url);
    type_text(&mut screen, ":8443");
    screen.focus(SettingsField::Site);
    type_text(&mut screen, "x");
    let tested = effective(&screen);
    assert_eq!(tested.controller, "https://10.0.9.1:8443");
    assert_eq!(tested.site, "defaultx");
    let url = field_row(&frame(&screen, 120, 40), "Controller URL").expect("url row");
    assert!(!url.contains("--controller"), "{url}");

    // The session keeps only the flags the test ran under.
    press(&mut screen, KeyCode::Enter);
    finish(&mut screen, Ok(pass("defaultx")));
    let applied = saved(&mut screen, Ok(Vec::new()));
    let Some(Action::ApplyConnection { flags, .. }) = applied else {
        panic!("applied: {applied:?}");
    };
    assert_eq!(flags.controller, None);
    assert_eq!(flags.site, None);
    assert!(flags.api_key.is_none());
    assert_eq!(flags.timeout, Some(7), "nothing typed replaced it");
    assert!(flags.no_cache, "a flag about the session, not the profile");
}

/// A launch key that leaves the sign-in in place still stands when the
/// password changes: the hybrid test signs in with it either way.
#[test]
fn a_launch_key_stands_while_the_changed_value_is_still_tested() {
    let (mut screen, _dir) = screen_for(TWO_PROFILES, "work");
    screen.draft.auth_mode = AuthMode::Hybrid;
    screen.original = screen.draft.clone();
    screen.session = Session {
        profile: Some("work".into()),
        overrides: ConnectionOverrides {
            api_key: Some("flag-key".into()),
            ..ConnectionOverrides::default()
        },
        ..Session::default()
    };

    screen.focus(SettingsField::Password);
    type_text(&mut screen, "2");
    let tested = screen.overrides().apply(&screen.draft.to_profile());
    assert_eq!(tested.auth_mode, "hybrid");
    assert_eq!(tested.api_key.as_deref(), Some("flag-key"));
    assert_eq!(tested.password.as_deref(), Some("work-password2"));

    // Typing a key is another matter: the flag would test its own.
    screen.focus(SettingsField::ApiKey);
    type_text(&mut screen, "typed-key");
    let tested = screen.overrides().apply(&screen.draft.to_profile());
    assert_eq!(tested.api_key.as_deref(), Some("typed-key"));
}

#[test]
fn the_profile_picker_shows_another_profile_and_asks_before_dropping_edits() {
    let (mut screen, _dir) = screen_for(TWO_PROFILES, "home");
    screen.focus(SettingsField::Profile);
    let rows = frame(&screen, 120, 40).join("\n");
    assert!(rows.contains("Settings \u{00B7} home"), "{rows}");
    assert!(rows.contains("\u{25C2} home \u{25B8} 1 of 2"), "{rows}");

    press(&mut screen, KeyCode::Right);
    assert_eq!(screen.profile_name, "work");
    assert_eq!(screen.draft.auth_mode, AuthMode::Session);
    assert_eq!(screen.draft.url.text(), "https://10.0.9.1");
    assert!(!screen.is_dirty());
    let rows = frame(&screen, 120, 40).join("\n");
    assert!(rows.contains("Settings \u{00B7} work"), "{rows}");

    // With edits pending, switching asks first.
    screen.focus(SettingsField::Username);
    type_text(&mut screen, "2");
    screen.focus(SettingsField::Profile);
    let asked = press(&mut screen, KeyCode::Left);
    let Some(Action::ShowConfirm(confirm)) = asked else {
        panic!("edits are asked about: {asked:?}");
    };
    assert_eq!(confirm.prompt, "Switch profiles?");
    assert_eq!(screen.profile_name, "work");
    assert_eq!(
        screen.draft.username.text(),
        "admin2",
        "nothing dropped yet"
    );

    // The dialog's yes comes back as the action it was given.
    screen.apply_action(&confirm.action);
    assert_eq!(screen.profile_name, "home");
    assert_eq!(screen.draft.api_key.text(), "home-key");
    assert!(!screen.is_dirty());
}

#[test]
fn a_new_profile_is_named_on_the_profile_row_and_opens_the_wizard() {
    let (mut screen, _dir) = screen_for(TWO_PROFILES, "home");
    screen.focus(SettingsField::Profile);

    press(&mut screen, KeyCode::Char('n'));
    assert_eq!(screen.state, SettingsState::Naming);
    assert!(screen.captures_input());
    let rows = frame(&screen, 120, 40).join("\n");
    assert!(rows.contains("New profile"), "{rows}");
    assert!(rows.contains("Enter set it up"), "{rows}");

    type_text(&mut screen, "work");
    assert!(press(&mut screen, KeyCode::Enter).is_none());
    assert!(
        matches!(&screen.message, Some(Message::Error(text)) if text.contains("already")),
        "{:?}",
        screen.message
    );

    ctrl(&mut screen, 'u');
    type_text(&mut screen, "lab 2");
    assert_eq!(screen.new_profile.text(), "lab2", "no spaces in a name");
    let action = press(&mut screen, KeyCode::Enter);
    assert!(
        matches!(&action, Some(Action::SetupNewProfile(name)) if name == "lab2"),
        "{action:?}"
    );

    // Esc leaves the naming, not Settings.
    press(&mut screen, KeyCode::Char('n'));
    assert!(press(&mut screen, KeyCode::Esc).is_none());
    assert_eq!(screen.state, SettingsState::Editing);
}

/// While the session redacts what it shows, Settings shows no address,
/// user, site, or profile name, and reveals nothing.
#[test]
fn a_redacting_session_sees_no_values() {
    let (mut screen, _dir) = screen_for(TWO_PROFILES, "work");
    screen.set_known_sites(sites(&["default", "hq-site"]));
    screen.draft.site.set("hq-site");
    screen.original = screen.draft.clone();
    screen.hide_values();
    screen.focus(SettingsField::Password);
    ctrl(&mut screen, 't');

    let rows = frame(&screen, 120, 40).join("\n");
    let hidden = ["10.0.9.1", "admin", "work-password", "work", "hq-site"];
    for secret in hidden {
        assert!(!rows.contains(secret), "{secret} is on screen:\n{rows}");
    }

    // Nor does a failed test or a launch flag bring them back.
    screen.session = Session {
        profile: Some("work".into()),
        overrides: ConnectionOverrides {
            site: Some("hq-site".into()),
            ..ConnectionOverrides::default()
        },
        ..Session::default()
    };
    press(&mut screen, KeyCode::Enter);
    let mut refused = failure(FailureKind::Unreachable, DraftField::Site);
    refused.summary = "Nothing is accepting connections at https://10.0.9.1/.".into();
    refused.detail = Some("tcp connect error for admin@10.0.9.1".into());
    finish(&mut screen, Err(refused));
    let rows = frame(&screen, 120, 40).join("\n");
    assert!(rows.contains("could not be reached"), "{rows}");
    assert!(rows.contains("--site"), "{rows}");
    for secret in hidden {
        assert!(!rows.contains(secret), "{secret} is on screen:\n{rows}");
    }
    assert!(rows.contains("Settings"), "{rows}");
    assert!(rows.contains("profile 2"), "{rows}");
}

/// The form asks before dropping edits only when a save would change the
/// profile, and a save changes only what was edited: keys the form does
/// not show for the current mode come back exactly as they were.
#[test]
fn an_untouched_form_is_clean_and_saves_the_profile_as_it_was() {
    let toml = "[profiles.home]\ncontroller = \"https://10.0.0.1\"\nauth_mode = \"integration\"\n\
         api_key = \"config-key\"\napi_key_env = \"UNIFLY_TEST_UNSET_KEY\"\n\
         host_id = \"console-1\"\nhost_id_env = \"UNIFLY_TEST_UNSET_HOST\"\n\
         username = \"admin\"\npassword = \"config-password\"\n";
    let (mut screen, dir) = screen_for(toml, "home");
    let before = crate::config::load_config_from(&dir.path().join("config.toml"))
        .expect("load")
        .profiles["home"]
        .clone();

    assert!(!screen.is_dirty());
    assert_eq!(
        toml::to_string(&screen.draft.to_profile()).expect("serializes"),
        toml::to_string(&before).expect("serializes"),
    );

    // Leaving the mode is an edit, and still drops nothing.
    screen.draft.auth_mode = AuthMode::Session;
    assert!(screen.is_dirty());
    let saved = screen.draft.to_profile();
    assert_eq!(saved.api_key.as_deref(), Some("config-key"));
    assert_eq!(saved.api_key_env.as_deref(), Some("UNIFLY_TEST_UNSET_KEY"));
    assert_eq!(saved.host_id_env.as_deref(), Some("UNIFLY_TEST_UNSET_HOST"));
}

/// The checkbox is not an exception for one certificate. Its row and its
/// help say what ticking it does, and a profile that already skips
/// verification says so wherever the form is open, focused or not.
#[test]
fn a_profile_that_skips_tls_verification_says_so_in_the_form() {
    let (mut screen, _dir) = screen_for(TWO_PROFILES, "home");
    let tls = field_row(&frame(&screen, 120, 40), "Skip TLS verify").expect("tls row");
    assert!(tls.contains("[ ] turns certificate checks off"), "{tls}");
    assert!(!tls.contains("self-signed"), "{tls}");

    // `work` is saved with insecure = true.
    screen.focus(SettingsField::Profile);
    press(&mut screen, KeyCode::Right);
    let rows = frame(&screen, 120, 40);
    let tls = field_row(&rows, "Skip TLS verify").expect("tls row");
    assert!(tls.contains("off: any certificate is accepted"), "{tls}");

    screen.focus(SettingsField::Insecure);
    let all = frame(&screen, 120, 40).join("\n");
    assert!(
        all.contains("TLS verification is off for this profile"),
        "{all}"
    );
}

/// A one-time code is good for the test it was typed for. Once that test
/// reports, the field is empty, so a retest cannot send the spent code.
#[test]
fn a_one_time_code_is_cleared_once_its_test_has_run() {
    let (mut screen, _dir) = screen_for(TWO_PROFILES, "work");
    screen.focus(SettingsField::Totp);
    type_text(&mut screen, "123456");
    assert!(!screen.is_dirty(), "a code is never saved");

    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.state, SettingsState::Testing);
    assert_eq!(screen.draft.totp.text(), "123456", "still under test");
    finish(
        &mut screen,
        Err(failure(FailureKind::Credentials, DraftField::Password)),
    );
    assert!(screen.draft.totp.is_empty());

    // A cancelled test reported nothing, so its code is left to retry.
    screen.focus(SettingsField::Totp);
    type_text(&mut screen, "654321");
    press(&mut screen, KeyCode::Enter);
    press(&mut screen, KeyCode::Esc);
    assert_eq!(screen.draft.totp.text(), "654321");
}

/// The command that makes MFA codes may carry the seed in its arguments,
/// so the form names the program and nothing after it.
#[test]
fn the_profile_says_a_command_supplies_its_mfa_codes_without_showing_it() {
    let (screen, _dir) = screen_for(
        "[profiles.home]\ncontroller = \"https://10.0.0.1\"\nauth_mode = \"session\"\n\
         username = \"admin\"\n\
         totp_command = [\"/usr/local/bin/oathtool\", \"--totp\", \"-b\", \"JBSWY3DPEHPK3PXP\"]\n",
        "home",
    );

    let rows = frame(&screen, 120, 40);
    let code = field_row(&rows, "One-time code").expect("code row");
    assert!(code.contains("only for two-factor sign-in"), "{code}");
    let source = field_row(&rows, "MFA codes").expect("mfa row");
    assert!(source.contains("from a command (oathtool)"), "{source}");
    let all = rows.join("\n");
    for hidden in ["JBSWY3DPEHPK3PXP", "--totp", "/usr/local/bin"] {
        assert!(!all.contains(hidden), "{hidden} is on screen:\n{all}");
    }
}

#[test]
fn the_certificate_and_timeout_are_edited_in_the_form() {
    let (mut screen, _dir) = screen_for(TWO_PROFILES, "home");
    let rows = frame(&screen, 120, 40);
    let timeout = field_row(&rows, "Timeout").expect("timeout row");
    assert!(timeout.contains("30 seconds (default)"), "{timeout}");

    screen.focus(SettingsField::Timeout);
    type_text(&mut screen, "4x5");
    assert_eq!(screen.draft.timeout.text(), "45", "digits only");
    assert_eq!(screen.draft.to_profile().timeout, Some(45));
    assert!(screen.is_dirty());

    screen.focus(SettingsField::CaCert);
    type_text(&mut screen, "/nonexistent/ca.pem");
    press(&mut screen, KeyCode::Enter);
    assert_eq!(screen.state, SettingsState::Editing);
    assert_eq!(screen.active_field, SettingsField::CaCert);
    assert!(
        matches!(&screen.message, Some(Message::Error(text)) if text.contains("No certificate")),
        "{:?}",
        screen.message
    );
}

/// A preference that cannot be written is reported, and the screen does
/// not pretend it was saved.
#[test]
fn a_preference_that_cannot_be_saved_says_so() {
    let (mut screen, dir) = screen_for(TWO_PROFILES, "home");
    std::fs::write(
        dir.path().join("config.toml"),
        "[defaults]\ntimeout = \"thirty\"\n",
    )
    .expect("break config");
    screen.focus(SettingsField::ShowDonate);
    let shown = screen.show_donate;

    press(&mut screen, KeyCode::Char(' '));

    assert_eq!(screen.show_donate, shown);
    assert!(
        matches!(&screen.message, Some(Message::Error(text)) if text.contains("sponsor")),
        "{:?}",
        screen.message
    );
}

#[test]
fn the_sponsor_toggle_writes_only_its_own_setting() {
    let (mut screen, dir) = screen_for(TWO_PROFILES, "home");
    screen.focus(SettingsField::ShowDonate);

    press(&mut screen, KeyCode::Char(' '));

    assert!(!screen.show_donate);
    assert_eq!(screen.message, None);
    let cfg = crate::config::load_config_from(&dir.path().join("config.toml")).expect("reload");
    assert!(!cfg.defaults.show_donate);
    assert_eq!(cfg.profiles["work"].username.as_deref(), Some("admin"));
}
