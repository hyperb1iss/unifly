use std::time::Duration;

use serde_json::json;
use unifly_api::{AuthCredentials, Controller, ControllerConfig};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{Action, App, StatsPeriod};
use crate::tui::action::NotificationLevel;

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
async fn dpi_failures_notify_without_discarding_loaded_history_or_other_dpi() {
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
        let mut app = App::new(Some(controller.clone()), None, false, false);
        app.fetch_stats(StatsPeriod::OneHour);
        let mut notices = Vec::new();
        let data = loop {
            let action = tokio::time::timeout(Duration::from_secs(5), app.action_rx.recv())
                .await
                .expect("history must publish even when DPI fails")
                .expect("action channel remains open");
            match action {
                Action::Notify(notice) => {
                    assert_eq!(notice.level, NotificationLevel::Error);
                    notices.push(notice.message);
                }
                Action::StatsUpdated(data) => break data,
                other => panic!("unexpected action: {other:?}"),
            }
        };
        assert_eq!(data.bandwidth_tx, vec![(1.0, 1000.0)]);
        assert_eq!(data.bandwidth_rx, vec![(1.0, 2000.0)]);
        assert_eq!(data.client_counts, vec![(1.0, 4.0)]);
        assert_eq!(data.dpi_apps.is_empty(), apps_fail);
        assert_eq!(data.dpi_categories.is_empty(), categories_fail);
        assert_eq!(
            notices.len(),
            usize::from(apps_fail) + usize::from(categories_fail)
        );
        assert_eq!(
            notices
                .iter()
                .any(|message| message.contains("application traffic")),
            apps_fail
        );
        assert_eq!(
            notices
                .iter()
                .any(|message| message.contains("traffic categories")),
            categories_fail
        );
        controller.disconnect().await;
        server.verify().await;
    }
}
