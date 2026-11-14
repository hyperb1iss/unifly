use std::time::Duration;

use serde_json::json;
use unifly_api::{AuthCredentials, Controller, ControllerConfig};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{Action, App, StatsPeriod};
use crate::tui::action::Access;
use crate::tui::app::ConnectionStatus;

/// The next queued action, with a fetch's result taken out of its fence.
async fn next_action(app: &mut App, why: &str) -> Action {
    let action = tokio::time::timeout(Duration::from_secs(5), app.action_rx.recv())
        .await
        .expect(why)
        .expect("action channel remains open");
    match action {
        Action::MonitorFetched { result, .. } => *result,
        other => other,
    }
}

async fn controller(server: &MockServer) -> Controller {
    for (route, status) in [("/api/auth/login", 401), ("/api/login", 404)] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(status))
            .mount(server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/proxy/network/integration/v1/sites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "offset": 0, "limit": 50, "count": 1, "totalCount": 1,
            "data": [{
                "id": "550e8400-e29b-41d4-a716-446655440000",
                "internalReference": "default", "name": "Default"
            }]
        })))
        .mount(server)
        .await;
    let controller = Controller::new(ControllerConfig {
        url: server.uri().parse().expect("mock URL"),
        auth: AuthCredentials::ApiKey("test-key".to_owned().into()),
        site: "default".into(),
        timeout: Duration::from_secs(5),
        refresh_interval_secs: 0,
        websocket_enabled: false,
        ..ControllerConfig::default()
    });
    controller
        .connect_lightweight()
        .await
        .expect("mock controller connects");
    controller
}

#[tokio::test]
async fn dpi_failures_ride_inline_without_discarding_loaded_history_or_other_dpi() {
    for (apps_fail, categories_fail) in [(true, false), (false, true), (true, true)] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/proxy/network/v2/api/stats/source"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!("LEGACY")))
            .mount(&server)
            .await;
        for (report, row) in [
            (
                "gw",
                json!({"time": 1000, "wan-tx_bytes": 300_000, "wan-rx_bytes": 600_000}),
            ),
            ("site", json!({"time": 1000, "num_sta": 4})),
        ] {
            Mock::given(method("POST"))
                .and(path(format!(
                    "/proxy/network/api/s/default/stat/report/5minutes.{report}"
                )))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "meta": {"rc": "ok"}, "data": [row]
                })))
                .expect(1)
                .mount(&server)
                .await;
        }
        for (group, fails) in [("applications", apps_fail), ("categories", categories_fail)] {
            let response = if fails {
                ResponseTemplate::new(403)
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "offset": 0, "limit": 200, "count": 1, "totalCount": 1,
                    "data": [{"id": 3, "name": "Streaming", "txBytes": 300, "rxBytes": 200}]
                }))
            };
            Mock::given(method("GET"))
                .and(path(format!("/proxy/network/integration/v1/dpi/{group}")))
                .respond_with(response)
                .expect(1)
                .mount(&server)
                .await;
        }
        Mock::given(method("GET"))
            .and(path(
                "/proxy/network/v2/api/site/default/traffic-flow-latest-statistics",
            ))
            .respond_with(ResponseTemplate::new(403))
            .expect(u64::from(apps_fail) + u64::from(categories_fail))
            .mount(&server)
            .await;
        let controller = controller(&server).await;
        let app = App::new(Some(controller.clone()), None, false, false);
        let mut app = app;
        let _task = app.fetch_stats(StatsPeriod::OneHour);
        let action = next_action(&mut app, "history must publish even when DPI fails").await;
        // A DPI failure rides along with the history, so a timed reload
        // cannot stack a toast for it every minute.
        let Action::StatsUpdated(data) = action else {
            panic!("unexpected action: {action:?}");
        };
        assert_eq!(data.period, StatsPeriod::OneHour);
        assert_eq!(data.bandwidth_tx, vec![(1.0, 1000.0)]);
        assert_eq!(data.bandwidth_rx, vec![(1.0, 2000.0)]);
        assert_eq!(data.client_counts, vec![(1.0, 4.0)]);
        assert_eq!(data.dpi_apps.is_empty(), apps_fail);
        assert_eq!(data.dpi_categories.is_empty(), categories_fail);
        let error = data.dpi_error.unwrap_or_default();
        assert_eq!(error.contains("applications:"), apps_fail, "{error}");
        assert_eq!(error.contains("categories:"), categories_fail, "{error}");
        controller.disconnect().await;
        server.verify().await;
    }
}

async fn demo_app(session: bool) -> App {
    let demo = Controller::demo(unifly_api::DemoFleetOptions::default());
    demo.connect().await.expect("demo fleet connects");
    let mut app = App::new(Some(demo), None, false, false);
    app.connection_status = ConnectionStatus::Connected;
    app.access = Access {
        integration: true,
        session,
        live_events: session,
    };
    app
}

#[tokio::test]
async fn statistics_are_not_requested_without_session_access() {
    let mut app = demo_app(false).await;

    app.process_action(&Action::RequestStats(StatsPeriod::SevenDays))
        .expect("request");

    assert!(app.monitor.stats_task.is_none(), "nothing was fetched");
    assert_eq!(app.stats_period, StatsPeriod::SevenDays);
    assert!(app.action_rx.try_recv().is_err(), "and nothing was toasted");
}

#[tokio::test]
async fn statistics_are_not_requested_while_disconnected() {
    let mut app = demo_app(true).await;
    // A retry after a failed connect: nothing was ever reached.
    for action in [
        Action::Disconnected("refused".into()),
        Action::Reconnecting { attempt: 2 },
    ] {
        app.process_action(&action).expect("link");
        app.process_action(&Action::RequestStats(StatsPeriod::OneHour))
            .expect("request");
        assert!(app.monitor.stats_task.is_none(), "{action:?}");
    }
}

#[tokio::test]
async fn statistics_are_still_requested_after_one_failed_refresh() {
    let mut app = demo_app(true).await;
    app.process_action(&Action::Connected).expect("connected");
    app.process_action(&Action::Reconnecting { attempt: 1 })
        .expect("a refresh failed");
    assert_eq!(
        app.connection_status,
        ConnectionStatus::Reconnecting { attempt: 1 }
    );

    app.process_action(&Action::RequestStats(StatsPeriod::OneHour))
        .expect("request");
    assert!(app.monitor.stats_task.is_some(), "the clients are still up");

    // The timed reload goes on too.
    app.active_screen = crate::tui::screen::ScreenId::Stats;
    app.last_stats_fetch = std::time::Instant::now().checked_sub(Duration::from_mins(2));
    while app.action_rx.try_recv().is_ok() {}
    app.process_action(&Action::Tick).expect("tick");
    let mut reloads = 0;
    while let Ok(action) = app.action_rx.try_recv() {
        reloads += usize::from(matches!(action, Action::RequestStats(_)));
    }
    assert_eq!(reloads, 1);
}

#[tokio::test]
async fn a_newer_request_aborts_the_one_in_flight() {
    let mut app = demo_app(true).await;

    app.process_action(&Action::RequestStats(StatsPeriod::OneHour))
        .expect("first request");
    let first = app.monitor.stats_task.clone().expect("a fetch started");
    app.process_action(&Action::RequestStats(StatsPeriod::SevenDays))
        .expect("second request");

    let data = loop {
        let action = next_action(&mut app, "the newer fetch reports").await;
        if let Action::StatsUpdated(data) = action {
            break data;
        }
    };
    assert_eq!(data.period, StatsPeriod::SevenDays);
    assert!(first.is_finished(), "the superseded fetch was stopped");
    assert!(app.action_rx.try_recv().is_err(), "and never reported");
}

#[tokio::test]
async fn regaining_access_reloads_the_statistics_on_screen() {
    use crate::tui::screen::ScreenId;

    let mut app = demo_app(true).await;
    app.active_screen = ScreenId::Stats;
    app.stats_period = StatsPeriod::ThirtyDays;

    app.process_action(&Action::AccessUpdated(app.access))
        .expect("access");

    let queued = app.action_rx.try_recv().expect("a reload is queued");
    assert!(
        matches!(queued, Action::RequestStats(StatsPeriod::ThirtyDays)),
        "{queued:?}"
    );
}

/// A controller whose history and DPI catalog both answer, so no fetch
/// falls back to the session counters.
async fn stats_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/proxy/network/v2/api/stats/source"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!("LEGACY")))
        .mount(&server)
        .await;
    for (report, row) in [
        ("gw", json!({"time": 1000, "wan-tx_bytes": 300_000})),
        ("site", json!({"time": 1000, "num_sta": 4})),
    ] {
        Mock::given(method("POST"))
            .and(path(format!(
                "/proxy/network/api/s/default/stat/report/5minutes.{report}"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "meta": {"rc": "ok"}, "data": [row]
            })))
            .mount(&server)
            .await;
    }
    for group in ["applications", "categories"] {
        Mock::given(method("GET"))
            .and(path(format!("/proxy/network/integration/v1/dpi/{group}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "offset": 0, "limit": 200, "count": 1, "totalCount": 1,
                "data": [{"id": 3, "name": "Streaming", "txBytes": 300, "rxBytes": 200}]
            })))
            .mount(&server)
            .await;
    }
    server
}

/// How many times the controller was asked for its DPI applications.
async fn dpi_reads(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|request| request.url.path().ends_with("/dpi/applications"))
        .count()
}

/// Run one stats load to its result and let the app take it in.
async fn load(app: &mut App, request: &Action) -> crate::tui::action::StatsData {
    app.process_action(request).expect("request");
    loop {
        let action = tokio::time::timeout(Duration::from_secs(5), app.action_rx.recv())
            .await
            .expect("the load reports")
            .expect("action channel remains open");
        app.process_action(&action).expect("process");
        if let Action::MonitorFetched { result, .. } = action
            && let Action::StatsUpdated(data) = *result
        {
            return data;
        }
    }
}

#[tokio::test]
async fn dpi_totals_are_reused_briefly_then_read_again() {
    let server = stats_server().await;
    let controller = controller(&server).await;
    let mut app = App::new(Some(controller.clone()), None, false, false);
    app.connection_status = ConnectionStatus::Connected;
    app.access = Access {
        integration: true,
        session: true,
        live_events: false,
    };
    let timed = Action::RequestStats(StatsPeriod::OneHour);

    let first = load(&mut app, &timed).await;
    assert!(first.dpi_fetched);
    assert_eq!(first.dpi_apps, vec![("Streaming".to_owned(), 500)]);
    assert_eq!(dpi_reads(&server).await, 1);
    let stamped = app.monitor.dpi.as_ref().expect("the totals are kept").0;

    // A reload inside the reuse window shows the same totals without a
    // read, and does not make them look newer than they are.
    let reused = load(&mut app, &timed).await;
    assert!(!reused.dpi_fetched);
    assert_eq!(reused.dpi_apps, first.dpi_apps);
    assert_eq!(dpi_reads(&server).await, 1);
    assert_eq!(
        app.monitor.dpi.as_ref().map(|(at, _)| *at),
        Some(stamped),
        "reused totals keep the time they were read"
    );

    // Once they are older than the window, the timed reload reads again.
    let old = stamped
        .checked_sub(Duration::from_mins(3))
        .expect("the clock is past three minutes");
    if let Some((at, _)) = app.monitor.dpi.as_mut() {
        *at = old;
    }
    assert!(load(&mut app, &timed).await.dpi_fetched);
    assert_eq!(dpi_reads(&server).await, 2);

    // Asking for a reload reads again at once, however fresh they are.
    let asked = load(&mut app, &Action::ReloadStats(StatsPeriod::OneHour)).await;
    assert!(asked.dpi_fetched);
    assert_eq!(dpi_reads(&server).await, 3);

    // So does a full refresh.
    app.process_action(&Action::Refresh).expect("refresh");
    assert!(app.monitor.dpi.is_none());
    controller.disconnect().await;
}
