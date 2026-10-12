//! Tree layout for the topology canvas.
//!
//! Every device hangs under the device it actually uplinks through
//! (`uplink_device_mac`, falling back to `uplink_device_id`), with the
//! gateway at the root. A device whose uplink is unknown is attached to the
//! gateway with an [`LinkKind::Inferred`] edge rather than being guessed into
//! a switch.
//!
//! Coordinates are terminal cells: `x` grows right and `y` grows down, one
//! unit per column or row. Boxes are sized to their labels, and a parent
//! whose children would not fit the available width wraps them into rows,
//! so at the fitted zoom every label is drawn whole and none overlap.

mod place;
#[cfg(test)]
mod tests;

pub(super) use place::MARGIN;

use std::collections::HashMap;
use std::sync::Arc;

use unifly_api::{Device, DeviceState, DeviceType, EntityId, UplinkMedium};

/// Longest device name drawn in a box.
const LABEL_MAX: usize = 16;

/// One device box. `x`/`y` are the top-left corner in cells.
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

    pub(super) fn bottom(&self) -> f64 {
        self.y + self.height
    }

    /// Where the label starts, and how many cells it takes.
    pub(super) fn label_span(&self) -> (f64, usize) {
        (self.x + 1.0, cell_width(&self.label))
    }
}

/// Width of `text` in terminal cells. Wide characters (CJK, most emoji)
/// take two cells, so a character count would under-size boxes for them.
pub(super) fn cell_width(text: &str) -> usize {
    ratatui::text::Span::raw(text).width()
}

/// `text` cut to at most `max` terminal cells, never splitting a wide
/// character across the limit.
pub(super) fn fit_cells(text: &str, max: usize) -> String {
    let mut fitted = String::new();
    let mut used = 0;
    let mut buf = [0u8; 4];
    for ch in text.chars() {
        let width = cell_width(ch.encode_utf8(&mut buf));
        if used + width > max {
            break;
        }
        used += width;
        fitted.push(ch);
    }
    fitted
}

/// How a device reaches its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LinkKind {
    /// Wired uplink, or an uplink whose medium the controller did not report.
    Wired,
    /// Wireless uplink as reported by the controller (mesh).
    Wireless,
    /// No usable uplink was reported; drawn to the gateway as a best guess.
    Inferred,
}

/// A parent-to-child link, as indices into [`TopoLayout::nodes`]. It leaves
/// the parent's bottom edge at `stem_x`, runs along `bus_y`, and drops into
/// the child.
pub(super) struct TopoEdge {
    pub parent: usize,
    pub child: usize,
    pub kind: LinkKind,
    /// Port on the parent the child is plugged into, when reported.
    pub port: Option<u32>,
    pub stem_x: f64,
    pub bus_y: f64,
}

pub(super) struct TopoLayout {
    pub nodes: Vec<TopoNode>,
    pub edges: Vec<TopoEdge>,
    /// `[x_min, x_max, y_min, y_max]` around every node.
    pub bounds: [f64; 4],
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

/// Lay `devices` out as an uplink tree that fits `columns` cells wide when
/// it can.
pub(super) fn layout(devices: &[Arc<Device>], columns: f64) -> TopoLayout {
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

    let mut nodes: Vec<TopoNode> = devices
        .iter()
        .map(|device| {
            let label: String = display_name(device).chars().take(LABEL_MAX).collect();
            let ip = device.ip.map(|ip| ip.to_string()).unwrap_or_default();
            TopoNode {
                width: place::box_width(&label, &ip),
                height: place::NODE_HEIGHT,
                label,
                ip,
                device_type: device.device_type,
                state: device.state,
                traffic_bytes_per_sec: traffic(device),
                x: 0.0,
                y: 0.0,
            }
        })
        .collect();

    let mut placer = place::Placer::new(&mut nodes, &children, columns);
    let routes = placer.place_forest(&roots);

    let edges = parents
        .iter()
        .enumerate()
        .filter_map(|(child, parent)| {
            let (parent, kind) = (*parent)?;
            let (stem_x, bus_y) = routes.get(&child).copied()?;
            Some(TopoEdge {
                parent,
                child,
                kind,
                port: (kind == LinkKind::Wired)
                    .then_some(devices[child].uplink_port_idx)
                    .flatten(),
                stem_x,
                bus_y,
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
            // Trust the medium the controller reports. When it is unknown,
            // draw a plain link rather than guessing at mesh.
            let kind = if device.uplink_medium == Some(UplinkMedium::Wireless) {
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
        return [0.0, 80.0, 0.0, 20.0];
    }
    nodes.iter().fold(
        [f64::MAX, f64::MIN, f64::MAX, f64::MIN],
        |[x_min, x_max, y_min, y_max], node| {
            [
                x_min.min(node.x),
                x_max.max(node.x + node.width),
                y_min.min(node.y),
                y_max.max(node.bottom()),
            ]
        },
    )
}
