//! Horizontal sub-tab bar for use within screens (e.g., firewall sub-tabs,
//! client type filters, device detail tabs).

use ratatui::style::Modifier;
use ratatui::text::Span;

use super::tab_strip::TabStrip;
use crate::tui::theme;

/// Builds a horizontal tab bar with the active tab highlighted.
///
/// Each label is rendered inline. The active tab gets Electric Purple + underline;
/// inactive tabs get Dim White.
pub fn sub_tab_strip<'a>(labels: &[&'a str], active_index: usize) -> TabStrip<'a> {
    let mut strip = TabStrip::default();

    for (i, label) in labels.iter().enumerate() {
        if i > 0 {
            strip.push_gap(Span::styled("  ", theme::key_hint()));
        }

        if i == active_index {
            strip.push_tab(
                i,
                Span::styled(
                    format!("[{label}]"),
                    theme::tab_active().add_modifier(Modifier::BOLD),
                ),
            );
        } else {
            strip.push_tab(i, Span::styled(*label, theme::tab_inactive()));
        }
    }

    strip
}
