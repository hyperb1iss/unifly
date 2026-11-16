use std::cell::{Cell, RefCell};
use std::sync::Arc;

use ratatui::layout::Rect;

use super::layout::{MARGIN, TopoLayout, Wrap, fitted, layout_within};
use super::{Camera, TopologyScreen};
use crate::tui::action::Action;
use crate::tui::screens::link::Link;

/// Closest and furthest the camera zooms.
pub(super) const ZOOM_RANGE: (f64, f64) = (0.2, 5.0);

/// The tree and the camera resolved against one canvas.
pub(super) struct Scene {
    pub tree: TopoLayout,
    /// How the tree was wrapped.
    pub wrap: Wrap,
    pub canvas: Rect,
    /// Layout point at the center of the canvas.
    pub cx: f64,
    pub cy: f64,
    pub zoom: f64,
}

impl Scene {
    /// Layout units the canvas spans across and down. A canvas of `n`
    /// cells shows `n - 1` units at 100%, so both edges land on a cell.
    pub(super) fn spans(&self) -> (f64, f64) {
        spans(self.canvas, self.zoom)
    }

    /// Layout point at the canvas's top-left corner.
    pub(super) fn origin(&self) -> (f64, f64) {
        let (x_span, y_span) = self.spans();
        (self.cx - x_span / 2.0, self.cy - y_span / 2.0)
    }

    /// Whether the box of node `index` is wholly on the canvas.
    pub(super) fn shows(&self, index: usize) -> bool {
        let Some(node) = self.tree.nodes.get(index) else {
            return false;
        };
        let (left, top) = self.origin();
        let (x_span, y_span) = self.spans();
        node.x >= left
            && node.x + node.width <= left + x_span
            && node.y >= top
            && node.bottom() <= top + y_span
    }

    /// Node indices in reading order: top to bottom, then left to right.
    pub(super) fn reading_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.tree.nodes.len()).collect();
        order.sort_by(|left, right| {
            let (left, right) = (&self.tree.nodes[*left], &self.tree.nodes[*right]);
            left.y.total_cmp(&right.y).then(left.x.total_cmp(&right.x))
        });
        order
    }
}

fn spans(canvas: Rect, zoom: f64) -> (f64, f64) {
    (
        (f64::from(canvas.width.max(2)) - 1.0) / zoom,
        (f64::from(canvas.height.max(2)) - 1.0) / zoom,
    )
}

impl TopologyScreen {
    pub fn new() -> Self {
        Self {
            focused: false,
            devices: Arc::new(Vec::new()),
            camera: Camera::Fit,
            selected: None,
            link: Link::default(),
            canvas: Cell::new(Rect::new(0, 0, 80, 20)),
            hits: RefCell::default(),
            press: None,
        }
    }

    /// The tree laid out for `canvas`, and the camera's place over it.
    pub(super) fn scene(&self, canvas: Rect) -> Scene {
        self.scene_through(canvas, self.camera)
    }

    /// The scene `camera` would show on `canvas`.
    fn scene_through(&self, canvas: Rect, camera: Camera) -> Scene {
        let columns = f64::from(canvas.width.max(2));
        let rows = f64::from(canvas.height.max(2));
        match camera {
            Camera::Fit => {
                let (tree, zoom, wrap) = fitted(&self.devices, columns, rows);
                let [x_lo, x_hi, y_lo, y_hi] = tree.bounds;
                let (x_span, y_span) = spans(canvas, zoom);
                // Along an axis the tree fits it is centered. Along one it
                // overflows, the view starts at the tree's near edge, so
                // the root is on screen and the rest lies one way.
                let place = |lo: f64, hi: f64, span: f64| {
                    if hi - lo + 2.0 * MARGIN > span {
                        lo - MARGIN + span / 2.0
                    } else {
                        f64::midpoint(lo, hi)
                    }
                };
                Scene {
                    cx: place(x_lo, x_hi, x_span),
                    cy: place(y_lo, y_hi, y_span),
                    tree,
                    wrap,
                    canvas,
                    zoom,
                }
            }
            Camera::Free { cx, cy, zoom, wrap } => Scene {
                tree: layout_within(&self.devices, wrap),
                wrap,
                canvas,
                cx,
                cy,
                zoom,
            },
        }
    }

    /// The scene as the last frame drew it.
    pub(super) fn last_scene(&self) -> Scene {
        self.scene(self.canvas.get())
    }

    /// Point the camera at `(cx, cy)`, held to the tree so the map cannot
    /// be panned away: along an axis the tree fits, all of it stays on the
    /// canvas, and along one it does not, the canvas stays inside it.
    pub(super) fn aim(&mut self, scene: &Scene, cx: f64, cy: f64, zoom: f64) {
        let [x_lo, x_hi, y_lo, y_hi] = scene.tree.bounds;
        let (x_span, y_span) = spans(scene.canvas, zoom);
        let hold = |center: f64, lo: f64, hi: f64, span: f64| {
            let (first, last) = (lo - MARGIN + span / 2.0, hi + MARGIN - span / 2.0);
            center.clamp(first.min(last), first.max(last))
        };
        self.camera = Camera::Free {
            cx: hold(cx, x_lo, x_hi, x_span),
            cy: hold(cy, y_lo, y_hi, y_span),
            zoom,
            wrap: scene.wrap,
        };
    }

    /// The camera at 100% on the top of the tree, wrapped to the canvas
    /// so it only runs off the bottom.
    pub(super) fn reset(&mut self) {
        let canvas = self.canvas.get();
        let scene = self.scene_through(
            canvas,
            Camera::Free {
                cx: 0.0,
                cy: 0.0,
                zoom: 1.0,
                wrap: Wrap::full_size(f64::from(canvas.width.max(2))),
            },
        );
        let [x_lo, x_hi, y_lo, _] = scene.tree.bounds;
        self.aim(&scene, f64::midpoint(x_lo, x_hi), y_lo, 1.0);
    }

    /// Index of the selected device's node, when it is still in the tree.
    pub(super) fn selected_index(&self) -> Option<usize> {
        let selected = self.selected.as_ref()?;
        self.devices
            .iter()
            .position(|device| &device.id == selected)
    }

    pub(super) fn apply_action(&mut self, action: &Action) {
        match action {
            Action::DevicesUpdated(devices) => {
                self.devices = Arc::clone(devices);
                if self.selected_index().is_none() {
                    self.selected = None;
                }
            }
            Action::Connecting
            | Action::Connected
            | Action::Reconnecting { .. }
            | Action::Disconnected(_) => self.link.follow(action),
            Action::ControllerChanged => {
                *self = Self {
                    focused: self.focused,
                    canvas: self.canvas.clone(),
                    ..Self::new()
                };
            }
            _ => {}
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use unifly_api::Device;

    use super::TopologyScreen;
    use crate::tui::action::Action;

    /// A device named `name`, uplinked to the device with MAC `uplink`.
    pub(in crate::tui::screens::topology) fn device(
        name: &str,
        mac: &str,
        device_type: &str,
        uplink: Option<&str>,
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
                "has_switching": false,
                "has_access_point": false,
                "stats": {},
                "client_count": null,
                "origin": null
            }))
            .expect("device fixture should deserialize"),
        )
    }

    /// A gateway, a switch under it, and `aps` access points under that.
    pub(in crate::tui::screens::topology) fn fleet(aps: usize) -> Arc<Vec<Arc<Device>>> {
        let mut devices = vec![
            device("gw", "00:00:00:00:00:01", "Gateway", None),
            device(
                "sw",
                "00:00:00:00:00:02",
                "Switch",
                Some("00:00:00:00:00:01"),
            ),
        ];
        for index in 0..aps {
            devices.push(device(
                &format!("ap-{index:02}"),
                &format!("00:00:00:00:01:{index:02x}"),
                "AccessPoint",
                Some("00:00:00:00:00:02"),
            ));
        }
        Arc::new(devices)
    }

    pub(in crate::tui::screens::topology) fn screen_with(aps: usize) -> TopologyScreen {
        let mut screen = TopologyScreen::new();
        screen.apply_action(&Action::Connected);
        screen.apply_action(&Action::DevicesUpdated(fleet(aps)));
        screen
    }

    #[test]
    fn devices_update_replaces_visible_topology_devices() {
        let screen = screen_with(1);

        assert_eq!(screen.devices.len(), 3);
        assert_eq!(screen.devices[0].name.as_deref(), Some("gw"));
        assert_eq!(screen.devices[2].name.as_deref(), Some("ap-00"));
    }

    #[test]
    fn a_selected_device_that_leaves_the_site_is_deselected() {
        let mut screen = screen_with(2);
        screen.selected = Some(screen.devices[3].id.clone());

        screen.apply_action(&Action::DevicesUpdated(fleet(1)));

        assert!(screen.selected.is_none());
    }

    #[test]
    fn switching_controllers_drops_the_tree_and_the_camera() {
        let mut screen = screen_with(4);
        screen.selected = Some(screen.devices[0].id.clone());
        screen.reset();

        screen.apply_action(&Action::ControllerChanged);

        assert!(screen.devices.is_empty() && screen.selected.is_none());
        assert_eq!(screen.camera, super::Camera::Fit);
    }
}
