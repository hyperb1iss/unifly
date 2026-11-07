use serde_json::{Value, json};
use url::Url;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::ControllerPlatform;
use crate::session::SessionAuth;

use super::*;

const MODERN: &str = "/v2/api/site/default/system-log/all";
const LEGACY: &str = "/api/s/default/stat/event";

mod pagination;

async fn setup() -> (MockServer, SessionClient) {
    crate::transport::ensure_crypto_provider();
    let server = MockServer::start().await;
    let client = SessionClient::with_client(
        reqwest::Client::new(),
        Url::parse(&server.uri()).expect("URL"),
        "default".into(),
        ControllerPlatform::ClassicController,
        SessionAuth::Cookie,
    );
    (server, client)
}

fn event(id: &str, subcategory: &str) -> Value {
    json!({
        "id": id,
        "timestamp": Utc::now().timestamp_millis() - 1000,
        "category": "SECURITY",
        "subcategory": subcategory,
        "key": "shared-type-key",
        "message_raw": "test {CLIENT}",
        "parameters": {"CLIENT": {"name": "laptop"}}
    })
}

async fn page(server: &MockServer, number: u32, pages: u32, total: u64, rows: Vec<Value>) {
    Mock::given(method("POST"))
        .and(path(MODERN))
        .and(body_partial_json(json!({"pageNumber": number})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": rows,
            "page_number": number,
            "total_page_count": pages,
            "total_element_count": total
        })))
        .expect(1)
        .mount(server)
        .await;
}

fn ids(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .map(|event| event.id.as_ref().expect("ID").to_string())
        .collect()
}

#[tokio::test]
async fn filtered_short_pages_do_not_hide_later_ips_or_clients() {
    let (server, client) = setup().await;
    page(&server, 0, 3, 4, vec![event("block", "SECURITY_FIREWALL")]).await;
    page(
        &server,
        1,
        3,
        4,
        vec![event("ips", "SECURITY_INTRUSION_PREVENTION")],
    )
    .await;
    page(
        &server,
        2,
        3,
        4,
        vec![
            event("client", "CLIENT_CONNECTION"),
            event("client-two", "CLIENT_CONNECTION"),
        ],
    )
    .await;
    let events = fetch_events(
        &client,
        EventHistoryQuery {
            limit: 3,
            within_hours: 168,
            ..Default::default()
        },
    )
    .await
    .expect("history");
    assert_eq!(events.len(), 3);
    let found = ids(&events);
    assert!(found.contains(&"ips".to_owned()));
    assert!(found.contains(&"client".to_owned()));
    let requests = server.received_requests().await.expect("requests");
    let bodies: Vec<Value> = requests
        .iter()
        .map(|request| serde_json::from_slice(&request.body).expect("body"))
        .collect();
    let first = bodies.first().expect("request");
    for body in &bodies {
        assert_eq!(body["timestampFrom"], first["timestampFrom"]);
        assert_eq!(body["timestampTo"], first["timestampTo"]);
        assert_eq!(body["sortByField"], "TIMESTAMP");
        assert_eq!(body["sortDirection"], "DESCENDING");
        assert!(body.get("type").is_none());
    }
    assert_eq!(
        first["timestampTo"].as_i64().expect("end")
            - first["timestampFrom"].as_i64().expect("start"),
        168 * 3_600_000
    );
}

#[tokio::test]
async fn include_firewall_and_limits_above_refresh_size_work() {
    let (server, client) = setup().await;
    let rows = (0..180)
        .map(|index| event(&format!("event-{index}"), "SECURITY_FIREWALL"))
        .collect();
    page(&server, 0, 1, 180, rows).await;
    let events = fetch_events(
        &client,
        EventHistoryQuery {
            limit: 150,
            include_firewall: true,
            ..Default::default()
        },
    )
    .await
    .expect("history");
    assert_eq!(events.len(), 150);
}

#[tokio::test]
async fn duplicate_ids_deduplicate_without_treating_type_keys_as_identity() {
    let (server, client) = setup().await;
    page(
        &server,
        0,
        2,
        3,
        vec![
            event("one", "CLIENT_CONNECTION"),
            event("two", "CLIENT_CONNECTION"),
        ],
    )
    .await;
    page(
        &server,
        1,
        2,
        3,
        vec![
            event("two", "CLIENT_CONNECTION"),
            event("three", "CLIENT_CONNECTION"),
        ],
    )
    .await;
    let events = fetch_events(&client, EventHistoryQuery::default())
        .await
        .expect("history");
    assert_eq!(events.len(), 3);
}

#[tokio::test]
async fn empty_modern_result_never_falls_back() {
    let (server, client) = setup().await;
    page(&server, 0, 0, 0, vec![]).await;
    Mock::given(method("POST"))
        .and(path(LEGACY))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    assert!(
        fetch_events(&client, EventHistoryQuery::default())
            .await
            .expect("history")
            .is_empty()
    );
}

#[tokio::test]
async fn only_initial_not_found_selects_bounded_legacy_post() {
    let (server, client) = setup().await;
    Mock::given(method("POST"))
        .and(path(MODERN))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST")).and(path(LEGACY))
        .and(body_partial_json(json!({"_start": 0, "_limit": 1000, "within": 168, "_sort": "-time", "type": null})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"meta": {"rc": "ok"}, "data": [
            {"_id": "legacy", "time": Utc::now().timestamp_millis() - 1000, "msg": "{user} joined", "user": "laptop"},
            {"_id": "old", "time": 0, "msg": "outside window"}
        ]}))).expect(1).mount(&server).await;
    let events = fetch_events(
        &client,
        EventHistoryQuery {
            within_hours: 168,
            ..Default::default()
        },
    )
    .await
    .expect("history");
    assert_eq!(ids(&events), ["legacy"]);
    assert_eq!(events.first().expect("event").message, "laptop joined");
}

#[tokio::test]
async fn auth_server_and_parse_errors_never_select_legacy() {
    for status in [401, 403, 500, 200] {
        let (server, client) = setup().await;
        Mock::given(method("POST"))
            .and(path(MODERN))
            .respond_with(
                ResponseTemplate::new(status).set_body_json(json!({"message": "api.err.NotFound"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(LEGACY))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        assert!(
            fetch_events(&client, EventHistoryQuery::default())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn later_page_error_does_not_return_partial_success_or_fallback() {
    let (server, client) = setup().await;
    page(&server, 0, 2, 2, vec![event("first", "CLIENT_CONNECTION")]).await;
    Mock::given(method("POST"))
        .and(path(MODERN))
        .and(body_partial_json(json!({"pageNumber": 1})))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(LEGACY))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    assert!(
        fetch_events(&client, EventHistoryQuery::default())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn repeated_page_metadata_and_incomplete_totals_fail() {
    for response in [
        json!({"page_number": 0, "total_page_count": 2, "total_element_count": 3, "data": [event("again", "CLIENT_CONNECTION")]}),
        json!({"page_number": 1, "total_page_count": 2, "total_element_count": 3, "data": [event("last", "CLIENT_CONNECTION")]}),
    ] {
        let (server, client) = setup().await;
        page(&server, 0, 2, 3, vec![event("first", "CLIENT_CONNECTION")]).await;
        Mock::given(method("POST"))
            .and(path(MODERN))
            .and(body_partial_json(json!({"pageNumber": 1})))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .expect(1)
            .mount(&server)
            .await;
        assert!(
            fetch_events(&client, EventHistoryQuery::default())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn zero_limit_makes_no_request() {
    let (server, client) = setup().await;
    assert!(
        fetch_events(
            &client,
            EventHistoryQuery {
                limit: 0,
                ..Default::default()
            }
        )
        .await
        .expect("history")
        .is_empty()
    );
    assert!(
        server
            .received_requests()
            .await
            .expect("requests")
            .is_empty()
    );
}

#[tokio::test]
async fn legacy_pagination_counts_filtered_raw_rows_and_keeps_ips() {
    let (server, client) = setup().await;
    Mock::given(method("POST"))
        .and(path(MODERN))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    let timestamp = Utc::now().timestamp_millis() - 1000;
    let blocks: Vec<_> = (0..1000)
        .map(|index| json!({"_id": format!("block-{index}"), "time": timestamp, "key": "EVT_GW_FirewallBlock"}))
        .collect();
    for (offset, data) in [
        (0, blocks),
        (
            1000,
            vec![json!({"_id": "ips", "time": timestamp, "key": "EVT_IPS_IpsAlert"})],
        ),
    ] {
        Mock::given(method("POST"))
            .and(path(LEGACY))
            .and(body_partial_json(json!({"_start": offset})))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"meta": {"rc": "ok"}, "data": data})),
            )
            .expect(1)
            .mount(&server)
            .await;
    }
    let events = fetch_events(&client, EventHistoryQuery::default())
        .await
        .expect("history");
    assert_eq!(ids(&events), ["ips"]);
}

#[test]
fn huge_history_window_clamps_to_epoch() {
    let history = History::new(EventHistoryQuery {
        within_hours: u32::MAX,
        ..Default::default()
    });
    assert_eq!(history.start, 0);
}

#[test]
fn equal_timestamps_have_stable_selection_before_truncation() {
    let timestamp = Utc::now().timestamp_millis() - 1000;
    for order in [
        ["z-last", "a-first", "m-middle"],
        ["m-middle", "a-first", "z-last"],
    ] {
        let mut history = History::new(EventHistoryQuery {
            limit: 2,
            ..Default::default()
        });
        for id in order {
            let mut row = event(id, "CLIENT_CONNECTION");
            row["timestamp"] = json!(timestamp);
            let row: crate::session::system_log::SystemLogEntry =
                serde_json::from_value(row).expect("entry");
            history.push(Event::try_from(row).expect("event"), false);
        }
        assert_eq!(ids(&history.finish()), ["a-first", "m-middle"]);
    }
}

#[cfg(feature = "demo")]
#[tokio::test]
async fn demo_history_remains_offline_and_obeys_limit_and_visibility() {
    let controller = Controller::demo(crate::DemoFleetOptions::default());
    controller.connect().await.expect("demo connects");
    assert!(controller.inner.session_client.lock().await.is_none());
    let filtered = controller
        .list_events(EventHistoryQuery {
            limit: 2000,
            within_hours: 168,
            ..Default::default()
        })
        .await
        .expect("demo history");
    let all = controller
        .list_events(EventHistoryQuery {
            limit: 2000,
            within_hours: 168,
            include_firewall: true,
        })
        .await
        .expect("demo history");
    assert!(!filtered.is_empty());
    assert!(
        filtered
            .iter()
            .all(|event| event.event_type != "EVT_GW_FirewallBlock")
    );
    assert!(all.len() > filtered.len());
    let limited = controller
        .list_events(EventHistoryQuery {
            limit: 2,
            ..Default::default()
        })
        .await
        .expect("limited history");
    assert_eq!(limited.len(), 2);
    assert!(
        limited
            .windows(2)
            .all(|pair| pair.first().expect("first").timestamp
                >= pair.last().expect("last").timestamp)
    );
    controller.disconnect().await;
}
