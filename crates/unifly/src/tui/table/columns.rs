//! Column widths sized to what the cells hold.
//!
//! Each column asks for the width of its header or widest value. When the
//! table is wider than the columns need, the spare cells widen the gaps
//! between columns evenly (up to a cap) instead of all landing in one name
//! column. When it is narrower, elastic columns give width back widest
//! first, and only past their floors does the right edge get cut.

use std::cmp::Reverse;

use ratatui::layout::Constraint;
use ratatui::text::Span;

/// Cells between adjacent columns, matching ratatui's `Table` default.
const COLUMN_SPACING: u16 = 1;

/// Most spare cells one column takes, so a sparse table on a wide terminal
/// keeps its columns together instead of scattering them across the screen.
const MAX_GROWTH: u16 = 8;

/// A trailing column narrower than this is dropped rather than shown as a
/// sliver.
const MIN_VISIBLE: u16 = 3;

/// Width request for one table column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column {
    /// Cells the column asks for.
    want: u16,
    /// Cells the column keeps when the table is too narrow for every `want`.
    floor: u16,
    /// Whether spare width may widen the column.
    grow: bool,
}

impl Column {
    /// A column of exactly `width` cells that never shrinks, for fixed
    /// formats such as MAC addresses. Spare width still widens the gap
    /// after it.
    pub const fn fixed(width: u16) -> Self {
        Self {
            want: width,
            floor: width,
            grow: true,
        }
    }

    /// Keep the gap after this column at one cell, so a marker column (a
    /// status glyph, the selection arrow) stays next to the value it marks.
    #[must_use]
    pub const fn tight(mut self) -> Self {
        self.grow = false;
        self
    }

    /// A column as wide as its header or its widest value, capped at `max`.
    pub fn fit<I, S>(header: &str, values: I, max: u16) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let widest = values
            .into_iter()
            .map(|value| text_width(value.as_ref()))
            .fold(text_width(header), u16::max);
        let want = widest.min(max);
        Self {
            want,
            floor: want,
            grow: true,
        }
    }

    /// Ask for at least `cells`, so a column whose values change every
    /// refresh (rates, durations) holds its width instead of jittering.
    #[must_use]
    pub fn at_least(mut self, cells: u16) -> Self {
        let rigid = self.floor == self.want;
        self.want = self.want.max(cells);
        if rigid {
            self.floor = self.want;
        }
        self
    }

    /// Reserve `cells` more, for a marker drawn inside the column (the
    /// selection arrow, for one).
    #[must_use]
    pub const fn pad(mut self, cells: u16) -> Self {
        self.want = self.want.saturating_add(cells);
        self.floor = self.floor.saturating_add(cells);
        self
    }

    /// Let the column give width back, down to `floor` cells, when the
    /// table is narrower than its columns.
    #[must_use]
    pub fn shrink_to(mut self, floor: u16) -> Self {
        self.floor = floor.min(self.want);
        self
    }

    /// Cells the column asks for.
    pub const fn want(self) -> u16 {
        self.want
    }
}

/// Plan `Constraint::Length` widths for `columns` in a table `available`
/// cells wide (the table's default one-cell column spacing included).
///
/// The result can hold fewer constraints than `columns` when even the
/// floors do not fit: trailing columns are dropped, and `Table` then skips
/// their cells. The first column is never dropped, only cut.
pub fn plan_columns(columns: &[Column], available: u16) -> Vec<Constraint> {
    let mut widths: Vec<u16> = columns.iter().map(|column| column.want).collect();
    let total = total_width(&widths);
    if total > available {
        shrink(&mut widths, columns, total - available);
    } else {
        grow(&mut widths, columns, available - total);
    }
    widths.into_iter().map(Constraint::Length).collect()
}

fn text_width(text: &str) -> u16 {
    u16::try_from(Span::raw(text).width()).unwrap_or(u16::MAX)
}

fn total_width(widths: &[u16]) -> u16 {
    let gaps = u16::try_from(widths.len().saturating_sub(1))
        .unwrap_or(u16::MAX)
        .saturating_mul(COLUMN_SPACING);
    widths
        .iter()
        .fold(gaps, |total, width| total.saturating_add(*width))
}

fn shrink(widths: &mut Vec<u16>, columns: &[Column], mut deficit: u16) {
    while deficit > 0 {
        let widest_elastic = widths
            .iter()
            .zip(columns)
            .enumerate()
            .filter(|(_, (width, column))| **width > column.floor)
            .max_by_key(|(index, (width, _))| (**width, Reverse(*index)))
            .map(|(index, _)| index);
        let Some(index) = widest_elastic else {
            break;
        };
        widths[index] -= 1;
        deficit -= 1;
    }

    while deficit > 0 {
        let Some(&last) = widths.last() else {
            break;
        };
        let kept = last.saturating_sub(deficit);
        if kept < MIN_VISIBLE && widths.len() > 1 {
            widths.pop();
            deficit = deficit.saturating_sub(last.saturating_add(COLUMN_SPACING));
        } else if let Some(width) = widths.last_mut() {
            // The first column always stays, cut to what is left: an empty
            // width list would make `Table` split the area evenly instead.
            *width = kept;
            deficit = 0;
        }
    }
}

fn grow(widths: &mut [u16], columns: &[Column], slack: u16) {
    // Widening the last column only adds blank space at the right edge, so
    // the spare width goes to the gaps between columns instead.
    let growers: Vec<usize> = columns
        .iter()
        .enumerate()
        .take(columns.len().saturating_sub(1))
        .filter(|(_, column)| column.grow)
        .map(|(index, _)| index)
        .collect();
    let Ok(count) = u16::try_from(growers.len()) else {
        return;
    };
    if count == 0 {
        return;
    }
    let share = (slack / count).min(MAX_GROWTH);
    for index in growers {
        widths[index] += share;
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Constraint;

    use super::{Column, MAX_GROWTH, plan_columns};

    fn lengths(constraints: &[Constraint]) -> Vec<u16> {
        constraints
            .iter()
            .map(|constraint| match constraint {
                Constraint::Length(width) => *width,
                other => panic!("expected a length constraint, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn fit_takes_the_widest_value_or_header_and_respects_the_cap() {
        let model = Column::fit("Model", ["U7 Pro", "U6 Enterprise", "USW Flex"], 24);
        assert_eq!(model.want(), 13);

        let short = Column::fit("Status", ["on", "off"], 24);
        assert_eq!(short.want(), 6, "the header sets the floor of a fit");

        let capped = Column::fit("Name", ["an extremely long device name here"], 12);
        assert_eq!(capped.want(), 12);

        let empty = Column::fit("Uptime", Vec::<String>::new(), 24);
        assert_eq!(empty.want(), 6);

        let steady = Column::fit("TX/RX", ["1M/3M"], 15).at_least(9);
        assert_eq!(steady.want(), 9);
        assert_eq!(
            Column::fit("TX/RX", ["151.4G/352.0G"], 15)
                .at_least(9)
                .want(),
            13
        );
    }

    #[test]
    fn fit_measures_display_width_not_bytes() {
        let bars = Column::fit("", ["▂▄▆█"], 24);
        assert_eq!(bars.want(), 4);
        assert_eq!(Column::fit("", ["ap"], 24).pad(1).want(), 3);
    }

    #[test]
    fn spare_width_widens_gaps_evenly_but_not_the_last_column() {
        let columns = [
            Column::fixed(2).tight(),
            Column::fit("Name", ["ap-atrium"], 32),
            Column::fixed(17),
            Column::fit("Uptime", ["41d"], 8),
        ];
        // wants 2 + 9 + 17 + 6 = 34, plus 3 gaps = 37; 10 spare cells
        let widths = lengths(&plan_columns(&columns, 47));
        assert_eq!(widths, vec![2, 14, 22, 6]);
    }

    #[test]
    fn growth_is_capped_so_sparse_tables_stay_together() {
        let columns = [
            Column::fit("Name", ["External"], 32),
            Column::fit("Networks", ["2 networks"], 32),
        ];
        let widths = lengths(&plan_columns(&columns, 181));
        assert_eq!(widths, vec![8 + MAX_GROWTH, 10]);
    }

    #[test]
    fn narrow_tables_shrink_elastic_columns_widest_first() {
        let columns = [
            Column::fixed(2),
            Column::fit("Name", ["a-really-long-client-name"], 32).shrink_to(10),
            Column::fit("Network", ["Cameras-and-sensors"], 24).shrink_to(8),
            Column::fixed(15),
        ];
        // wants 2 + 25 + 19 + 15 = 61 plus 3 gaps = 64; 14 too many
        let widths = lengths(&plan_columns(&columns, 50));
        assert_eq!(widths, vec![2, 15, 15, 15]);
        assert_eq!(widths.iter().sum::<u16>() + 3, 50);
    }

    #[test]
    fn floors_hold_and_trailing_columns_give_way_last() {
        let columns = [
            Column::fixed(4),
            Column::fit("Name", ["a-long-name"], 32).shrink_to(6),
            Column::fixed(10),
            Column::fixed(8),
        ];
        // floors 4 + 6 + 10 + 8 = 28 plus 3 gaps = 31
        let truncated = lengths(&plan_columns(&columns, 27));
        assert_eq!(truncated, vec![4, 6, 10, 4]);

        let dropped = lengths(&plan_columns(&columns, 24));
        assert_eq!(dropped, vec![4, 6, 10]);
    }

    #[test]
    fn the_first_column_survives_any_width() {
        let columns = [Column::fixed(4), Column::fixed(10)];
        assert_eq!(lengths(&plan_columns(&columns, 2)), vec![2]);
        assert_eq!(lengths(&plan_columns(&columns, 0)), vec![0]);
        assert_eq!(plan_columns(&[], 10), Vec::<Constraint>::new());
    }
}
