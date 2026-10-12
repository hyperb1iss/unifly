use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Context, Line as CanvasLine, Rectangle};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use unifly_api::DeviceType;

use super::TopologyScreen;
use super::layout::{LinkKind, MARGIN, TopoEdge, TopoNode, layout};
use crate::tui::theme;

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

        let canvas = Canvas::default()
            .x_bounds([left, left + x_span])
            .y_bounds([-(top + y_span), -top])
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

    let (label_x, _) = node.label_span();
    ctx.print(
        label_x,
        up(node.y + 1.0),
        Span::styled(node.label.clone(), Style::default().fg(color)),
    );
    if !node.ip.is_empty() {
        ctx.print(
            label_x,
            up(node.y + 2.0),
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
    let (stem, cx) = (edge.stem_x, child.center_x());
    let parent_bottom = up(parent.bottom());
    let child_top = up(child.y);
    let bus = up(edge.bus_y);

    let (color, dash) = match edge.kind {
        LinkKind::Wired if child.state.is_online() => (node_color(child), None),
        LinkKind::Wired => (theme::error(), None),
        LinkKind::Wireless => (theme::info(), Some((1.6, 1.0))),
        LinkKind::Inferred => (theme::border_unfocused(), Some((0.5, 0.9))),
    };

    let segments = [
        ((stem, parent_bottom), (stem, bus)),
        ((stem, bus), (cx, bus)),
    ];
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
        // On the row under the bus, beside the drop.
        ctx.print(
            cx + 1.0,
            up(edge.bus_y + 1.0),
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
