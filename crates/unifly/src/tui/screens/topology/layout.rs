//! Tree layout for the topology canvas.
//!
//! Every device hangs under the device it actually uplinks through
//! (`uplink_device_mac`, falling back to `uplink_device_id`), with the
//! gateway at the root. A device whose uplink is unknown is attached to the
//! gateway with an [`LinkKind::Inferred`] edge rather than being guessed into
//! a switch. Canvas coordinates grow upward, so deeper levels sit at lower
//! `y` values.

use std::collections::HashMap;
use std::sync::Arc;

use unifly_api::{Device, DeviceState, DeviceType, EntityId};

/// Vertical distance from one tree level to the next, in canvas units.
pub(super) const LEVEL_GAP: f64 = 18.0;
/// Horizontal gap between neighboring subtrees.
const SIBLING_GAP: f64 = 3.0;

/// One device box on the canvas. `x`/`y` are the bottom-left corner.
pub(super) struct TopoNode {
    pub label: String,
    pub ip: String,
    pub device_type: DeviceType,
    pub state: DeviceState,
    pub traffic_bytes_per_sec: u64,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl TopoNode {
    pub(super) fn center_x(&self) -> f64 {
        self.x + self.width / 2.0
    }

    pub(super) fn top(&self) -> f64 {
        self.y + self.height
    }
}

/// How a device reaches its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LinkKind {
    /// Uplink reported by the controller over a cable.
    Wired,
    /// Wireless mesh: an AP uplinked to another AP with no switch port.
    Wireless,
    /// No usable uplink was reported; drawn to the gateway as a best guess.
    Inferred,
}

/// A parent-to-child link, as indices into [`TopoLayout::nodes`].
pub(super) struct TopoEdge {
    pub parent: usize,
    pub child: usize,
    pub kind: LinkKind,
    /// Port on the parent the child is plugged into, when reported.
    pub port: Option<u32>,
}

pub(super) struct TopoLayout {
    pub nodes: Vec<TopoNode>,
    pub edges: Vec<TopoEdge>,
    /// `[x_min, x_max, y_min, y_max]` around every node.
    pub bounds: [f64; 4],
}

fn node_size(device_type: DeviceType) -> (f64, f64) {
    match device_type {
        DeviceType::Gateway => (16.0, 8.0),
        DeviceType::Switch => (14.0, 7.0),
        _ => (12.0, 7.0),
    }
}

fn type_rank(device_type: DeviceType) -> u8 {
    match device_type {
        DeviceType::Gateway => 0,
        DeviceType::Switch => 1,
        DeviceType::AccessPoint => 2,
        _ => 3,
    }
}

fn display_name(device: &Device) -> String {
    device
        .name
        .clone()
        .unwrap_or_else(|| device.mac.to_string())
}

/// Lay `devices` out as an uplink tree.
pub(super) fn layout(devices: &[Arc<Device>]) -> TopoLayout {
    let parents = resolve_parents(devices);

    let mut children: Vec<Vec<usize>> = vec![Vec::new(); devices.len()];
    let mut roots = Vec::new();
    for (index, parent) in parents.iter().enumerate() {
        match parent {
            Some((parent, _)) => children[*parent].push(index),
            None => roots.push(index),
        }
    }
    let order = |left: &usize, right: &usize| {
        let (left, right) = (&devices[*left], &devices[*right]);
        type_rank(left.device_type)
            .cmp(&type_rank(right.device_type))
            .then_with(|| display_name(left).cmp(&display_name(right)))
    };
    roots.sort_by(order);
    for list in &mut children {
        list.sort_by(order);
    }

    let mut placer = Placer {
        devices,
        children: &children,
        cursor: 0.0,
        centers: vec![0.0; devices.len()],
        depths: vec![0; devices.len()],
    };
    for root in &roots {
        placer.place(*root, 0);
    }

    let nodes: Vec<TopoNode> = devices
        .iter()
        .enumerate()
        .map(|(index, device)| {
            let (width, height) = node_size(device.device_type);
            let depth = placer.depths[index];
            TopoNode {
                label: display_name(device),
                ip: device.ip.map(|ip| ip.to_string()).unwrap_or_default(),
                device_type: device.device_type,
                state: device.state,
                traffic_bytes_per_sec: traffic(device),
                x: placer.centers[index] - width / 2.0,
                y: -level_top(depth) - height,
                width,
                height,
            }
        })
        .collect();

    let edges = parents
        .iter()
        .enumerate()
        .filter_map(|(child, parent)| {
            let (parent, kind) = (*parent)?;
            Some(TopoEdge {
                parent,
                child,
                kind,
                port: (kind == LinkKind::Wired)
                    .then_some(devices[child].uplink_port_idx)
                    .flatten(),
            })
        })
        .collect();

    let bounds = bounds(&nodes);
    TopoLayout {
        nodes,
        edges,
        bounds,
    }
}

/// Distance from the root row's top edge down to `depth`'s top edge.
#[allow(clippy::cast_precision_loss, clippy::as_conversions)]
fn level_top(depth: usize) -> f64 {
    depth as f64 * LEVEL_GAP
}

/// Each device's parent and link kind, with cycles broken and orphans
/// attached to the gateway.
fn resolve_parents(devices: &[Arc<Device>]) -> Vec<Option<(usize, LinkKind)>> {
    let by_mac: HashMap<&str, usize> = devices
        .iter()
        .enumerate()
        .map(|(index, device)| (device.mac.as_str(), index))
        .collect();
    let by_id: HashMap<&EntityId, usize> = devices
        .iter()
        .enumerate()
        .map(|(index, device)| (&device.id, index))
        .collect();

    let mut parents: Vec<Option<(usize, LinkKind)>> = devices
        .iter()
        .enumerate()
        .map(|(index, device)| {
            let parent = device
                .uplink_device_mac
                .as_ref()
                .and_then(|mac| by_mac.get(mac.as_str()))
                .or_else(|| {
                    device
                        .uplink_device_id
                        .as_ref()
                        .and_then(|id| by_id.get(id))
                })
                .copied()
                .filter(|parent| *parent != index)?;
            let mesh = device.device_type == DeviceType::AccessPoint
                && devices[parent].device_type == DeviceType::AccessPoint
                && device.uplink_port_idx.is_none();
            let kind = if mesh {
                LinkKind::Wireless
            } else {
                LinkKind::Wired
            };
            Some((parent, kind))
        })
        .collect();

    // Bad data can describe a loop; cut it where the walk first repeats.
    for start in 0..parents.len() {
        let mut seen = vec![false; parents.len()];
        seen[start] = true;
        let mut cursor = start;
        while let Some((parent, _)) = parents[cursor] {
            if seen[parent] {
                parents[cursor] = None;
                break;
            }
            seen[parent] = true;
            cursor = parent;
        }
    }

    // Orphans hang off the first root gateway. Linking one root to another
    // cannot form a cycle.
    let gateway = devices.iter().enumerate().position(|(index, device)| {
        device.device_type == DeviceType::Gateway && parents[index].is_none()
    });
    if let Some(gateway) = gateway {
        for (index, device) in devices.iter().enumerate() {
            if parents[index].is_none()
                && index != gateway
                && device.device_type != DeviceType::Gateway
            {
                parents[index] = Some((gateway, LinkKind::Inferred));
            }
        }
    }
    parents
}

/// Tidy tree placement: leaves take consecutive slots left to right and
/// each parent centers over its children.
struct Placer<'a> {
    devices: &'a [Arc<Device>],
    children: &'a [Vec<usize>],
    cursor: f64,
    centers: Vec<f64>,
    depths: Vec<usize>,
}

impl Placer<'_> {
    fn place(&mut self, index: usize, depth: usize) -> f64 {
        let (width, _) = node_size(self.devices[index].device_type);
        self.depths[index] = depth;
        let start = self.cursor;
        let kids = &self.children[index];

        let center = if kids.is_empty() {
            start + width / 2.0
        } else {
            let mut first = None;
            let mut last = 0.0;
            for child in kids {
                let child_center = self.place(*child, depth + 1);
                first.get_or_insert(child_center);
                last = child_center;
            }
            let span_center = f64::midpoint(first.unwrap_or(last), last);
            // A parent wider than its subtree must not poke into the
            // previous sibling's space.
            span_center.max(start + width / 2.0)
        };

        self.centers[index] = center;
        self.cursor = self.cursor.max(center + width / 2.0 + SIBLING_GAP);
        center
    }
}

fn traffic(device: &Device) -> u64 {
    device
        .stats
        .uplink_bandwidth
        .as_ref()
        .map_or(0, |bandwidth| {
            bandwidth
                .tx_bytes_per_sec
                .saturating_add(bandwidth.rx_bytes_per_sec)
        })
}

fn bounds(nodes: &[TopoNode]) -> [f64; 4] {
    if nodes.is_empty() {
        return [0.0, 100.0, -50.0, 0.0];
    }
    nodes.iter().fold(
        [f64::MAX, f64::MIN, f64::MAX, f64::MIN],
        |[x_min, x_max, y_min, y_max], node| {
            [
                x_min.min(node.x),
                x_max.max(node.x + node.width),
                y_min.min(node.y),
                y_max.max(node.top()),
            ]
        },
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use unifly_api::{Device, DeviceType};

    use super::{LinkKind, TopoLayout, layout};

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
            device(
                "ap-mesh",
                "00:00:00:00:00:06",
                "AccessPoint",
                Some("00:00:00:00:00:04"),
                None,
            ),
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
        let layout = layout(&fleet());
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
        let layout = layout(&fleet());
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
        let layout = layout(&fleet());
        for edge in &layout.edges {
            let (parent, child) = (&layout.nodes[edge.parent], &layout.nodes[edge.child]);
            assert!(
                child.top() < parent.y,
                "{} should sit below {}",
                child.label,
                parent.label
            );
        }
        for (left_index, left) in layout.nodes.iter().enumerate() {
            for right in layout.nodes.iter().skip(left_index + 1) {
                if (left.top() - right.top()).abs() < f64::EPSILON {
                    let apart = left.x + left.width <= right.x || right.x + right.width <= left.x;
                    assert!(apart, "{} overlaps {}", left.label, right.label);
                }
            }
        }
        let [x_min, x_max, y_min, y_max] = layout.bounds;
        assert!(x_min < x_max && y_min < y_max);
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
        let layout = layout(&devices);
        assert_eq!(layout.nodes.len(), 3);
        assert_eq!(layout.edges.len(), 2, "the loop is cut once and reattached");
    }

    #[test]
    fn empty_network_has_sane_bounds() {
        let layout = layout(&[]);
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
        let layout = layout(&devices);
        let kinds: Vec<LinkKind> = layout.edges.iter().map(|edge| edge.kind).collect();
        assert_eq!(kinds, [LinkKind::Inferred, LinkKind::Inferred]);
        let gateway = &layout.nodes[index(&layout, "gw")];
        assert_eq!(gateway.device_type, DeviceType::Gateway);
        let sw = &layout.nodes[index(&layout, "sw")];
        let ap = &layout.nodes[index(&layout, "ap")];
        assert!(sw.top() < gateway.y && (sw.top() - ap.top()).abs() < f64::EPSILON);
    }
}
