use super::*;

#[tokio::test]
async fn modern_history_refresh_works_with_session_and_api_key_auth() {
    for api_key in [false, true] {
        let server = MockServer::start().await;
        let (auth, route) = if api_key {
            mock_api_key_with_legacy(&server).await;
            (
                AuthCredentials::ApiKey(secret("the-key")),
                "/proxy/network/v2/api/site/default/system-log/all",
            )
        } else {
            mock_legacy_connect(&server, legacy_site_envelope()).await;
            (
                AuthCredentials::Credentials {
                    username: "admin".into(),
                    password: secret("password"),
                },
                "/v2/api/site/default/system-log/all",
            )
        };
        let timestamp = chrono::Utc::now().timestamp_millis() - 1_000;
        let rows = json!([
            {"id": "block", "timestamp": timestamp, "category": "SECURITY",
             "subcategory": "SECURITY_FIREWALL", "message": "Packet blocked"},
            {"id": "ips", "timestamp": timestamp, "category": "SECURITY",
             "subcategory": "SECURITY_INTRUSION_PREVENTION", "severity": "HIGH",
             "message": "Threat detected"},
            {"id": "client", "timestamp": timestamp, "category": "CLIENT_DEVICES",
             "message_raw": "{CLIENT} connected", "parameters": {"CLIENT": {"name": "Laptop"}}},
            {"id": "audit", "timestamp": timestamp, "category": "AUDIT",
             "message": "Administrator signed in"}
        ]);
        let phase = Arc::new(std::sync::Mutex::new(RoutePhase::Populated));
        Mock::given(method("POST"))
            .and(path(route))
            .and(body_partial_json(json!({
                "pageNumber": 0, "sortByField": "TIMESTAMP", "sortDirection": "DESCENDING"
            })))
            .respond_with(SwitchableRoute {
                phase: Arc::clone(&phase),
                populated: json!({"data": rows, "page_number": 0,
                    "total_element_count": 4, "total_page_count": 1}),
                empty: json!({"data": [], "page_number": 0,
                    "total_element_count": 0, "total_page_count": 0}),
            })
            .mount(&server)
            .await;
        let controller = Controller::new(base_config(
            Url::parse(&server.uri()).unwrap(),
            auth,
            LEGACY_SITE_NAME,
            false,
        ));
        controller.connect().await.unwrap();
        let events = controller.events_snapshot();
        assert_eq!(events.len(), 3);
        assert!(
            events
                .iter()
                .any(|event| event.message == "Laptop connected")
        );
        assert!(
            events
                .iter()
                .any(|event| event.category == unifly_api::EventCategory::Admin)
        );
        assert!(
            events
                .iter()
                .any(|event| event.severity == unifly_api::EventSeverity::Error)
        );
        let all = controller
            .list_events(unifly_api::EventHistoryQuery {
                include_firewall: true,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(all.len(), 4);

        set_phase(&phase, RoutePhase::Failing(500));
        controller.full_refresh().await.unwrap();
        assert_eq!(controller.events_snapshot().len(), 3);
        assert!(
            controller
                .list_events(unifly_api::EventHistoryQuery::default())
                .await
                .is_err()
        );
        set_phase(&phase, RoutePhase::Empty);
        controller.full_refresh().await.unwrap();
        assert!(controller.events_snapshot().is_empty());
        controller.disconnect().await;
    }
}
