use std::time::Duration;

use serde_json::json;
use unifly_api::{AuthCredentials, Controller, ControllerConfig};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{Action, App, StatsPeriod};
use crate::tui::action::Access;
use crate::tui::app::ConnectionStatus;

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
        let action = tokio::time::timeout(Duration::from_secs(5), app.action_rx.recv())
            .await
            .expect("history must publish even when DPI fails")
            .expect("action channel remains open");
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
        let action = tokio::time::timeout(Duration::from_secs(5), app.action_rx.recv())
            .await
            .expect("the newer fetch reports")
            .expect("action channel remains open");
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
