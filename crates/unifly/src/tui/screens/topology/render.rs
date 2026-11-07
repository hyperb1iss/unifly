use std::collections::HashMap;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui::widgets::canvas::{Canvas, Context, Line as CanvasLine, Rectangle};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use unifly_api::DeviceType;

use super::TopologyScreen;
use super::layout::{LinkKind, MARGIN, TopoEdge, TopoNode, cell_width, fit_cells, layout};
use crate::tui::theme;
use crate::tui::widgets::click::{self, Hint};

impl TopologyScreen {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    pub(super) fn render_screen(&self, frame: &mut Frame, area: Rect) {
        self.hits.borrow_mut().clear();
        let zoom_pct = (self.zoom * 100.0) as u32;
        let title = format!(
            " Topology  ·  Zoom: {zoom_pct}%  Pan: {:.0},{:.0} ",
            self.pan_x, self.pan_y
        );
        let block = Block::default()
            .title(title)
            .title_style(theme::title_style())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(if self.focused {
                theme::border_focused()
            } else {
                theme::border_default()
            });

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let content_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: inner.height.saturating_sub(1),
        };
        let hints_area = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        };

        // The layout works in cells (y down), and at 100% zoom one unit is
        // always one cell, so every label lands whole inside its box (a
        // canvas maps a span of n - 1 units onto n cells). A tree that fits
        // is centered; one that does not is anchored at its top-left corner
        // and panned rather than squeezed until labels collide.
        let columns = f64::from(content_area.width.max(2));
        let rows = f64::from(content_area.height.max(2));
        let tree = layout(&self.devices, columns);
        let [x_lo, x_hi, y_lo, y_hi] = tree.bounds;
        let x_span = (columns - 1.0) / self.zoom;
        let y_span = (rows - 1.0) / self.zoom;
        let anchor = |lo: f64, hi: f64, span: f64| {
            if hi - lo + 2.0 * MARGIN <= span {
                f64::midpoint(lo, hi) - span / 2.0
            } else {
                lo - MARGIN
            }
        };
        let left = (anchor(x_lo, x_hi, x_span) + self.pan_x).floor();
        let top = (anchor(y_lo, y_hi, y_span) - self.pan_y).floor();

        let viewport = Viewport {
            left,
            top,
            x_span,
            y_span,
            width: content_area.width,
            height: content_area.height,
        };
        let canvas = Canvas::default()
            .x_bounds([left, left + x_span])
            .y_bounds([-(top + y_span), -top])
            .paint(|ctx: &mut Context<'_>| {
                let trunks = trunk_styles(&tree.nodes, &tree.edges);
                for edge in &tree.edges {
                    let trunk = trunks
                        .get(&edge.parent)
                        .copied()
                        .unwrap_or_else(|| edge_style(&tree.nodes, edge));
                    draw_edge(ctx, &tree.nodes, edge, trunk);
                }
                for node in &tree.nodes {
                    draw_node(ctx, node);
                }
            });

        frame.render_widget(canvas, content_area);
        for edge in &tree.edges {
            draw_edge_label(
                frame.buffer_mut(),
                content_area,
                &tree.nodes,
                edge,
                &viewport,
            );
        }
        for node in &tree.nodes {
            draw_node_labels(frame.buffer_mut(), content_area, node, &viewport);
        }

        let mut bar = click::key_hints(&[
            Hint::info("←→↑↓", "pan"),
            Hint::button("+", "zoom in", click::char_key('+')),
            Hint::button("-", "zoom out", click::char_key('-')),
            Hint::button("f", "fit", click::char_key('f')),
            Hint::button("r", "reset", click::char_key('r')),
        ]);
        for (symbol, color, label) in [
            ("─ ", theme::accent_secondary(), "wired"),
            ("╌ ", theme::info(), "mesh"),
            ("┄ ", theme::border_unfocused(), "uplink unknown"),
        ] {
            bar.push_gap(Span::raw("  "));
            bar.push_gap(Span::styled(symbol, Style::default().fg(color)));
            bar.push_gap(Span::styled(label, theme::key_hint()));
        }
        let line = bar.draw(hints_area, &mut self.hits.borrow_mut());
        frame.render_widget(Paragraph::new(line), hints_area);
    }
}

fn node_color(node: &TopoNode) -> Color {
    if !node.state.is_online() {
        return theme::error();
    }
    match node.device_type {
        DeviceType::Gateway => theme::accent_tertiary(),
        DeviceType::Switch => theme::accent_secondary(),
        DeviceType::AccessPoint => theme::accent_primary(),
        _ => theme::text_secondary(),
    }
}

/// Canvas braille shapes round 2x4-dot coordinates. Project borders into
/// terminal cells once, then place text directly inside those borders.
struct Viewport {
    left: f64,
    top: f64,
    x_span: f64,
    y_span: f64,
    width: u16,
    height: u16,
}

impl Viewport {
    fn border_column(&self, x: f64) -> f64 {
        (((x - self.left) * (2.0 * f64::from(self.width) - 1.0) / self.x_span).round() / 2.0)
            .floor()
    }

    fn border_row(&self, y: f64) -> f64 {
        (((y - self.top) * (4.0 * f64::from(self.height) - 1.0) / self.y_span).round() / 4.0)
            .floor()
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    fn print(&self, buffer: &mut Buffer, area: Rect, column: f64, row: f64, text: &Span<'_>) {
        if column < 0.0
            || row < 0.0
            || column >= f64::from(self.width)
            || row >= f64::from(self.height)
        {
            return;
        }
        let column = column as u16;
        buffer.set_span(
            area.x + column,
            area.y + row as u16,
            text,
            self.width - column,
        );
    }
}

/// Layout rows grow down; canvas `y` grows up.
fn up(y: f64) -> f64 {
    -y
}

fn draw_node(ctx: &mut Context<'_>, node: &TopoNode) {
    let color = node_color(node);
    ctx.draw(&Rectangle {
        x: node.x,
        y: up(node.bottom()),
        width: node.width,
        height: node.height,
        color,
    });
}

fn draw_node_labels(buffer: &mut Buffer, area: Rect, node: &TopoNode, viewport: &Viewport) {
    let color = node_color(node);
    let column = viewport.border_column(node.x) + 1.0;
    let row = viewport.border_row(node.y) + 1.0;
    let right = viewport
        .border_column(node.x + node.width)
        .min(f64::from(viewport.width));
    let bottom = viewport.border_row(node.bottom());
    if column >= right || row >= bottom {
        return;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    let inner = (right - column) as usize;
    viewport.print(
        buffer,
        area,
        column,
        row,
        &Span::styled(fit_cells(&node.label, inner), Style::default().fg(color)),
    );
    if !node.ip.is_empty() && cell_width(&node.ip) <= inner && row + 1.0 < bottom {
        viewport.print(
            buffer,
            area,
            column,
            row + 1.0,
            &Span::styled(
                node.ip.clone(),
                Style::default().fg(theme::text_secondary()),
            ),
        );
    }
}

/// Org-chart connector: a stub down from the parent, a horizontal run at
/// mid-gap, and a drop into the child. Wired drops thicken with traffic;
/// mesh links are dashed, and guessed links are dotted and dim.
pub(super) type LinkStyle = (Color, Option<(f64, f64)>);

pub(super) const MESH_DASH: Option<(f64, f64)> = Some((1.6, 1.0));
const GUESS_DOTS: Option<(f64, f64)> = Some((0.5, 0.9));

/// How one child's own drop is drawn: its state and link kind.
fn edge_style(nodes: &[TopoNode], edge: &TopoEdge) -> LinkStyle {
    let online = nodes
        .get(edge.child)
        .is_some_and(|child| child.state.is_online());
    match edge.kind {
        LinkKind::Wired if online => (
            nodes
                .get(edge.child)
                .map_or_else(theme::text_secondary, node_color),
            None,
        ),
        LinkKind::Wired => (theme::error(), None),
        LinkKind::Wireless => (theme::info(), MESH_DASH),
        LinkKind::Inferred => (theme::border_unfocused(), GUESS_DOTS),
    }
}

/// One style per parent for the stem and bus its children share. Every
/// child redraws its stretch of that trunk, so styling it per child would
/// let whichever child is drawn last (an offline device, a guessed orphan)
/// repaint the line its siblings use. The trunk is dashed when every child
/// is a mesh link, dim and dotted when every child is a guess, and
/// otherwise solid in the parent's colour; each child's state stays on its
/// own drop.
pub(super) fn trunk_styles(nodes: &[TopoNode], edges: &[TopoEdge]) -> HashMap<usize, LinkStyle> {
    let mut kinds: HashMap<usize, Vec<LinkKind>> = HashMap::new();
    for edge in edges {
        kinds.entry(edge.parent).or_default().push(edge.kind);
    }
    kinds
        .into_iter()
        .map(|(parent, kinds)| {
            let style = if kinds.iter().all(|kind| *kind == LinkKind::Wireless) {
                (theme::info(), MESH_DASH)
            } else if kinds.iter().all(|kind| *kind == LinkKind::Inferred) {
                (theme::border_unfocused(), GUESS_DOTS)
            } else {
                (
                    nodes
                        .get(parent)
                        .map_or_else(theme::text_secondary, node_color),
                    None,
                )
            };
            (parent, style)
        })
        .collect()
}

fn draw_edge(ctx: &mut Context<'_>, nodes: &[TopoNode], edge: &TopoEdge, trunk: LinkStyle) {
    let (Some(parent), Some(child)) = (nodes.get(edge.parent), nodes.get(edge.child)) else {
        return;
    };
    let (stem, cx) = (edge.stem_x, child.center_x());
    let parent_bottom = up(parent.bottom());
    let child_top = up(child.y);
    let bus = up(edge.bus_y);

    let (trunk_color, trunk_dash) = trunk;
    let segments = [
        ((stem, parent_bottom), (stem, bus)),
        ((stem, bus), (cx, bus)),
    ];
    for (from, to) in segments {
        draw_segment(ctx, from, to, trunk_color, trunk_dash);
    }

    let (color, dash) = edge_style(nodes, edge);

    let strands: &[f64] = match (edge.kind, edge_weight(child.traffic_bytes_per_sec)) {
        (LinkKind::Wired, 2) => &[-0.22, 0.22],
        (LinkKind::Wired, 3) => &[-0.38, 0.0, 0.38],
        _ => &[0.0],
    };
    for offset in strands {
        draw_segment(
            ctx,
            (cx + offset, bus),
            (cx + offset, child_top),
            color,
            dash,
        );
    }
}

fn draw_edge_label(
    buffer: &mut Buffer,
    area: Rect,
    nodes: &[TopoNode],
    edge: &TopoEdge,
    viewport: &Viewport,
) {
    let Some(child) = nodes.get(edge.child) else {
        return;
    };
    let label = match (edge.kind, edge.port) {
        (LinkKind::Wireless, _) => "mesh".to_owned(),
        (LinkKind::Wired, Some(port)) => port.to_string(),
        _ => return,
    };
    let column = viewport.border_column(child.center_x()) + 1.0;
    let row = viewport.border_row(edge.bus_y) + 1.0;
    let end = column + f64::from(u16::try_from(cell_width(&label)).unwrap_or(u16::MAX));
    if row >= viewport.border_row(child.y) || end > f64::from(viewport.width) {
        return;
    }
    // A neighboring branch may occupy the same projected cells after zooming.
    if nodes.iter().any(|node| {
        row >= viewport.border_row(node.y)
            && row <= viewport.border_row(node.bottom())
            && column <= viewport.border_column(node.x + node.width)
            && end > viewport.border_column(node.x)
    }) {
        return;
    }
    viewport.print(
        buffer,
        area,
        column,
        row,
        &Span::styled(label, Style::default().fg(theme::text_muted())),
    );
}

/// Draw a straight segment, optionally dashed as `(on, off)` lengths.
fn draw_segment(
    ctx: &mut Context<'_>,
    (x1, y1): (f64, f64),
    (x2, y2): (f64, f64),
    color: Color,
    dash: Option<(f64, f64)>,
) {
    let Some((on, off)) = dash else {
        ctx.draw(&CanvasLine {
            x1,
            y1,
            x2,
            y2,
            color,
        });
        return;
    };
    let length = (x2 - x1).hypot(y2 - y1);
    if length <= f64::EPSILON {
        return;
    }
    let (ux, uy) = ((x2 - x1) / length, (y2 - y1) / length);
    let mut start = 0.0;
    while start < length {
        let end = (start + on).min(length);
        ctx.draw(&CanvasLine {
            x1: x1 + ux * start,
            y1: y1 + uy * start,
            x2: x1 + ux * end,
            y2: y1 + uy * end,
            color,
        });
        start = end + off;
    }
}

fn edge_weight(traffic_bytes_per_sec: u64) -> u8 {
    match traffic_bytes_per_sec {
        0..=125_000 => 1,
        125_001..=12_500_000 => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::edge_weight;

    #[test]
    fn zoomed_labels_stay_inside_their_box_and_use_separate_rows() {
        use super::{TopoNode, Viewport, draw_node, draw_node_labels};
        use ratatui::{Terminal, backend::TestBackend, widgets::canvas::Canvas};
        let mut node = TopoNode {
            label: "ABCDEFGHIJKLMNOP".into(),
            ip: "10.10.10.10".into(),
            device_type: unifly_api::DeviceType::Gateway,
            state: unifly_api::DeviceState::Online,
            traffic_bytes_per_sec: 0,
            x: 10.0,
            y: 8.0,
            width: 18.0,
            height: 3.0,
        };
        for (zoom, y) in [
            (0.5, 8.0),
            (1.0 / 1.2 / 1.2, 24.0),
            (0.75, 8.0),
            (1.0, 8.0),
            (2.0, 8.0),
        ] {
            node.y = y;
            let mut terminal = Terminal::new(TestBackend::new(80, 30)).expect("terminal");
            let viewport = Viewport {
                left: 0.0,
                top: 0.0,
                x_span: 79.0 / zoom,
                y_span: 29.0 / zoom,
                width: 80,
                height: 30,
            };
            let border = TopoNode {
                label: String::new(),
                ip: String::new(),
                device_type: node.device_type,
                state: node.state,
                traffic_bytes_per_sec: 0,
                x: node.x,
                y: node.y,
                width: node.width,
                height: node.height,
            };
            terminal
                .draw(|frame| {
                    frame.render_widget(
                        Canvas::default()
                            .x_bounds([0.0, 79.0 / zoom])
                            .y_bounds([-29.0 / zoom, 0.0])
                            .paint(|context| draw_node(context, &border)),
                        frame.area(),
                    );
                })
                .expect("border render");
            let border_cells = terminal.backend().buffer().clone();
            terminal
                .draw(|frame| {
                    frame.render_widget(
                        Canvas::default()
                            .x_bounds([0.0, 79.0 / zoom])
                            .y_bounds([-29.0 / zoom, 0.0])
                            .paint(|context| draw_node(context, &node)),
                        frame.area(),
                    );
                    let area = frame.area();
                    draw_node_labels(frame.buffer_mut(), area, &node, &viewport);
                })
                .expect("render");
            let buffer = terminal.backend().buffer();
            let mut label_rows = std::collections::BTreeSet::new();
            let mut ip_rows = std::collections::BTreeSet::new();
            for y in 0..30 {
                for x in 0..80 {
                    let symbol = buffer[(x, y)].symbol();
                    if symbol
                        .chars()
                        .any(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                    {
                        assert_eq!(
                            border_cells[(x, y)].symbol(),
                            " ",
                            "text overwrote border at {x},{y}, zoom {zoom}"
                        );
                        if symbol.chars().any(|c| c.is_ascii_uppercase()) {
                            label_rows.insert(y);
                        }
                        if symbol.chars().any(|c| c.is_ascii_digit()) {
                            ip_rows.insert(y);
                        }
                    }
                }
            }
            if zoom >= 0.75 {
                assert_eq!(label_rows.len(), 1);
            }
            assert_eq!(ip_rows.len(), usize::from(zoom >= 1.0));
            assert!(label_rows.is_disjoint(&ip_rows));
        }
    }

    #[test]
    fn clipped_nodes_keep_names_in_last_visible_row_and_column() {
        use super::{TopoNode, Viewport, draw_node, draw_node_labels};
        use ratatui::{Terminal, backend::TestBackend, widgets::canvas::Canvas};
        for (x, y, expected) in [(10.0, 30.0, (9, 24)), (139.0, 8.0, (99, 8))] {
            let node = TopoNode {
                label: "BOTTOM".into(),
                ip: "192.168.1.10".into(),
                device_type: unifly_api::DeviceType::Switch,
                state: unifly_api::DeviceState::Offline,
                traffic_bytes_per_sec: 0,
                x,
                y,
                width: 18.0,
                height: 3.0,
            };
            let viewport = Viewport {
                left: -2.0,
                top: -2.0,
                x_span: 99.0 * 1.44,
                y_span: 24.0 * 1.44,
                width: 100,
                height: 25,
            };
            let mut terminal = Terminal::new(TestBackend::new(100, 25)).expect("terminal");
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    frame.render_widget(
                        Canvas::default()
                            .x_bounds([-2.0, -2.0 + viewport.x_span])
                            .y_bounds([-(-2.0 + viewport.y_span), 2.0])
                            .paint(|context| draw_node(context, &node)),
                        area,
                    );
                    draw_node_labels(frame.buffer_mut(), area, &node, &viewport);
                })
                .expect("render");
            assert_eq!(terminal.backend().buffer()[expected].symbol(), "B");
        }
    }

    #[test]
    fn addresses_are_hidden_instead_of_truncated() {
        use super::{Buffer, Rect, TopoNode, Viewport, draw_node_labels};
        let area = Rect::new(0, 0, 30, 10);
        let viewport = Viewport {
            left: 0.0,
            top: 0.0,
            x_span: 29.0,
            y_span: 9.0,
            width: 30,
            height: 10,
        };
        let mut node = TopoNode {
            label: "SWITCH".into(),
            ip: "192.168.1.10".into(),
            device_type: unifly_api::DeviceType::Switch,
            state: unifly_api::DeviceState::Online,
            traffic_bytes_per_sec: 0,
            x: 5.0,
            y: 1.0,
            width: 11.0,
            height: 4.0,
        };
        for width in [11.0, 40.0] {
            node.width = width;
            if width > 11.0 {
                node.x = 23.0;
            }
            let mut buffer = Buffer::empty(area);
            draw_node_labels(&mut buffer, area, &node, &viewport);
            assert!(
                !buffer
                    .content
                    .iter()
                    .any(|cell| cell.symbol().chars().any(|c| c.is_ascii_digit()))
            );
        }
    }

    #[test]
    fn port_labels_never_overwrite_projected_child_borders() {
        use super::{Buffer, LinkKind, Rect, TopoEdge, TopoNode, Viewport, draw_edge_label};
        let area = Rect::new(0, 0, 100, 25);
        let node = TopoNode {
            label: "CHILD".into(),
            ip: String::new(),
            device_type: unifly_api::DeviceType::Switch,
            state: unifly_api::DeviceState::Online,
            traffic_bytes_per_sec: 0,
            x: 10.0,
            y: 12.0,
            width: 18.0,
            height: 3.0,
        };
        let edge = TopoEdge {
            parent: 0,
            child: 0,
            kind: LinkKind::Wired,
            port: Some(42),
            stem_x: 19.0,
            bus_y: 11.0,
        };
        let viewport = Viewport {
            left: -2.0,
            top: 8.0,
            x_span: 99.0 * 1.44,
            y_span: 24.0 * 1.44,
            width: 100,
            height: 25,
        };
        let mut buffer = Buffer::empty(area);
        draw_edge_label(&mut buffer, area, &[node], &edge, &viewport);
        assert!(buffer.content.iter().all(|cell| cell.symbol() == " "));
    }

    #[test]
    fn edge_weight_scales_with_traffic() {
        assert_eq!(edge_weight(0), 1);
        assert_eq!(edge_weight(125_000), 1);
        assert_eq!(edge_weight(125_001), 2);
        assert_eq!(edge_weight(12_500_000), 2);
        assert_eq!(edge_weight(12_500_001), 3);
    }
}
