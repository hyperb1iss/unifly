use std::collections::HashMap;

use std::fmt::Write as _;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Context, Line as CanvasLine, Rectangle};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use unifly_api::{DeviceState, DeviceType};

use super::layout::{LinkKind, TopoEdge, TopoNode, cell_width, fit_cells};
use super::state::Scene;
use super::{Camera, TopologyScreen};
use crate::tui::theme;
use crate::tui::widgets::click::{self, Click, Hint};

/// Legend entries, most useful first: they are added while they fit, so a
/// narrow terminal drops the tail rather than clipping one. Device state
/// leads because nothing on the map spells it out, where a mesh link and
/// a wired port carry their own labels.
fn legend() -> [(&'static str, Color, &'static str); 5] {
    [
        ("\u{25A0} ", theme::error(), "down"),
        ("\u{25A0} ", theme::warning(), "busy"),
        ("\u{2500} ", theme::accent_secondary(), "wired"),
        ("\u{254C} ", theme::info(), "mesh"),
        ("\u{2504} ", theme::border_unfocused(), "uplink unknown"),
    ]
}

/// A device state as the title names it.
fn state_label(state: DeviceState) -> &'static str {
    match state {
        DeviceState::Online => "online",
        DeviceState::Offline => "offline",
        DeviceState::PendingAdoption => "pending adoption",
        DeviceState::Updating => "updating",
        DeviceState::GettingReady => "getting ready",
        DeviceState::Adopting => "adopting",
        DeviceState::Deleting => "being removed",
        DeviceState::ConnectionInterrupted => "connection interrupted",
        DeviceState::Isolated => "isolated",
        _ => "state unknown",
    }
}

impl TopologyScreen {
    pub(super) fn render_screen(&self, frame: &mut Frame, area: Rect) {
        self.hits.borrow_mut().clear();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(if self.focused {
                theme::border_focused()
            } else {
                theme::border_default()
            });
        let content_area = block.inner(area);
        self.canvas.set(content_area);

        // The layout works in cells (y down). At 100% zoom one unit is one
        // cell, so every label lands whole inside its box (a canvas maps a
        // span of n - 1 units onto n cells).
        let scene = self.scene(content_area);
        let (x_span, y_span) = scene.spans();
        let (left, top) = scene.origin();
        let (left, top) = (left.floor(), top.floor());
        let viewport = Viewport {
            left,
            top,
            x_span,
            y_span,
            width: content_area.width,
            height: content_area.height,
        };

        let mut block = block
            .title(self.title(&scene, &viewport))
            .title_style(theme::title_style());
        // The legend rides the bottom border: it explains the map, so it
        // sits on the map's frame, where it can never cover a device.
        if !scene.tree.nodes.is_empty() {
            block = block.title_bottom(legend_line(area.width.saturating_sub(2)));
        }
        frame.render_widget(block, area);

        let tree = &scene.tree;
        if tree.nodes.is_empty() {
            let message = self.link.waiting().unwrap_or("No devices on this site yet");
            let row = Rect {
                y: content_area.y + content_area.height / 2,
                height: content_area.height.min(1),
                ..content_area
            };
            frame.render_widget(
                Paragraph::new(Span::styled(
                    message,
                    Style::default().fg(theme::text_muted()),
                ))
                .alignment(Alignment::Center),
                row,
            );
            return;
        }

        let selected = self.selected_index();
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
                for (index, node) in tree.nodes.iter().enumerate() {
                    draw_node(ctx, node, selected == Some(index), scene.zoom);
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
        let mut hits = self.hits.borrow_mut();
        for (index, node) in tree.nodes.iter().enumerate() {
            draw_node_labels(
                frame.buffer_mut(),
                content_area,
                node,
                &viewport,
                selected == Some(index),
            );
            hits.push(viewport.node_cells(content_area, node), Click::Row(index));
        }
    }

    /// The title: how many devices, the zoom and whether it is the fitted
    /// one, the device under the cursor, and which way more of the tree
    /// lies when it runs off the canvas.
    fn title(&self, scene: &Scene, viewport: &Viewport) -> String {
        let mut title = format!(" Topology ({})", scene.tree.nodes.len());
        if scene.tree.nodes.is_empty() {
            title.push(' ');
            return title;
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::as_conversions
        )]
        let percent = (scene.zoom * 100.0).round() as u32;
        let _ = write!(title, "  \u{00B7}  {percent}%");
        if self.camera == Camera::Fit {
            title.push_str(" fit");
        }
        if let Some(node) = self
            .selected_index()
            .and_then(|at| scene.tree.nodes.get(at))
        {
            let _ = write!(title, "  \u{00B7}  \u{25B8} {}", node.label);
            if !node.ip.is_empty() {
                let _ = write!(title, " {}", node.ip);
            }
            let _ = write!(title, " {}", state_label(node.state));
        }
        let [x_lo, x_hi, y_lo, y_hi] = scene.tree.bounds;
        let more: String = [
            (x_lo < viewport.left, '\u{2190}'),
            (y_lo < viewport.top, '\u{2191}'),
            (y_hi > viewport.top + viewport.y_span, '\u{2193}'),
            (x_hi > viewport.left + viewport.x_span, '\u{2192}'),
        ]
        .into_iter()
        .filter_map(|(beyond, arrow)| beyond.then_some(arrow))
        .collect();
        if !more.is_empty() {
            let _ = write!(title, "  \u{00B7}  more {more}");
        }
        title.push(' ');
        title
    }

    /// What the status bar offers: the device cursor and what `Enter`
    /// does with it, then the zoom. Panning is navigation and lives in the
    /// help overlay.
    pub(super) fn status_hints(&self) -> Vec<(u8, Hint)> {
        if self.devices.is_empty() {
            return Vec::new();
        }
        // With no cursor yet, `Enter` puts it on the first device.
        let enter = if self.selected.is_some() {
            "open"
        } else {
            "select"
        };
        let mut hints = vec![
            (
                0,
                Hint::button("Enter", enter, crossterm::event::KeyCode::Enter),
            ),
            (1, Hint::button("[", "prev", click::char_key('['))),
            (1, Hint::button("]", "next", click::char_key(']'))),
            (2, Hint::button("+", "zoom in", click::char_key('+'))),
            (2, Hint::button("-", "zoom out", click::char_key('-'))),
            (3, Hint::button("r", "100%", click::char_key('r'))),
        ];
        // A fitted map has nothing to fit.
        if self.camera != Camera::Fit {
            hints.push((3, Hint::button("f", "fit", click::char_key('f'))));
        }
        hints
    }
}

/// The legend for a bottom border with `width` columns between its
/// corners: as many whole entries as fit.
fn legend_line(width: u16) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    let mut used = 1;
    for (index, (symbol, color, label)) in legend().into_iter().enumerate() {
        let gap = if index == 0 { "" } else { "  " };
        let entry = [
            Span::raw(gap),
            Span::styled(symbol, Style::default().fg(color)),
            Span::styled(label, theme::key_hint()),
        ];
        let entry_width: usize = entry.iter().map(Span::width).sum();
        // One more column closes the legend off from the border.
        if used + entry_width + 1 > usize::from(width) {
            break;
        }
        used += entry_width;
        spans.extend(entry);
    }
    if spans.len() == 1 {
        return Line::default();
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

fn node_color(node: &TopoNode) -> Color {
    // A device that is updating or adopting is busy, not broken.
    if node.state.is_transitional() {
        return theme::warning();
    }
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

    /// The cells of `area` that `node`'s box covers, borders included.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    fn node_cells(&self, area: Rect, node: &TopoNode) -> Rect {
        let clamp = |value: f64, limit: u16| value.clamp(0.0, f64::from(limit)) as u16;
        let left = clamp(self.border_column(node.x), self.width);
        let right = clamp(self.border_column(node.x + node.width) + 1.0, self.width);
        let top = clamp(self.border_row(node.y), self.height);
        let bottom = clamp(self.border_row(node.bottom()) + 1.0, self.height);
        Rect {
            x: area.x + left,
            y: area.y + top,
            width: right.saturating_sub(left),
            height: bottom.saturating_sub(top),
        }
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

/// Draw a device box. The selected one gets a second outline one braille
/// dot outside its border, in the cursor color.
fn draw_node(ctx: &mut Context<'_>, node: &TopoNode, selected: bool, zoom: f64) {
    let color = node_color(node);
    ctx.draw(&Rectangle {
        x: node.x,
        y: up(node.bottom()),
        width: node.width,
        height: node.height,
        color,
    });
    if selected {
        // A cell is two dots wide and four tall.
        let (dx, dy) = (0.5 / zoom, 0.25 / zoom);
        ctx.draw(&Rectangle {
            x: node.x - dx,
            y: up(node.bottom()) - dy,
            width: node.width + 2.0 * dx,
            height: node.height + 2.0 * dy,
            color: theme::text_primary(),
        });
    }
}

fn draw_node_labels(
    buffer: &mut Buffer,
    area: Rect,
    node: &TopoNode,
    viewport: &Viewport,
    selected: bool,
) {
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
        &Span::styled(
            fit_cells(&node.label, inner),
            if selected {
                theme::table_selected()
            } else {
                Style::default().fg(color)
            },
        ),
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
    match edge.kind {
        LinkKind::Wired => (
            nodes
                .get(edge.child)
                .map_or_else(theme::text_secondary, node_color),
            None,
        ),
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
                            .paint(|context| draw_node(context, &border, false, zoom)),
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
                            .paint(|context| draw_node(context, &node, false, 1.0)),
                        frame.area(),
                    );
                    let area = frame.area();
                    draw_node_labels(frame.buffer_mut(), area, &node, &viewport, false);
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
                            .paint(|context| draw_node(context, &node, false, 1.0)),
                        area,
                    );
                    draw_node_labels(frame.buffer_mut(), area, &node, &viewport, false);
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
            draw_node_labels(&mut buffer, area, &node, &viewport, false);
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

    fn node_in(state: unifly_api::DeviceState) -> super::TopoNode {
        super::TopoNode {
            label: "AP".into(),
            ip: String::new(),
            device_type: unifly_api::DeviceType::AccessPoint,
            state,
            traffic_bytes_per_sec: 0,
            x: 0.0,
            y: 0.0,
            width: 8.0,
            height: 3.0,
        }
    }

    #[test]
    fn a_device_that_is_updating_is_drawn_busy_not_broken() {
        use unifly_api::DeviceState;

        use crate::tui::theme;

        for state in [
            DeviceState::Updating,
            DeviceState::Adopting,
            DeviceState::GettingReady,
            DeviceState::PendingAdoption,
        ] {
            assert_eq!(
                super::node_color(&node_in(state)),
                theme::warning(),
                "{state:?}"
            );
        }
        assert_eq!(
            super::node_color(&node_in(DeviceState::Offline)),
            theme::error()
        );
        assert_eq!(
            super::node_color(&node_in(DeviceState::Online)),
            theme::accent_primary()
        );
    }

    fn draw(screen: &super::TopologyScreen, width: u16, height: u16) -> Vec<String> {
        crate::tui::table::render_lines(width, height, |frame, area| {
            screen.render_screen(frame, area);
        })
    }

    #[test]
    fn an_empty_map_says_why_it_is_empty() {
        use crate::tui::action::Action;

        let mut screen = super::TopologyScreen::new();
        assert!(draw(&screen, 80, 20).join("\n").contains("Connecting"));

        screen.apply_action(&Action::Connected);
        let text = draw(&screen, 80, 20).join("\n");
        assert!(text.contains("No devices on this site yet"), "{text}");

        screen.apply_action(&Action::Disconnected("gone".into()));
        assert!(draw(&screen, 80, 20).join("\n").contains("Not connected"));
    }

    #[test]
    fn the_legend_rides_the_bottom_border_and_drops_entries_whole() {
        use crate::tui::screens::topology::state::tests::screen_with;

        let screen = screen_with(3);
        let border = |width| {
            let rows = draw(&screen, width, 20);
            rows[rows.len() - 1].clone()
        };

        let wide = border(100);
        assert!(
            wide.starts_with(
                "\u{2570} \u{25A0} down  \u{25A0} busy  \u{2500} wired  \u{254C} mesh"
            ),
            "{wide}"
        );
        assert!(wide.contains("uplink unknown \u{2500}"), "{wide}");

        let narrow = border(30);
        assert!(narrow.contains("wired \u{2500}"), "{narrow}");
        assert!(
            !narrow.contains("mes") && !narrow.contains("upl"),
            "a whole entry ends the legend: {narrow}"
        );

        let empty = draw(&super::TopologyScreen::new(), 100, 20);
        assert!(
            !empty[empty.len() - 1].contains("wired"),
            "nothing to explain yet: {}",
            empty[empty.len() - 1]
        );
    }

    #[test]
    fn the_map_takes_every_row_inside_the_border() {
        use ratatui::layout::Rect;

        use crate::tui::screens::topology::state::tests::screen_with;

        let screen = screen_with(3);
        let text = draw(&screen, 100, 20).join("\n");
        assert_eq!(screen.canvas.get(), Rect::new(1, 1, 98, 18));
        for key in ["Enter open", "zoom in", "f fit", "hjkl"] {
            assert!(!text.contains(key), "a hint row of its own:\n{text}");
        }
    }

    #[test]
    fn the_status_bar_offers_the_cursor_and_the_zoom() {
        use crossterm::event::{KeyCode, KeyEvent};

        use crate::tui::screens::topology::state::tests::screen_with;
        use crate::tui::widgets::click::hint_text;

        assert!(super::TopologyScreen::new().status_hints().is_empty());

        let mut screen = screen_with(3);
        draw(&screen, 100, 30);
        // A fitted map offers no fit, and Enter has no device to open.
        assert_eq!(
            hint_text(&screen.status_hints()),
            "Enter select  [ prev  ] next  + zoom in  - zoom out  r 100%"
        );

        screen.handle_key_input(KeyEvent::from(KeyCode::Char(']')));
        screen.handle_key_input(KeyEvent::from(KeyCode::Char('+')));
        assert_eq!(
            hint_text(&screen.status_hints()),
            "Enter open  [ prev  ] next  + zoom in  - zoom out  r 100%  f fit"
        );
    }

    #[test]
    fn the_title_points_at_what_runs_off_the_canvas() {
        use crossterm::event::{KeyCode, KeyEvent};

        use crate::tui::screens::topology::state::tests::screen_with;

        let small = &draw(&screen_with(3), 100, 30)[0];
        assert!(!small.contains("more"), "nothing is hidden: {small}");

        // A tree too tall for a fit that keeps names whole says where the
        // rest is, and only there.
        let mut screen = screen_with(40);
        let fitted = &draw(&screen, 100, 30)[0];
        assert!(
            fitted.contains("fit") && fitted.contains("more \u{2193}"),
            "{fitted}"
        );
        for arrow in ['\u{2190}', '\u{2191}', '\u{2192}'] {
            assert!(!fitted.contains(arrow), "{fitted}");
        }

        screen.handle_key_input(KeyEvent::from(KeyCode::Char('r')));
        let title = &draw(&screen, 100, 30)[0];
        assert!(
            title.contains("more") && title.contains('\u{2193}'),
            "{title}"
        );
        assert!(
            !title.contains('\u{2191}'),
            "nothing is above the top: {title}"
        );
    }

    /// The demo fleet's devices, as the Topology screen receives them.
    async fn demo_devices() -> std::sync::Arc<Vec<std::sync::Arc<unifly_api::Device>>> {
        let demo = unifly_api::Controller::demo(unifly_api::DemoFleetOptions::default());
        demo.connect().await.expect("demo fleet connects");
        let devices = demo.devices_snapshot();
        demo.disconnect().await;
        devices
    }

    #[tokio::test]
    async fn the_fitted_demo_fleet_draws_every_name_whole_at_common_sizes() {
        use crate::tui::action::Action;
        use crate::tui::screens::topology::TopologyScreen;

        let devices = demo_devices().await;
        let mut screen = TopologyScreen::new();
        screen.apply_action(&Action::Connected);
        screen.apply_action(&Action::DevicesUpdated(devices));

        // The screen's area on 80x24, 100x30, and 120x34 terminals.
        for (width, height) in [(80, 22), (100, 28), (120, 32)] {
            let rows = draw(&screen, width, height);
            let text = rows.join("\n");
            let scene = screen.last_scene();
            assert!(scene.zoom >= 0.8, "{width}x{height}: {}", scene.zoom);
            let mut whole = 0;
            for (index, node) in scene.tree.nodes.iter().enumerate() {
                if !scene.shows(index) {
                    continue;
                }
                whole += 1;
                assert!(
                    text.contains(&node.label),
                    "{} is cut at {width}x{height}:\n{text}",
                    node.label
                );
            }
            assert!(whole >= 3, "{width}x{height} shows {whole} boxes:\n{text}");
            // What is not on screen is pointed at, never drawn as a stub.
            let hidden = whole < scene.tree.nodes.len();
            assert_eq!(rows[0].contains("more"), hidden, "{}", rows[0]);
        }
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
