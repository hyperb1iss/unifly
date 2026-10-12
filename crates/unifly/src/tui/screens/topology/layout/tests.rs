use std::sync::Arc;

use serde_json::json;
use unifly_api::{Device, DeviceType, UplinkMedium};

use super::{LinkKind, TopoLayout, layout};

/// A wide canvas, so these structural tests never wrap.
const WIDE: f64 = 400.0;

fn wireless(device: Arc<Device>) -> Arc<Device> {
    let mut device = Arc::unwrap_or_clone(device);
    device.uplink_medium = Some(UplinkMedium::Wireless);
    Arc::new(device)
}

fn device(
    name: &str,
    mac: &str,
    device_type: &str,
    uplink: Option<&str>,
    port: Option<u32>,
) -> Arc<Device> {
    Arc::new(
        serde_json::from_value(json!({
            "id": format!("id-{name}"),
            "mac": mac,
            "ip": "10.0.0.2",
            "wan_ipv6": null,
            "name": name,
            "model": "UniFi",
            "device_type": device_type,
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
            "uplink_device_mac": uplink,
            "uplink_port_idx": port,
            "has_switching": false,
            "has_access_point": false,
            "stats": {},
            "client_count": null,
            "origin": null
        }))
        .expect("device fixture should deserialize"),
    )
}

fn index(layout: &TopoLayout, name: &str) -> usize {
    layout
        .nodes
        .iter()
        .position(|node| node.label == name)
        .expect("node exists")
}

fn parent_of(layout: &TopoLayout, name: &str) -> Option<(String, LinkKind)> {
    let child = index(layout, name);
    layout
        .edges
        .iter()
        .find(|edge| edge.child == child)
        .map(|edge| (layout.nodes[edge.parent].label.clone(), edge.kind))
}

/// gw ─ core ─┬ edge-sw ─ ap-b
///            ├ ap-a ≈ ap-mesh
///            └ (orphan) ap-lost
fn fleet() -> Vec<Arc<Device>> {
    vec![
        device("ap-lost", "00:00:00:00:00:07", "AccessPoint", None, None),
        wireless(device(
            "ap-mesh",
            "00:00:00:00:00:06",
            "AccessPoint",
            Some("00:00:00:00:00:04"),
            None,
        )),
        device(
            "ap-b",
            "00:00:00:00:00:05",
            "AccessPoint",
            Some("00:00:00:00:00:03"),
            Some(4),
        ),
        device(
            "ap-a",
            "00:00:00:00:00:04",
            "AccessPoint",
            Some("00:00:00:00:00:02"),
            Some(17),
        ),
        device(
            "edge-sw",
            "00:00:00:00:00:03",
            "Switch",
            Some("00:00:00:00:00:02"),
            Some(9),
        ),
        device(
            "core",
            "00:00:00:00:00:02",
            "Switch",
            Some("00:00:00:00:00:01"),
            Some(25),
        ),
        device("gw", "00:00:00:00:00:01", "Gateway", None, None),
    ]
}

#[test]
fn edges_follow_reported_uplinks_only() {
    let layout = layout(&fleet(), WIDE);
    assert_eq!(layout.edges.len(), 6, "one edge per non-root device");
    assert_eq!(parent_of(&layout, "gw"), None);
    assert_eq!(
        parent_of(&layout, "core"),
        Some(("gw".into(), LinkKind::Wired))
    );
    assert_eq!(
        parent_of(&layout, "edge-sw"),
        Some(("core".into(), LinkKind::Wired))
    );
    assert_eq!(
        parent_of(&layout, "ap-b"),
        Some(("edge-sw".into(), LinkKind::Wired))
    );
    assert_eq!(
        parent_of(&layout, "ap-a"),
        Some(("core".into(), LinkKind::Wired))
    );
    let ap_b = index(&layout, "ap-b");
    let port = layout
        .edges
        .iter()
        .find(|edge| edge.child == ap_b)
        .and_then(|edge| edge.port);
    assert_eq!(port, Some(4));
}

#[test]
fn mesh_aps_get_wireless_links_and_orphans_hang_off_the_gateway() {
    let layout = layout(&fleet(), WIDE);
    assert_eq!(
        parent_of(&layout, "ap-mesh"),
        Some(("ap-a".into(), LinkKind::Wireless))
    );
    assert_eq!(
        parent_of(&layout, "ap-lost"),
        Some(("gw".into(), LinkKind::Inferred))
    );
}

#[test]
fn children_sit_below_parents_without_overlapping() {
    let layout = layout(&fleet(), WIDE);
    for edge in &layout.edges {
        let (parent, child) = (&layout.nodes[edge.parent], &layout.nodes[edge.child]);
        assert!(
            child.y > parent.bottom(),
            "{} should sit below {}",
            child.label,
            parent.label
        );
    }
    for (left_index, left) in layout.nodes.iter().enumerate() {
        for right in layout.nodes.iter().skip(left_index + 1) {
            if (left.y - right.y).abs() < f64::EPSILON {
                let apart = left.x + left.width <= right.x || right.x + right.width <= left.x;
                assert!(apart, "{} overlaps {}", left.label, right.label);
            }
        }
    }
    let [x_min, x_max, y_min, y_max] = layout.bounds;
    assert!(x_min < x_max && y_min < y_max);
}

#[test]
fn link_kind_follows_the_reported_medium_not_the_device_types() {
    let devices = vec![
        device("gw", "00:00:00:00:00:01", "Gateway", None, None),
        device(
            "ap-a",
            "00:00:00:00:00:02",
            "AccessPoint",
            Some("00:00:00:00:00:01"),
            Some(3),
        ),
        // AP to AP over a cable whose remote port went unreported.
        device(
            "ap-wired",
            "00:00:00:00:00:03",
            "AccessPoint",
            Some("00:00:00:00:00:02"),
            None,
        ),
        // Wireless uplink straight to a radio-bearing gateway.
        wireless(device(
            "ap-air",
            "00:00:00:00:00:04",
            "AccessPoint",
            Some("00:00:00:00:00:01"),
            None,
        )),
    ];
    let layout = layout(&devices, WIDE);
    assert_eq!(
        parent_of(&layout, "ap-wired"),
        Some(("ap-a".into(), LinkKind::Wired))
    );
    assert_eq!(
        parent_of(&layout, "ap-air"),
        Some(("gw".into(), LinkKind::Wireless))
    );
}

#[test]
fn uplink_cycles_do_not_hang_the_layout() {
    let devices = vec![
        device("gw", "00:00:00:00:00:01", "Gateway", None, None),
        device(
            "a",
            "00:00:00:00:00:02",
            "Switch",
            Some("00:00:00:00:00:03"),
            Some(1),
        ),
        device(
            "b",
            "00:00:00:00:00:03",
            "Switch",
            Some("00:00:00:00:00:02"),
            Some(1),
        ),
    ];
    let layout = layout(&devices, WIDE);
    assert_eq!(layout.nodes.len(), 3);
    assert_eq!(layout.edges.len(), 2, "the loop is cut once and reattached");
}

#[test]
fn empty_network_has_sane_bounds() {
    let layout = layout(&[], WIDE);
    assert_eq!(layout.nodes.len(), 0);
    assert_eq!(layout.edges.len(), 0);
    let [x_min, x_max, y_min, y_max] = layout.bounds;
    assert!(x_min < x_max && y_min < y_max);
}

#[test]
fn gateway_without_uplink_data_becomes_a_star() {
    let devices = vec![
        device("gw", "00:00:00:00:00:01", "Gateway", None, None),
        device("sw", "00:00:00:00:00:02", "Switch", None, None),
        device("ap", "00:00:00:00:00:03", "AccessPoint", None, None),
    ];
    let layout = layout(&devices, WIDE);
    let kinds: Vec<LinkKind> = layout.edges.iter().map(|edge| edge.kind).collect();
    assert_eq!(kinds, [LinkKind::Inferred, LinkKind::Inferred]);
    let gateway = &layout.nodes[index(&layout, "gw")];
    assert_eq!(gateway.device_type, DeviceType::Gateway);
    let sw = &layout.nodes[index(&layout, "sw")];
    let ap = &layout.nodes[index(&layout, "ap")];
    assert!(sw.y > gateway.bottom() && (sw.y - ap.y).abs() < f64::EPSILON);
}

/// A gateway with four switches and four access points directly under it,
/// with names long enough that one row cannot hold them at 80 columns.
fn crowded() -> Vec<Arc<Device>> {
    let names = [
        ("sw-core-main", "Switch"),
        ("sw-render-bay", "Switch"),
        ("sw-lounge-flex", "Switch"),
        ("sw-loading-dock", "Switch"),
        ("ap-lighthouse", "AccessPoint"),
        ("ap-gallery-west", "AccessPoint"),
        ("ap-roof-deck", "AccessPoint"),
        ("ap-render-bay", "AccessPoint"),
    ];
    let mut devices = vec![device(
        "gw-harbor",
        "00:00:00:00:00:01",
        "Gateway",
        None,
        None,
    )];
    for (index, (name, kind)) in names.iter().enumerate() {
        let mac = format!("00:00:00:00:01:{index:02}");
        let port = u32::try_from(index + 1).ok();
        devices.push(device(name, &mac, kind, Some("00:00:00:00:00:01"), port));
    }
    devices
}

#[test]
fn crowded_siblings_wrap_instead_of_overlapping_at_80_columns() {
    let columns = 80.0;
    let layout = layout(&crowded(), columns);
    let [x_min, x_max, _, _] = layout.bounds;
    assert!(
        x_max - x_min <= columns - 2.0 * super::MARGIN,
        "tree is {} cells wide",
        x_max - x_min
    );

    // Every label sits inside its box, on its own row band.
    for node in &layout.nodes {
        let (start, len) = node.label_span();
        assert!(start + f64::from(u32::try_from(len).unwrap_or(u32::MAX)) < node.x + node.width);
    }
    for (index, left) in layout.nodes.iter().enumerate() {
        for right in layout.nodes.iter().skip(index + 1) {
            let same_band = left.y < right.bottom() && right.y < left.bottom();
            let apart = left.x + left.width < right.x || right.x + right.width < left.x;
            assert!(
                !same_band || apart,
                "{} and {} overlap",
                left.label,
                right.label
            );
        }
    }
    let gateway_y = layout.nodes[0].y;
    let first_child_y = layout
        .nodes
        .iter()
        .skip(1)
        .map(|node| node.y)
        .fold(f64::MAX, f64::min);
    let rows = layout
        .nodes
        .iter()
        .skip(1)
        .filter(|node| node.y > first_child_y)
        .count();
    assert!(gateway_y < first_child_y);
    assert!(rows > 0, "eight wide children must wrap onto a second row");
}

#[test]
fn every_name_survives_rendering_at_80_columns() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut screen = super::super::TopologyScreen::new();
    screen.devices = Arc::new(crowded());
    let mut terminal = Terminal::new(TestBackend::new(80, 40)).expect("test terminal");
    terminal
        .draw(|frame| screen.render_screen(frame, frame.area()))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    let text: Vec<String> = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect()
        })
        .collect();
    for device in crowded() {
        let name = device.name.clone().unwrap_or_default();
        assert!(
            text.iter().any(|row| row.contains(&name)),
            "{name} is not drawn whole:\n{}",
            text.join("\n")
        );
    }
}

/// A gateway with four access points whose names are wide CJK characters,
/// two terminal cells each, as UniFi device names can be in any language.
fn wide_siblings() -> Vec<Arc<Device>> {
    let names = [
        "会議室アクセス点",
        "東側ギャラリー点",
        "屋上デッキ無線点",
        "編集室アクセス点",
    ];
    let mut devices = vec![device(
        "gw-harbor",
        "00:00:00:00:00:01",
        "Gateway",
        None,
        None,
    )];
    for (index, name) in names.iter().enumerate() {
        let mac = format!("00:00:00:00:02:{index:02}");
        let port = u32::try_from(index + 1).ok();
        devices.push(device(
            name,
            &mac,
            "AccessPoint",
            Some("00:00:00:00:00:01"),
            port,
        ));
    }
    devices
}

#[test]
fn wide_names_get_boxes_measured_in_cells() {
    let layout = layout(&wide_siblings(), 80.0);
    for node in &layout.nodes {
        let (start, cells) = node.label_span();
        assert!(
            start + f64::from(u32::try_from(cells).unwrap_or(u32::MAX)) < node.x + node.width,
            "{} spills out of its box",
            node.label
        );
    }
    for (index, left) in layout.nodes.iter().enumerate() {
        for right in layout.nodes.iter().skip(index + 1) {
            let same_band = left.y < right.bottom() && right.y < left.bottom();
            let apart = left.x + left.width < right.x || right.x + right.width < left.x;
            assert!(
                !same_band || apart,
                "{} and {} overlap",
                left.label,
                right.label
            );
        }
    }
}

#[test]
fn wide_names_survive_rendering_at_80_columns() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut screen = super::super::TopologyScreen::new();
    screen.devices = Arc::new(wide_siblings());
    let mut terminal = Terminal::new(TestBackend::new(80, 40)).expect("test terminal");
    terminal
        .draw(|frame| screen.render_screen(frame, frame.area()))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    // A wide character covers two cells and a terminal draws nothing in the
    // second one, so skip it to rebuild the text as it reads on screen.
    let text: Vec<String> = (0..buffer.area.height)
        .map(|y| {
            let mut row = String::new();
            let mut x = 0;
            while x < buffer.area.width {
                let symbol = buffer[(x, y)].symbol();
                row.push_str(symbol);
                x += if super::cell_width(symbol) == 2 { 2 } else { 1 };
            }
            row
        })
        .collect();
    for device in wide_siblings() {
        let name = device.name.clone().unwrap_or_default();
        assert!(
            text.iter().any(|row| row.contains(&name)),
            "{name} is not drawn whole:\n{}",
            text.join("\n")
        );
    }
}

#[test]
fn fit_cells_never_splits_a_wide_character() {
    assert_eq!(super::fit_cells("会議室", 5), "会議");
    assert_eq!(super::fit_cells("ap-roof", 4), "ap-r");
    assert_eq!(super::fit_cells("ap", 10), "ap");
}

/// The stem and bus a parent shares with all its children take one style.
/// A guessed orphan or an offline child must not repaint the line its
/// siblings use; their state shows on their own drop instead.
#[test]
fn shared_trunks_keep_one_style_per_parent() {
    use super::super::render::{MESH_DASH, trunk_styles};

    let mut devices = fleet();
    // Take ap-a offline: its drop turns red, the trunk under core must not.
    for device in &mut devices {
        if device.name.as_deref() == Some("ap-a") {
            let mut offline = Arc::unwrap_or_clone(Arc::clone(device));
            offline.state = unifly_api::DeviceState::Offline;
            *device = Arc::new(offline);
        }
    }
    let layout = layout(&devices, WIDE);
    let trunks = trunk_styles(&layout.nodes, &layout.edges);

    // gw has a wired switch and a guessed orphan: one solid trunk.
    let gw = trunks[&index(&layout, "gw")];
    assert_eq!(gw.1, None, "a mixed parent keeps a solid trunk");

    // core has an offline AP and an online switch: no error-red trunk.
    let core = trunks[&index(&layout, "core")];
    assert_ne!(core.0, crate::tui::theme::error());
    assert_eq!(core.1, None);

    // ap-a reaches its only child over mesh: the whole link is dashed.
    let ap_a = trunks[&index(&layout, "ap-a")];
    assert_eq!(ap_a.1, MESH_DASH);
}
