//! The one-row toolbar above a list: what narrows the view on the left,
//! what describes it on the right.

use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;

use super::click::{Click, ClickLine, HitMap};
use crate::tui::theme;

/// Columns kept clear between the two sides.
const GAP: u16 = 2;

/// Columns the right side stays in from the edge, matching the padding a
/// tab strip starts with on the left.
const MARGIN: u16 = 1;

/// Draw `left` from the left edge of `area` and `right` against its right
/// edge. The right side is left out when the two would not fit with a gap
/// between them: what narrows the list outranks what describes it.
pub fn render<'a>(
    frame: &mut Frame,
    area: Rect,
    left: ClickLine<'a, Click>,
    right: ClickLine<'a, Click>,
    hits: &mut HitMap<Click>,
) {
    if area.is_empty() {
        return;
    }
    if let Some(right_area) = right_area(area, left.width(), right.width()) {
        let line = right.draw(right_area, hits);
        frame.render_widget(Paragraph::new(line), right_area);
    }
    let line = left.draw(area, hits);
    frame.render_widget(Paragraph::new(line), area);
}

/// Where the right side lands, or `None` when it does not fit beside a
/// left side `left` columns wide.
fn right_area(area: Rect, left: usize, right: usize) -> Option<Rect> {
    let left = u16::try_from(left).ok()?;
    let right = u16::try_from(right).ok()?;
    if right == 0 {
        return None;
    }
    let needed = left
        .checked_add(GAP)?
        .checked_add(right)?
        .checked_add(MARGIN)?;
    (needed <= area.width).then(|| Rect {
        x: area.right() - MARGIN - right,
        width: right,
        height: 1,
        ..area
    })
}

/// A labelled value for the right side of a toolbar, such as `s sort name
/// ↑`. With a key it is a button that presses it, and shows the key so
/// the control explains itself where it sits.
pub fn chip(
    key: Option<(&'static str, KeyEvent)>,
    label: &'static str,
    value: String,
) -> ClickLine<'static, Click> {
    let label_style = Style::default().fg(theme::text_secondary());
    let value_style = Style::default().fg(theme::accent_secondary());
    let mut line = ClickLine::default();
    if let Some((shown, press)) = key {
        let target = Click::Key(press);
        line.push(
            target.clone(),
            Span::styled(format!("{shown} "), theme::key_hint_key()),
        );
        line.push(
            target.clone(),
            Span::styled(format!("{label} "), label_style),
        );
        line.push(target, Span::styled(value, value_style));
    } else {
        line.push_gap(Span::styled(format!("{label} "), label_style));
        line.push_gap(Span::styled(value, value_style));
    }
    line
}

/// Columns the right side can take in a toolbar `width` columns wide
/// whose left side takes `left`, for a screen that picks what goes on
/// the right by the room there is.
pub fn right_room(width: u16, left: usize) -> usize {
    usize::from(width).saturating_sub(left + usize::from(GAP + MARGIN))
}

/// Columns between two chips on the right side.
const CHIP_GAP: usize = 3;

/// Draw `left` from the left edge of `area` and, against its right edge,
/// as many of `chips` as fit beside it. The chips are given least
/// important first: on a row too narrow for all of them, the first ones
/// are left out whole and the rest keep their place at the edge.
pub fn render_chips<'a>(
    frame: &mut Frame,
    area: Rect,
    left: ClickLine<'a, Click>,
    chips: Vec<ClickLine<'a, Click>>,
    hits: &mut HitMap<Click>,
) {
    let room = right_room(area.width, left.width());
    let widths: Vec<usize> = chips.iter().map(ClickLine::width).collect();
    let mut right = ClickLine::default();
    for (index, chip) in chips
        .into_iter()
        .skip(chips_left_out(&widths, room))
        .enumerate()
    {
        if index > 0 {
            right.push_gap(Span::raw(" ".repeat(CHIP_GAP)));
        }
        right.append(chip);
    }
    render(frame, area, left, right, hits);
}

/// How many chips, from the first, must go for the rest to fit `room`
/// columns with their gaps.
fn chips_left_out(widths: &[usize], room: usize) -> usize {
    (0..widths.len())
        .find(|skip| {
            let shown = &widths[*skip..];
            shown.iter().sum::<usize>() + CHIP_GAP * (shown.len() - 1) <= room
        })
        .unwrap_or(widths.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_right_side_sits_one_column_in_from_the_edge() {
        let area = Rect::new(1, 1, 40, 1);
        let right = right_area(area, 10, 8).expect("it fits");
        assert_eq!(right.right(), area.right() - 1);
        assert_eq!(right.width, 8);
    }

    #[test]
    fn the_right_side_is_dropped_before_it_can_touch_the_left() {
        let area = Rect::new(0, 0, 20, 1);
        assert!(right_area(area, 10, 7).is_some());
        assert!(right_area(area, 10, 8).is_none());
        assert!(right_area(area, 10, 0).is_none());
    }

    #[test]
    fn the_room_on_the_right_is_what_render_will_draw_in() {
        let area = Rect::new(0, 0, 20, 1);
        let room = right_room(area.width, 10);
        assert_eq!(room, 7);
        assert!(right_area(area, 10, room).is_some());
        assert!(right_area(area, 10, room + 1).is_none());
        assert_eq!(right_room(5, 10), 0);
    }

    #[test]
    fn a_narrow_row_leaves_out_the_first_chips_whole() {
        let widths = [14, 21, 17];
        assert_eq!(chips_left_out(&widths, 58), 0);
        assert_eq!(chips_left_out(&widths, 57), 1);
        assert_eq!(chips_left_out(&widths, 41), 1);
        assert_eq!(chips_left_out(&widths, 40), 2);
        assert_eq!(chips_left_out(&widths, 16), 3);
        assert_eq!(chips_left_out(&[], 0), 0);
    }
}
