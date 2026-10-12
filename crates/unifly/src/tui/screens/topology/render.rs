use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Context, Line as CanvasLine, Rectangle};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use unifly_api::DeviceType;

use super::TopologyScreen;
use super::layout::{LEVEL_GAP, LinkKind, TopoEdge, TopoNode, layout};
use crate::tui::theme;

/// Breathing room around the tree when fitting it to the canvas.
const MARGIN: f64 = 4.0;
/// Narrowest view at 100% zoom, so a two-device site is not drawn huge.
const MIN_SPAN: f64 = 60.0;

impl TopologyScreen {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    pub(super) fn render_screen(&self, frame: &mut Frame, area: Rect) {
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

        let tree = layout(&self.devices);

        // Fit the whole tree at 100% zoom, centered; pan and zoom work
        // relative to that view.
        let width = f64::from(content_area.width.max(1));
        let height = f64::from(content_area.height.max(1));
        let aspect = (height * 2.0) / width;
        let [x_lo, x_hi, y_lo, y_hi] = tree.bounds;
        let need_x = (x_hi - x_lo) + MARGIN * 2.0;
        let need_y = (y_hi - y_lo) + MARGIN * 2.0;
        let x_span = need_x.max(need_y / aspect).max(MIN_SPAN) / self.zoom;
        let y_span = x_span * aspect;
        let x_center = f64::midpoint(x_lo, x_hi) + self.pan_x;
        let y_center = f64::midpoint(y_lo, y_hi) + self.pan_y;

        let canvas = Canvas::default()
            .x_bounds([x_center - x_span / 2.0, x_center + x_span / 2.0])
            .y_bounds([y_center - y_span / 2.0, y_center + y_span / 2.0])
            .paint(|ctx: &mut Context<'_>| {
                for edge in &tree.edges {
                    draw_edge(ctx, &tree.nodes, edge);
                }
                for node in &tree.nodes {
                    draw_node(ctx, node);
                }
            });

        frame.render_widget(canvas, content_area);

        let hints = Line::from(vec![
            Span::styled("  ←→↑↓ ", theme::key_hint_key()),
            Span::styled("pan  ", theme::key_hint()),
            Span::styled("+/- ", theme::key_hint_key()),
            Span::styled("zoom  ", theme::key_hint()),
            Span::styled("f ", theme::key_hint_key()),
            Span::styled("fit  ", theme::key_hint()),
            Span::styled("r ", theme::key_hint_key()),
            Span::styled("reset  ", theme::key_hint()),
            Span::styled("─ ", Style::default().fg(theme::accent_secondary())),
            Span::styled("wired  ", theme::key_hint()),
            Span::styled("╌ ", Style::default().fg(theme::info())),
            Span::styled("mesh  ", theme::key_hint()),
            Span::styled("┄ ", Style::default().fg(theme::border_unfocused())),
            Span::styled("uplink unknown", theme::key_hint()),
        ]);
        frame.render_widget(Paragraph::new(hints), hints_area);
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

fn draw_node(ctx: &mut Context<'_>, node: &TopoNode) {
    let color = node_color(node);
    ctx.draw(&Rectangle {
        x: node.x,
        y: node.y,
        width: node.width,
        height: node.height,
        color,
    });

    let short_label: String = node.label.chars().take(10).collect();
    ctx.print(
        node.x + 1.0,
        node.top() - 1.5,
        Span::styled(short_label, Style::default().fg(color)),
    );
    if !node.ip.is_empty() {
        ctx.print(
            node.x + 1.0,
            node.y + 0.5,
            Span::styled(
                node.ip.clone(),
                Style::default().fg(theme::text_secondary()),
            ),
        );
    }
}

/// Org-chart connector: a stub down from the parent, a horizontal run at
/// mid-gap, and a drop into the child. Wired drops thicken with traffic;
/// mesh links are dashed, and guessed links are dotted and dim.
fn draw_edge(ctx: &mut Context<'_>, nodes: &[TopoNode], edge: &TopoEdge) {
    let (Some(parent), Some(child)) = (nodes.get(edge.parent), nodes.get(edge.child)) else {
        return;
    };
    let (px, cx) = (parent.center_x(), child.center_x());
    let parent_bottom = parent.y;
    let child_top = child.top();
    let bus = parent_bottom - (LEVEL_GAP - parent.height) / 2.0;

    let (color, dash) = match edge.kind {
        LinkKind::Wired if child.state.is_online() => (node_color(child), None),
        LinkKind::Wired => (theme::error(), None),
        LinkKind::Wireless => (theme::info(), Some((1.6, 1.0))),
        LinkKind::Inferred => (theme::border_unfocused(), Some((0.5, 0.9))),
    };

    let segments = [((px, parent_bottom), (px, bus)), ((px, bus), (cx, bus))];
    for (from, to) in segments {
        draw_segment(ctx, from, to, color, dash);
    }

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

    let label = match (edge.kind, edge.port) {
        (LinkKind::Wireless, _) => Some("mesh".to_owned()),
        (LinkKind::Wired, Some(port)) => Some(port.to_string()),
        _ => None,
    };
    if let Some(label) = label {
        // Halfway down the drop, clear of both the bus and the child box.
        ctx.print(
            cx + 0.8,
            f64::midpoint(bus, child_top),
            Span::styled(label, Style::default().fg(theme::text_muted())),
        );
    }
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
    fn edge_weight_scales_with_traffic() {
        assert_eq!(edge_weight(0), 1);
        assert_eq!(edge_weight(125_000), 1);
        assert_eq!(edge_weight(125_001), 2);
        assert_eq!(edge_weight(12_500_000), 2);
        assert_eq!(edge_weight(12_500_001), 3);
    }
}
