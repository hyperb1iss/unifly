//! Horizontal sub-tab bar for use within screens (e.g., firewall sub-tabs,
//! client type filters, device detail tabs).

use ratatui::style::Modifier;
use ratatui::text::Span;

use super::click::{Click, ClickLine};
use crate::tui::theme;

/// Builds a horizontal tab bar with the active tab highlighted. A click on
/// a tab resolves to [`Click::Tab`] with its position.
///
/// Each label is rendered inline. The active tab gets Electric Purple + underline;
/// inactive tabs get Dim White.
pub fn sub_tab_strip<'a>(labels: &[&'a str], active_index: usize) -> ClickLine<'a, Click> {
    let mut strip = ClickLine::default();

    for (i, label) in labels.iter().enumerate() {
        if i > 0 {
            strip.push_gap(Span::styled("  ", theme::key_hint()));
        }

        if i == active_index {
            strip.push(
                Click::Tab(i),
                Span::styled(
                    format!("[{label}]"),
                    theme::tab_active().add_modifier(Modifier::BOLD),
                ),
            );
        } else {
            strip.push(Click::Tab(i), Span::styled(*label, theme::tab_inactive()));
        }
    }

    strip
}
