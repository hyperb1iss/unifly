use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::json;

use unifly_api::{Controller, DemoFleetOptions, Network};

use super::reference_summary;
use crate::tui::action::Action;
use crate::tui::app::App;
use crate::tui::screen::ScreenId;
use crate::tui::table::render_lines;

/// An App on the Networks screen of a connected demo fleet, holding the
/// fleet's networks as the data bridge would deliver them.
async fn networks_app() -> (App, Controller) {
    let demo = Controller::demo(DemoFleetOptions::default());
    demo.connect().await.expect("demo fleet connects");
    let mut app = App::new(Some(demo.clone()), None, false, true);
    app.active_screen = ScreenId::Networks;
    sync(&mut app, &demo);
    (app, demo)
}

/// Hand the screens the store's networks, as the data bridge does.
fn sync(app: &mut App, demo: &Controller) {
    app.process_action(&Action::NetworksUpdated(demo.networks_snapshot()))
        .expect("networks");
}

fn press(app: &mut App, code: KeyCode) {
    let key = KeyEvent::new(code, KeyModifiers::NONE);
    if let Some(action) = app.handle_key_event(key).expect("key") {
        app.process_action(&action).expect("action");
    }
}

fn type_text(app: &mut App, text: &str) {
    for ch in text.chars() {
        press(app, KeyCode::Char(ch));
    }
}

/// Process queued actions until one matches `done`, which is processed
/// too.
async fn run_until(app: &mut App, done: impl Fn(&Action) -> bool) -> Action {
    loop {
        let action = tokio::time::timeout(Duration::from_secs(10), app.action_rx.recv())
            .await
            .expect("the app reports back")
            .expect("the action channel stays open");
        app.process_action(&action).expect("action");
        if done(&action) {
            return action;
        }
    }
}

fn screen(app: &mut App) -> String {
    render_lines(100, 30, |frame, _| app.render(frame)).join("\n")
}

fn network<'a>(networks: &'a [Arc<Network>], name: &str) -> &'a Arc<Network> {
    networks
        .iter()
        .find(|network| network.name == name)
        .unwrap_or_else(|| panic!("the fleet has a {name} network"))
}

/// Open the edit form on the network called `name`.
fn edit(app: &mut App, demo: &Controller, name: &str) {
    let ordered = crate::cli::commands::util::order::networks(&demo.networks_snapshot());
    let row = ordered
        .iter()
        .position(|network| network.name == name)
        .unwrap_or_else(|| panic!("{name} is listed"));
    press(app, KeyCode::Char('g'));
    for _ in 0..row {
        press(app, KeyCode::Char('j'));
    }
    press(app, KeyCode::Char('e'));
}

#[tokio::test]
async fn a_refused_save_keeps_the_form_and_a_good_one_closes_it_on_fresh_data() {
    let (mut app, demo) = networks_app().await;
    let render_vlan = network(&demo.networks_snapshot(), "Render")
        .vlan_id
        .expect("a VLAN");
    edit(&mut app, &demo, "Studio");

    // Render's VLAN is taken, which only the controller can know.
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Backspace);
    press(&mut app, KeyCode::Backspace);
    type_text(&mut app, &render_vlan.to_string());
    press(&mut app, KeyCode::Enter);
    assert!(app.pending_confirm.is_some(), "a VLAN change asks first");
    press(&mut app, KeyCode::Char('y'));
    run_until(&mut app, |action| matches!(action, Action::NetworkSave(..))).await;
    assert!(
        screen(&mut app).contains("Saving\u{2026}"),
        "the form says it is saving while the controller has the draft"
    );
    run_until(&mut app, |action| {
        matches!(action, Action::NetworkEditResult(_))
    })
    .await;

    let text = screen(&mut app);
    assert!(
        text.contains("Edit network: Studio"),
        "the form is open:\n{text}"
    );
    assert!(text.contains("used by Render"), "{text}");
    assert!(
        app.notices.is_empty(),
        "the failure shows in the form, not a toast"
    );

    // Fix the draft: a free VLAN and a new name.
    press(&mut app, KeyCode::Backspace);
    press(&mut app, KeyCode::Backspace);
    type_text(&mut app, "77");
    press(&mut app, KeyCode::BackTab);
    type_text(&mut app, " B");
    press(&mut app, KeyCode::Enter);
    assert!(app.pending_confirm.is_some(), "a VLAN change asks first");
    press(&mut app, KeyCode::Char('y'));
    run_until(&mut app, |action| matches!(action, Action::Notify(_))).await;

    let saved = demo.networks_snapshot();
    assert_eq!(network(&saved, "Studio B").vlan_id, Some(77));
    sync(&mut app, &demo);
    let text = screen(&mut app);
    assert!(!text.contains("Edit network"), "the form closed:\n{text}");
    assert!(text.contains("Saved Studio B"), "{text}");
    assert!(
        text.contains("Studio B"),
        "the table shows the new name:\n{text}"
    );
}

#[tokio::test]
async fn a_save_without_a_connection_is_refused_in_the_form() {
    let (mut app, demo) = networks_app().await;
    edit(&mut app, &demo, "Studio");
    type_text(&mut app, " B");
    demo.disconnect().await;

    press(&mut app, KeyCode::Enter);
    run_until(&mut app, |action| {
        matches!(action, Action::NetworkEditResult(_))
    })
    .await;

    let text = screen(&mut app);
    assert!(
        text.contains("not connected, so nothing was sent"),
        "{text}"
    );
    assert!(text.contains("Studio B"), "the draft is intact:\n{text}");
}

#[tokio::test]
async fn a_created_network_shows_up_and_deleting_it_names_what_uses_it() {
    let (mut app, demo) = networks_app().await;
    app.process_action(&Action::AccessUpdated(crate::tui::action::Access {
        integration: true,
        session: true,
        live_events: false,
    }))
    .expect("access");

    press(&mut app, KeyCode::Char('c'));
    type_text(&mut app, "Stage");
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "60");
    press(&mut app, KeyCode::Enter);
    run_until(&mut app, |action| matches!(action, Action::Notify(_))).await;
    let created = network(&demo.networks_snapshot(), "Stage").id.clone();
    sync(&mut app, &demo);
    assert!(screen(&mut app).contains("Created network Stage"));

    // Nothing uses the new network; Studio carries clients.
    let studio = network(&demo.networks_snapshot(), "Studio").id.clone();
    for (id, expected) in [(studio, "clients"), (created.clone(), "Nothing uses it")] {
        app.process_action(&Action::RequestDeleteNetwork(id))
            .expect("delete request");
        let Action::ShowConfirm(confirm) =
            run_until(&mut app, |action| matches!(action, Action::ShowConfirm(_))).await
        else {
            unreachable!("run_until returns the match");
        };
        assert!(
            confirm.prompt.starts_with("Delete network "),
            "{}",
            confirm.prompt
        );
        let detail = confirm.detail.unwrap_or_default();
        assert!(detail.contains(expected), "{detail}");
        assert!(detail.contains("cannot be undone"), "{detail}");
        assert!(
            !app.notices
                .messages()
                .iter()
                .any(|message| message.starts_with("Checking")),
            "the checking toast is gone"
        );
        press(&mut app, KeyCode::Char('n'));
    }

    app.process_action(&Action::RequestDeleteNetwork(created.clone()))
        .expect("delete request");
    run_until(&mut app, |action| matches!(action, Action::ShowConfirm(_))).await;
    press(&mut app, KeyCode::Char('y'));
    run_until(&mut app, |action| {
        matches!(action, Action::OpFinished { .. })
    })
    .await;
    assert!(
        demo.networks_snapshot()
            .iter()
            .all(|network| network.id != created),
        "the network is gone from the controller"
    );
}

#[test]
fn references_read_as_counts_per_kind() {
    let references = json!({ "referenceResources": [
        { "resourceType": "CLIENT", "referenceCount": 12 },
        { "resourceType": "WIFI", "referenceCount": 1, "references": [] },
        { "resourceType": "NEXT_AI", "referenceCount": 2 },
    ]});
    assert_eq!(
        reference_summary(&references),
        "In use by 12 clients, 1 WiFi network, 2 other settings. They lose this network. \
         This cannot be undone."
    );
    assert_eq!(
        reference_summary(&json!({ "referenceResources": [] })),
        "Nothing uses it. This cannot be undone."
    );
}
