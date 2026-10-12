//! Ordering and selection tests for the WiFi screen state.

use std::sync::Arc;

use serde_json::json;
use unifly_api::Device;

use crate::tui::action::{Action, WifiSortField};
use crate::tui::screens::wifi::WifiScreen;

fn access_point(id: &str, name: &str) -> Arc<Device> {
    Arc::new(
        serde_json::from_value(json!({
            "id": id,
            "mac": format!("aa:bb:cc:dd:ee:{id}"),
            "ip": null,
            "wan_ipv6": null,
            "name": name,
            "model": "U7 Pro",
            "device_type": "AccessPoint",
            "state": "Online",
            "firmware_version": null,
            "firmware_updatable": false,
            "adopted_at": null,
            "provisioned_at": null,
            "last_seen": null,
            "serial": null,
            "supported": true,
            "ports": [],
            "radios": [],
            "uplink_device_id": null,
            "uplink_device_mac": null,
            "uplink_port_idx": null,
            "has_switching": false,
            "has_access_point": true,
            "stats": {},
            "client_count": null,
            "origin": null
        }))
        .expect("access point fixture should deserialize"),
    )
}

fn ap_names(screen: &WifiScreen) -> Vec<String> {
    screen
        .ap_devices()
        .iter()
        .map(|device| device.name.clone().unwrap_or_default())
        .collect()
}

#[test]
fn health_ties_fall_back_to_the_cli_name_order() {
    let mut screen = WifiScreen::new();
    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![
        access_point("03", "ap-roof"),
        access_point("01", "AP-Atrium"),
        access_point("02", "ap-beacon"),
    ])));

    assert_eq!(screen.sort_field, WifiSortField::Health);
    assert_eq!(ap_names(&screen), vec!["AP-Atrium", "ap-beacon", "ap-roof"]);
}

#[test]
fn ap_selection_follows_the_ap_across_refreshes() {
    let mut screen = WifiScreen::new();
    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![
        access_point("01", "ap-atrium"),
        access_point("02", "ap-beacon"),
        access_point("03", "ap-roof"),
    ])));
    screen.ap_table_state.select(Some(1));

    screen.apply_action(&Action::DevicesUpdated(Arc::new(vec![
        access_point("03", "ap-roof"),
        access_point("04", "ap-aaa"),
        access_point("02", "ap-beacon"),
        access_point("01", "ap-atrium"),
    ])));

    assert_eq!(ap_names(&screen)[0], "ap-aaa");
    assert_eq!(
        screen.selected_ap().and_then(|ap| ap.name.clone()),
        Some("ap-beacon".to_owned())
    );
}
