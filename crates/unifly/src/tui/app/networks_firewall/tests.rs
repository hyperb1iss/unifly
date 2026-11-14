use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::json;

use unifly_api::{Command, Controller, DemoFleetOptions, Network};

use super::{reference_summary, state_accepts_writes};
use crate::tui::action::{Action, ConfirmRequest};
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

/// Put the cursor on the network called `name`.
fn select(app: &mut App, demo: &Controller, name: &str) {
    let ordered = crate::cli::commands::util::order::networks(&demo.networks_snapshot());
    let row = ordered
        .iter()
        .position(|network| network.name == name)
        .unwrap_or_else(|| panic!("{name} is listed"));
    press(app, KeyCode::Char('g'));
    for _ in 0..row {
        press(app, KeyCode::Char('j'));
    }
}

/// Open the edit form on the network called `name`.
fn edit(app: &mut App, demo: &Controller, name: &str) {
    select(app, demo, name);
    press(app, KeyCode::Char('e'));
}

/// Press `D` on the network called `name` and return the question the
/// reference check ends in.
async fn ask_to_delete(app: &mut App, demo: &Controller, name: &str) -> ConfirmRequest {
    select(app, demo, name);
    press(app, KeyCode::Char('D'));
    match run_until(app, |action| matches!(action, Action::ShowConfirm(_))).await {
        Action::ShowConfirm(confirm) => confirm,
        _ => unreachable!("run_until returns the match"),
    }
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

    // Nothing uses the new network; Studio carries clients. The delete
    // is forced only where the question listed what it takes with it.
    for (name, expected, forced) in [
        ("Studio", "clients", true),
        ("Stage", "Nothing uses it", false),
    ] {
        let confirm = ask_to_delete(&mut app, &demo, name).await;
        assert_eq!(confirm.prompt, format!("Delete network {name}?"));
        let detail = confirm.detail.unwrap_or_default();
        assert!(detail.contains(expected), "{detail}");
        assert!(detail.contains("cannot be undone"), "{detail}");
        let Action::Run { command, .. } = *confirm.action else {
            panic!("yes runs the delete");
        };
        assert!(
            matches!(*command, Command::DeleteNetwork { force, .. } if force == forced),
            "{name}: {command:?}"
        );
        assert!(
            !app.notices
                .messages()
                .iter()
                .any(|message| message.starts_with("Checking")),
            "the checking toast is gone"
        );
        press(&mut app, KeyCode::Char('n'));
    }

    ask_to_delete(&mut app, &demo, "Stage").await;
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

/// The reference check takes a moment. What the user does in that moment
/// must not be answered by a delete question they have not read.
#[tokio::test]
async fn a_delete_check_that_comes_back_to_something_else_does_not_ask() {
    let (mut app, demo) = networks_app().await;
    let checked = "Checked what uses Studio. Press D on it again to delete it.";
    let after_the_check = |app: &App| {
        (
            app.pending_confirm.is_some(),
            app.notices.latest().map(|toast| toast.message.clone()),
        )
    };

    // Typing a new network's name while the check is out: the `y` in
    // "Lobby" used to run the delete.
    select(&mut app, &demo, "Studio");
    press(&mut app, KeyCode::Char('D'));
    press(&mut app, KeyCode::Char('c'));
    type_text(&mut app, "Lobby");
    run_until(&mut app, |action| matches!(action, Action::Notify(_))).await;
    assert_eq!(after_the_check(&app), (false, Some(checked.to_owned())));
    let text = screen(&mut app);
    assert!(
        text.contains("Lobby"),
        "every key reached the form:\n{text}"
    );
    press(&mut app, KeyCode::Esc);

    // The cursor moved to another network.
    select(&mut app, &demo, "Studio");
    press(&mut app, KeyCode::Char('D'));
    select(&mut app, &demo, "Render");
    run_until(&mut app, |action| matches!(action, Action::Notify(_))).await;
    assert_eq!(after_the_check(&app), (false, Some(checked.to_owned())));

    // Another screen, and the help overlay, are in front.
    for key in [KeyCode::Tab, KeyCode::Char('?')] {
        app.active_screen = ScreenId::Networks;
        select(&mut app, &demo, "Studio");
        press(&mut app, KeyCode::Char('D'));
        press(&mut app, key);
        run_until(&mut app, |action| {
            matches!(action, Action::NetworkDeleteChecked { .. })
        })
        .await;
        // Whatever the check's arrival queued has its say too.
        while let Ok(action) = app.action_rx.try_recv() {
            app.process_action(&action).expect("action");
        }
        assert_eq!(
            after_the_check(&app),
            (false, Some(checked.to_owned())),
            "{key:?}"
        );
        press(&mut app, KeyCode::Esc);
    }

    assert!(
        demo.networks_snapshot()
            .iter()
            .any(|network| network.name == "Studio"),
        "nothing was deleted"
    );
}

#[test]
fn only_a_controller_that_is_not_connected_refuses_writes() {
    use unifly_api::ConnectionState;

    assert!(state_accepts_writes(&ConnectionState::Connected));
    assert!(
        state_accepts_writes(&ConnectionState::Reconnecting { attempt: 1 }),
        "a failed refresh leaves the command path up"
    );
    for down in [
        ConnectionState::Disconnected,
        ConnectionState::Connecting,
        ConnectionState::Failed,
    ] {
        assert!(!state_accepts_writes(&down), "{down:?}");
    }
}

/// A controller whose periodic refresh is failing reads `Reconnecting`
/// while its clients and command processor are still up. A save and a
/// policy move are still sent there, and answer for themselves.
#[tokio::test]
async fn a_save_and_a_move_are_still_sent_while_a_refresh_is_failing() {
    use unifly_api::ConnectionState;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::tui::app::firewall::tests as fw;

    let server = MockServer::start().await;
    // Nothing a refresh reads is served, so the first one fails.
    let controller = fw::controller_refreshing(&server, 1).await;
    let mut state = controller.connection_state();
    tokio::time::timeout(
        Duration::from_secs(15),
        state.wait_for(|state| matches!(state, ConnectionState::Reconnecting { .. })),
    )
    .await
    .expect("a failed periodic refresh shows in the state")
    .expect("the state channel stays open");

    let id = uuid::Uuid::from_u128(0x4e7);
    let route = format!(
        "/proxy/network/integration/v1/sites/{}/networks/{id}",
        fw::SITE
    );
    let stored = json!({
        "id": id, "name": "Lab", "enabled": true, "management": "UNMANAGED",
        "vlanId": 30, "metadata": {"origin": "USER_DEFINED"},
    });
    Mock::given(method("GET"))
        .and(path(route.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(stored.clone()))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(route))
        .and(body_partial_json(json!({"name": "Workshop"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(stored))
        .expect(1)
        .mount(&server)
        .await;
    fw::mount_ordering_expecting(&server, (&[1, 3, 2], &[4]), 1).await;
    let mut app = App::new(Some(controller.clone()), None, false, true);

    app.process_action(&Action::NetworkSave(
        unifly_api::EntityId::Uuid(id),
        Box::new(unifly_api::UpdateNetworkRequest {
            name: Some("Workshop".into()),
            ..Default::default()
        }),
    ))
    .expect("save handling");
    let saved = run_until(&mut app, |action| {
        matches!(action, Action::NetworkEditResult(_))
    })
    .await;
    assert!(
        matches!(saved, Action::NetworkEditResult(Ok(()))),
        "{saved:?}"
    );

    app.process_action(&fw::move_down()).expect("move handling");
    let moved = fw::outcome(&mut app).await;
    assert!(
        moved.message.starts_with("Moved policy-2 down"),
        "{}",
        moved.message
    );
    server.verify().await;
    controller.disconnect().await;
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
        (
            "In use by 12 clients, 1 WiFi network, 2 other settings. They lose this network. \
             This cannot be undone."
                .to_owned(),
            true
        )
    );
    assert_eq!(
        reference_summary(&json!({ "referenceResources": [] })),
        ("Nothing uses it. This cannot be undone.".to_owned(), false)
    );
    let unused = json!({ "referenceResources": [
        { "resourceType": "CLIENT", "referenceCount": 0 },
    ]});
    assert!(!reference_summary(&unused).1, "a zero count is no use");
}
