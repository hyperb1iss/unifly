//! Horizontal sub-tab bar for use within screens (e.g., firewall sub-tabs,
//! client type filters, device detail tabs).

use ratatui::style::Modifier;
use ratatui::text::Span;

use super::click::{Click, ClickLine};
use crate::tui::theme;

/// Builds a horizontal tab bar with the active tab highlighted. A click on
/// a tab resolves to [`Click::Tab`] with its position.
///
/// Every tab is its label with one cell of padding on each side, whether
/// or not it is the active one, so moving the selection never moves a
/// label. The active tab is marked by weight and underline as well as
/// color, which keeps it readable without color.
pub fn sub_tab_strip<'a>(labels: &[&'a str], active_index: usize) -> ClickLine<'a, Click> {
    let mut strip = ClickLine::default();

    for (i, label) in labels.iter().enumerate() {
        let style = if i == active_index {
            theme::tab_active().add_modifier(Modifier::BOLD)
        } else {
            theme::tab_inactive()
        };
        strip.push(Click::Tab(i), Span::raw(" "));
        strip.push(Click::Tab(i), Span::styled(*label, style));
        strip.push(Click::Tab(i), Span::raw(" "));
    }

    strip
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(labels: &[&str], active: usize) -> String {
        sub_tab_strip(labels, active)
            .into_line()
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn moving_the_selection_never_moves_a_label() {
        let labels = ["All", "Wireless", "Wired"];
        let first = text(&labels, 0);
        assert_eq!(first, " All  Wireless  Wired ");
        assert_eq!(text(&labels, 1), first);
        assert_eq!(text(&labels, 2), first);
    }

    #[test]
    fn the_active_tab_is_marked_without_relying_on_color() {
        let line = sub_tab_strip(&["All", "Wired"], 1).into_line();
        let wired = line
            .spans
            .iter()
            .find(|span| span.content == "Wired")
            .expect("the label is drawn");
        assert!(wired.style.add_modifier.contains(Modifier::BOLD));
        assert!(wired.style.add_modifier.contains(Modifier::UNDERLINED));
        let all = line
            .spans
            .iter()
            .find(|span| span.content == "All")
            .expect("the label is drawn");
        assert!(!all.style.add_modifier.contains(Modifier::UNDERLINED));
    }
}
