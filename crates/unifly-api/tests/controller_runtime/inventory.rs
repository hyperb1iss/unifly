use super::*;

const HISTORY_ROUTE: &str = "/v2/api/site/default/system-log/all";

async fn inventory_controller(
    server: &MockServer,
    api_key: bool,
    refresh_interval_secs: u64,
) -> Controller {
    let (auth, prefix) = if api_key {
        mock_api_key_with_legacy(server).await;
        Mock::given(method("GET"))
            .and(path(integration_route("devices")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(integration_page(&json!({
                    "id": "9b2f6c1e-7a3d-4c5b-8e9f-0a1b2c3d4e5f",
                    "macAddress": SWITCH_MAC,
                    "ipAddress": "10.0.0.2",
                    "name": "Core Switch",
                    "model": "USW-24",
                    "state": "ONLINE",
                    "supported": true,
                    "firmwareUpdatable": false,
                    "features": [],
                    "interfaces": {},
                }))),
            )
            .with_priority(1)
            .mount(server)
            .await;
        (AuthCredentials::ApiKey(secret("the-key")), "/proxy/network")
    } else {
        mock_legacy_connect(server, legacy_site_envelope()).await;
        (
            AuthCredentials::Credentials {
                username: "admin".into(),
                password: secret("password"),
            },
            "",
        )
    };
    Mock::given(method("GET"))
        .and(path(format!("{prefix}/api/s/default/stat/device")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(legacy_envelope(&json!([{
                "_id": "switch-1", "mac": SWITCH_MAC, "type": "usw",
                "name": "Core Switch", "model": "USW-24", "state": 1,
            }]))),
        )
        .with_priority(1)
        .mount(server)
        .await;

    let mut config = base_config(
        Url::parse(&server.uri()).unwrap(),
        auth,
        LEGACY_SITE_NAME,
        false,
    );
    config.refresh_interval_secs = refresh_interval_secs;
    config.polling_interval_secs = 0;
    config.timeout = Duration::from_secs(30);
    Controller::new(config)
}

fn history_page() -> serde_json::Value {
    json!({
        "data": [{"id": "event-1", "timestamp": chrono::Utc::now().timestamp_millis() - 1000,
            "category": "UNIFI_DEVICES", "message": "Switch connected"}],
        "page_number": 0, "total_element_count": 1, "total_page_count": 1,
    })
}

fn modern_route(api_key: bool) -> String {
    if api_key {
        format!("/proxy/network{HISTORY_ROUTE}")
    } else {
        HISTORY_ROUTE.to_owned()
    }
}

async fn history_requests(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| {
            request.url.path().ends_with("/system-log/all")
                || request.url.path().ends_with("/stat/event")
        })
        .count()
}

#[tokio::test]
async fn inventory_connect_loads_devices_and_sites_without_reading_slow_history() {
    for api_key in [false, true] {
        let server = MockServer::start().await;
        let controller = inventory_controller(&server, api_key, 0).await;
        Mock::given(method("POST"))
            .and(path(modern_route(api_key)))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(history_page())
                    .set_delay(Duration::from_secs(20)),
            )
            .expect(0)
            .mount(&server)
            .await;

        timeout(Duration::from_secs(3), controller.connect_inventory())
            .await
            .expect("inventory startup must not wait for event history")
            .unwrap();
        assert_eq!(controller.devices_snapshot().len(), 1);
        assert_eq!(controller.sites_snapshot().len(), 1);
        assert!(controller.events_snapshot().is_empty());
        assert_eq!(history_requests(&server).await, 0);
        controller.disconnect().await;
    }
}

#[tokio::test]
async fn full_refresh_keeps_history_contract_and_inventory_reconnect_preserves_events() {
    for api_key in [false, true] {
        let server = MockServer::start().await;
        let controller = inventory_controller(&server, api_key, 0).await;
        Mock::given(method("POST"))
            .and(path(modern_route(api_key)))
            .respond_with(ResponseTemplate::new(200).set_body_json(history_page()))
            .expect(2)
            .mount(&server)
            .await;

        controller.connect().await.unwrap();
        assert_eq!(history_requests(&server).await, 1);
        assert_eq!(controller.events_snapshot().len(), 1);

        controller.connect_inventory().await.unwrap();
        assert_eq!(history_requests(&server).await, 1);
        assert_eq!(controller.events_snapshot().len(), 1);

        controller.full_refresh().await.unwrap();
        assert_eq!(history_requests(&server).await, 2);
        assert_eq!(controller.events_snapshot().len(), 1);
        controller.disconnect().await;
    }
}

#[tokio::test]
async fn disconnect_cancels_a_periodic_refresh_waiting_for_history() {
    for api_key in [false, true] {
        let server = MockServer::start().await;
        let controller = inventory_controller(&server, api_key, 1).await;
        let request_started = Arc::new(Notify::new());
        let observed = Arc::clone(&request_started);
        Mock::given(method("POST"))
            .and(path(modern_route(api_key)))
            .respond_with(move |_: &Request| {
                observed.notify_one();
                ResponseTemplate::new(200)
                    .set_body_json(history_page())
                    .set_delay(Duration::from_secs(20))
            })
            .expect(1)
            .mount(&server)
            .await;

        controller.connect_inventory().await.unwrap();
        timeout(Duration::from_secs(4), request_started.notified())
            .await
            .expect("the periodic full refresh must start its history request");
        timeout(Duration::from_secs(2), controller.disconnect())
            .await
            .expect("disconnect must cancel the in-flight read instead of awaiting its timeout");
        assert_eq!(
            *controller.connection_state().borrow(),
            unifly_api::ConnectionState::Disconnected
        );
        assert!(controller.events_snapshot().is_empty());
        assert_eq!(history_requests(&server).await, 1);
    }
}
