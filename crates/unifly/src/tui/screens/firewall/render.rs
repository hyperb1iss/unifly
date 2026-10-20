mod policies;
mod rules;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph};

use crate::tui::action::FirewallSubTab;
use crate::tui::theme;
use crate::tui::widgets::click::{self, Click, ClickLine, Hint};
use crate::tui::widgets::sub_tabs;

use super::FirewallScreen;

impl FirewallScreen {
    pub(super) fn render_screen(&self, frame: &mut Frame, area: Rect) {
        self.hits.borrow_mut().clear();
        let block = Block::default()
            .title(" Firewall ")
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

        let layout = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

        let tab_labels = &["Policies", "Zones", "ACL Rules", "NAT"];
        let strip = sub_tabs::sub_tab_strip(tab_labels, self.sub_tab_index());
        let line = strip.draw(layout[0], &mut self.hits.borrow_mut());
        frame.render_widget(Paragraph::new(line), layout[0]);

        match self.sub_tab {
            FirewallSubTab::Policies => self.render_policies(frame, layout[1]),
            FirewallSubTab::Zones => self.render_zones(frame, layout[1]),
            FirewallSubTab::AclRules => self.render_acl_rules(frame, layout[1]),
            FirewallSubTab::NatPolicies => self.render_nat_policies(frame, layout[1]),
        }

        let line = self
            .hint_line()
            .draw(layout[2], &mut self.hits.borrow_mut());
        frame.render_widget(Paragraph::new(line), layout[2]);
    }

    fn hint_line(&self) -> ClickLine<'static, Click> {
        match self.sub_tab {
            FirewallSubTab::Policies => click::key_hints(&[
                Hint::info("j/k", "navigate"),
                Hint::button("K", "move up", click::char_key('K')),
                Hint::button("J", "move down", click::char_key('J')),
                Hint::info("h/l", "sub-tab"),
            ]),
            _ => click::key_hints(&[Hint::info("j/k", "navigate"), Hint::info("h/l", "sub-tab")]),
        }
    }
}

/// Row style for a table row, highlighted when selected.
fn row_style(is_selected: bool) -> Style {
    if is_selected {
        theme::table_selected()
    } else {
        theme::table_row()
    }
}

/// Style for a rule or zone name, bold on the selected row.
fn name_style(is_selected: bool) -> Style {
    Style::default()
        .fg(theme::accent_secondary())
        .add_modifier(if is_selected {
            Modifier::BOLD
        } else {
            Modifier::empty()
        })
}

/// The ✓/✗ cell for whether a rule is enabled.
fn enabled_cell(enabled: bool) -> Cell<'static> {
    let (glyph, color) = if enabled {
        ("✓", theme::success())
    } else {
        ("✗", theme::border_unfocused())
    };
    Cell::from(glyph).style(Style::default().fg(color))
}

/// A rule's position in its evaluation order, behind the selection arrow.
fn index_label(is_selected: bool, index: Option<String>) -> String {
    let prefix = if is_selected { "▸" } else { " " };
    format!("{prefix}{}", index.unwrap_or_else(|| "─".into()))
}
