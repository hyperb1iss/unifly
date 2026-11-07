use std::time::Duration;

use super::*;

#[tokio::test]
async fn repeated_modern_rows_fail_even_when_page_number_advances() {
    for subcategory in ["CLIENT_CONNECTION", "SECURITY_FIREWALL"] {
        let (server, client) = setup().await;
        let rows = vec![event("one", subcategory), event("two", subcategory)];
        page(&server, 0, 2, 4, rows.clone()).await;
        page(&server, 1, 2, 4, rows).await;
        let error = fetch_events(&client, EventHistoryQuery::default())
            .await
            .expect_err("repeated raw records must fail");
        assert!(error.to_string().contains("no new records"));
        assert_eq!(server.received_requests().await.expect("requests").len(), 2);
    }
}

#[tokio::test]
async fn idless_records_use_full_contents_including_unknown_fields() {
    for repeat in [true, false] {
        let (server, client) = setup().await;
        let mut first = event("unused", "CLIENT_CONNECTION");
        first["id"] = Value::Null;
        first["future_detail"] = json!({"sequence": 1});
        let mut second = first.clone();
        if !repeat {
            second["future_detail"] = json!({"sequence": 2});
        }
        page(&server, 0, 2, 2, vec![first]).await;
        page(&server, 1, 2, 2, vec![second]).await;
        let result = fetch_events(&client, EventHistoryQuery::default()).await;
        if repeat {
            assert!(
                result
                    .expect_err("repeated ID-less record")
                    .to_string()
                    .contains("no new records")
            );
        } else {
            let events = result.expect("distinct ID-less records");
            assert_eq!(events.len(), 2);
            assert_ne!(events[0].id, events[1].id);
        }
    }
}

#[tokio::test]
async fn partially_overlapping_pages_remain_valid() {
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
    let result = fetch_events(&client, EventHistoryQuery::default())
        .await
        .expect("overlapping history");
    assert_eq!(result.len(), 3);
}

#[tokio::test]
async fn totals_may_grow_or_shrink_during_history_walk() {
    for (first_total, final_total) in [(3, 4), (4, 3)] {
        let (server, client) = setup().await;
        page(
            &server,
            0,
            2,
            first_total,
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
            final_total,
            vec![
                event("two", "CLIENT_CONNECTION"),
                event("three", "CLIENT_CONNECTION"),
            ],
        )
        .await;
        let result = fetch_events(&client, EventHistoryQuery::default())
            .await
            .expect("history counted at query start remains complete");
        assert_eq!(result.len(), 3);
    }
}

#[tokio::test]
async fn overlapping_rows_cannot_mask_missing_history() {
    let (server, client) = setup().await;
    page(
        &server,
        0,
        2,
        4,
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
        4,
        vec![
            event("two", "CLIENT_CONNECTION"),
            event("three", "CLIENT_CONNECTION"),
        ],
    )
    .await;
    let error = fetch_events(&client, EventHistoryQuery::default())
        .await
        .expect_err("one distinct record is missing");
    assert!(error.to_string().contains("incomplete"));
}

#[tokio::test]
async fn repeated_legacy_firewall_pages_stop_after_two_reads() {
    let (server, client) = setup().await;
    Mock::given(method("POST"))
        .and(path(MODERN))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    let timestamp = Utc::now().timestamp_millis() - 1000;
    let rows: Vec<_> = (0..1000)
        .map(|index| json!({"_id": format!("block-{index}"), "time": timestamp, "key": "EVT_GW_FirewallBlock"}))
        .collect();
    Mock::given(method("POST"))
        .and(path(LEGACY))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"meta": {"rc": "ok"}, "data": rows})),
        )
        .expect(2)
        .mount(&server)
        .await;
    let error = tokio::time::timeout(
        Duration::from_secs(5),
        fetch_events(&client, EventHistoryQuery::default()),
    )
    .await
    .expect("repeated pages must terminate")
    .expect_err("repeated raw records");
    assert!(error.to_string().contains("no new records"));
    assert_eq!(server.received_requests().await.expect("requests").len(), 3);
}

#[tokio::test]
async fn continuation_flags_prevent_false_end_of_history() {
    for (has_next, or_more) in [(true, false), (false, true), (true, true)] {
        let (server, client) = setup().await;
        Mock::given(method("POST"))
            .and(path(MODERN))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "page_number": 0, "total_page_count": 1, "total_element_count": 1,
                "has_next": has_next, "or_more": or_more,
                "data": [event("one", "CLIENT_CONNECTION")]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let error = fetch_events(&client, EventHistoryQuery::default())
            .await
            .expect_err("incomplete history");
        assert!(error.to_string().contains("additional history"));
    }
}

#[tokio::test]
async fn crossing_short_page_requires_complete_totals_even_after_limit_is_met() {
    for complete in [false, true] {
        let (server, client) = setup().await;
        for (number, start, count) in [
            (0, 0, 500),
            (1, 500, 1000),
            (2, 1500, if complete { 1000 } else { 500 }),
        ] {
            let rows = (start..start + count)
                .map(|index| event(&format!("entry-{index}"), "CLIENT_CONNECTION"))
                .collect();
            page(&server, number, 3, 2500, rows).await;
        }
        let result = fetch_events(
            &client,
            EventHistoryQuery {
                limit: 1500,
                ..Default::default()
            },
        )
        .await;
        if complete {
            assert_eq!(result.expect("complete variable-length pages").len(), 1500);
        } else {
            assert!(
                result
                    .expect_err("totals reveal missing history")
                    .to_string()
                    .contains("incomplete")
            );
        }
        assert_eq!(server.received_requests().await.expect("requests").len(), 3);
    }
}

#[tokio::test]
async fn http_200_not_found_envelope_selects_legacy() {
    let (server, client) = setup().await;
    Mock::given(method("POST"))
        .and(path(MODERN))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"meta": {"rc": "error", "msg": "api.err.NotFound"}, "data": []}),
            ),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(LEGACY))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"meta": {"rc": "ok"}, "data": [
                {"_id": "legacy", "time": Utc::now().timestamp_millis() - 1000}
            ]}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let result = fetch_events(&client, EventHistoryQuery::default())
        .await
        .expect("legacy history");
    assert_eq!(ids(&result), ["legacy"]);
}
