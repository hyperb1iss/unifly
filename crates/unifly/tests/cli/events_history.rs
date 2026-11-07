use super::demo_json;

#[test]
fn event_history_flags_keep_demo_offline_and_filter_only_firewall_blocks() {
    let visible = demo_json(&["events", "list", "--within", "168", "--limit", "500"]);
    let all = demo_json(&[
        "events",
        "list",
        "--within",
        "168",
        "--limit",
        "500",
        "--include-firewall",
    ]);
    let visible = visible.as_array().expect("visible events");
    let all = all.as_array().expect("all events");
    assert!(
        visible.len() > 100,
        "history must extend beyond the refresh limit"
    );
    assert!(all.len() > visible.len());
    assert!(
        visible
            .iter()
            .all(|row| row["event_type"] != "EVT_GW_FirewallBlock")
    );
    assert!(
        visible
            .iter()
            .any(|row| row["event_type"] == "EVT_IPS_IpsAlert")
    );
    assert!(
        all.iter()
            .any(|row| row["event_type"] == "EVT_GW_FirewallBlock")
    );
    assert!(visible.windows(2).all(|rows| {
        rows[0]["timestamp"].as_str().expect("timestamp")
            >= rows[1]["timestamp"].as_str().expect("timestamp")
    }));
    let limited = demo_json(&["events", "list", "--within", "168", "--limit", "150"]);
    assert_eq!(limited.as_array().expect("limited events").len(), 150);
    let empty = demo_json(&["events", "list", "--limit", "0"]);
    assert_eq!(empty, serde_json::json!([]));
}
