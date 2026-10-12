//! Subtree measurement and placement for the topology layout.
//!
//! A tidy tree in cell units: a parent centers over a single row of
//! children, and when that row would be wider than the canvas the children
//! wrap into several rows hung off a trunk on the left, so no row ever has
//! to be squeezed (and its labels overlapped) to fit.

use std::collections::HashMap;

use super::TopoNode;

/// Box height: border, name, address, border spans four rows, which is a
/// three-unit rectangle (both edges are drawn).
pub(super) const NODE_HEIGHT: f64 = 3.0;
/// Rows from a parent's bottom edge to its first row of children: the bus
/// row and the port-label row.
const LEVEL_GAP: f64 = 3.0;
/// Rows between wrapped rows of children (the bus runs through here).
const ROW_GAP: f64 = 3.0;
/// Columns between neighboring subtrees.
const SIBLING_GAP: f64 = 2.0;
/// Columns kept clear left of wrapped rows for the trunk line.
const TRUNK: f64 = 3.0;
/// Columns the renderer leaves around the tree when fitting it.
pub(in crate::tui::screens) const MARGIN: f64 = 2.0;
/// Narrowest box, so short names still read as boxes.
const MIN_BOX: usize = 8;

/// Box width in cells: the longer of name and address plus a cell of
/// padding on each side.
pub(super) fn box_width(label: &str, ip: &str) -> f64 {
    let cells = label.chars().count().max(ip.chars().count()) + 2;
    cells_f64(cells.max(MIN_BOX))
}

#[allow(clippy::cast_precision_loss, clippy::as_conversions)]
fn cells_f64(cells: usize) -> f64 {
    cells as f64
}

#[derive(Debug, Clone, Copy)]
struct Size {
    width: f64,
    height: f64,
}

pub(super) struct Placer<'a> {
    nodes: &'a mut [TopoNode],
    children: &'a [Vec<usize>],
    /// Widest a row of children may get.
    limit: f64,
    sizes: Vec<Option<Size>>,
    rows: Vec<Vec<Vec<usize>>>,
    /// Child index to `(stem_x, bus_y)` of the link into it.
    routes: HashMap<usize, (f64, f64)>,
}

impl<'a> Placer<'a> {
    pub(super) fn new(nodes: &'a mut [TopoNode], children: &'a [Vec<usize>], columns: f64) -> Self {
        let count = nodes.len();
        Self {
            nodes,
            children,
            limit: (columns - 2.0 * MARGIN).max(1.0),
            sizes: vec![None; count],
            rows: vec![Vec::new(); count],
            routes: HashMap::new(),
        }
    }

    /// Place every root side by side, returning the link routes.
    pub(super) fn place_forest(&mut self, roots: &[usize]) -> HashMap<usize, (f64, f64)> {
        let mut x = 0.0;
        for root in roots {
            let size = self.measure(*root);
            self.place(*root, x, 0.0);
            x += size.width + SIBLING_GAP;
        }
        std::mem::take(&mut self.routes)
    }

    fn size(&self, index: usize) -> Size {
        self.sizes[index].unwrap_or(Size {
            width: self.nodes[index].width,
            height: NODE_HEIGHT,
        })
    }

    fn row_width(&self, row: &[usize]) -> f64 {
        let widths: f64 = row.iter().map(|child| self.size(*child).width).sum();
        widths + SIBLING_GAP * cells_f64(row.len().saturating_sub(1))
    }

    fn row_height(&self, row: &[usize]) -> f64 {
        row.iter()
            .map(|child| self.size(*child).height)
            .fold(0.0, f64::max)
    }

    fn measure(&mut self, index: usize) -> Size {
        if let Some(size) = self.sizes[index] {
            return size;
        }
        let children = self.children;
        let kids = &children[index];
        let own = self.nodes[index].width;
        let size = if kids.is_empty() {
            Size {
                width: own,
                height: NODE_HEIGHT,
            }
        } else {
            for kid in kids {
                self.measure(*kid);
            }
            let rows = self.pack(kids);
            let widest = rows
                .iter()
                .map(|row| self.row_width(row))
                .fold(0.0, f64::max);
            let tall: f64 = rows.iter().map(|row| self.row_height(row)).sum();
            let gaps = ROW_GAP * cells_f64(rows.len().saturating_sub(1));
            let lane = if rows.len() > 1 { TRUNK } else { 0.0 };
            let size = Size {
                width: own.max(lane + widest),
                height: NODE_HEIGHT + LEVEL_GAP + tall + gaps,
            };
            self.rows[index] = rows;
            size
        };
        self.sizes[index] = Some(size);
        size
    }

    /// Split children into rows no wider than the limit, keeping order.
    fn pack(&self, kids: &[usize]) -> Vec<Vec<usize>> {
        if self.row_width(kids) <= self.limit {
            return vec![kids.to_vec()];
        }
        let limit = self.limit - TRUNK;
        let mut rows: Vec<Vec<usize>> = Vec::new();
        let mut current: Vec<usize> = Vec::new();
        let mut width = 0.0;
        for kid in kids {
            let kid_width = self.size(*kid).width;
            if !current.is_empty() && width + SIBLING_GAP + kid_width > limit {
                rows.push(std::mem::take(&mut current));
                width = 0.0;
            }
            if !current.is_empty() {
                width += SIBLING_GAP;
            }
            width += kid_width;
            current.push(*kid);
        }
        if !current.is_empty() {
            rows.push(current);
        }
        rows
    }

    /// Place the subtree of `index` with its block's top-left at `(x0, y0)`.
    fn place(&mut self, index: usize, x0: f64, y0: f64) {
        let size = self.size(index);
        let own = self.nodes[index].width;
        let rows = std::mem::take(&mut self.rows[index]);
        self.nodes[index].y = y0;

        let child_top = y0 + NODE_HEIGHT + LEVEL_GAP;
        // The bus runs on the row right under the parent (or the previous
        // row of children); the port labels take the row below it.
        let first_bus = y0 + NODE_HEIGHT + 1.0;
        match rows.as_slice() {
            // Positions stay on whole cells so borders and labels never
            // share a column.
            [] => self.nodes[index].x = (x0 + (size.width - own) / 2.0).round(),
            [row] => {
                let mut cursor = (x0 + (size.width - self.row_width(row)) / 2.0).round();
                let mut centers = Vec::with_capacity(row.len());
                for kid in row {
                    let kid_width = self.size(*kid).width;
                    self.place(*kid, cursor, child_top);
                    centers.push(self.nodes[*kid].center_x());
                    cursor += kid_width + SIBLING_GAP;
                }
                let first = centers.first().copied().unwrap_or(x0);
                let last = centers.last().copied().unwrap_or(first);
                let x = (f64::midpoint(first, last) - own / 2.0)
                    .round()
                    .clamp(x0, x0 + size.width - own);
                self.nodes[index].x = x;
                let stem = self.nodes[index].center_x();
                for kid in row {
                    self.routes.insert(*kid, (stem, first_bus));
                }
            }
            _ => {
                // Wrapped rows hang off a trunk dropping from the parent's
                // left side, clear of every row.
                self.nodes[index].x = x0;
                let stem = x0 + TRUNK / 2.0;
                let mut top = child_top;
                for (row_index, row) in rows.iter().enumerate() {
                    let bus = if row_index == 0 {
                        first_bus
                    } else {
                        top - ROW_GAP + 1.0
                    };
                    let mut cursor = x0 + TRUNK;
                    for kid in row {
                        let kid_width = self.size(*kid).width;
                        self.place(*kid, cursor, top);
                        self.routes.insert(*kid, (stem, bus));
                        cursor += kid_width + SIBLING_GAP;
                    }
                    top += self.row_height(row) + ROW_GAP;
                }
            }
        }
    }
}
