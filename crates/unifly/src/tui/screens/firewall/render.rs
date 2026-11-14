mod detail;
mod policies;
mod rules;

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row};

use crate::tui::action::FirewallSubTab;
use crate::tui::theme;
use crate::tui::widgets::click::{self, Click, ClickLine, Hint};
use crate::tui::widgets::detail_grid::{render_empty, render_panel, split_detail};
use crate::tui::widgets::sub_tabs;

use super::FirewallScreen;
use super::reorder::is_fixed;

/// Rows a table keeps when the detail panel opens: its header and four
/// rules.
const MIN_TABLE_ROWS: u16 = 5;

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

        let labels = [
            format!("Policies ({})", self.policies.len()),
            format!("Zones ({})", self.zones.len()),
            format!("ACL Rules ({})", self.acl_rules.len()),
            format!("NAT ({})", self.nat_policies.len()),
        ];
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let strip = sub_tabs::sub_tab_strip(&labels, self.sub_tab_index());
        let line = strip.draw(layout[0], &mut self.hits.borrow_mut());
        frame.render_widget(Paragraph::new(line), layout[0]);

        if self.active_len() == 0 {
            render_empty(frame, layout[1], self.empty_reason());
        } else {
            let detail = self
                .detail_open
                .then(|| self.detail(layout[1].width.saturating_sub(2)))
                .flatten();
            let (table, panel) = match &detail {
                Some((_, lines)) => split_detail(layout[1], lines.len(), MIN_TABLE_ROWS),
                None => (layout[1], Rect::default()),
            };
            match self.sub_tab {
                FirewallSubTab::Policies => self.render_policies(frame, table),
                FirewallSubTab::Zones => self.render_zones(frame, table),
                FirewallSubTab::AclRules => self.render_acl_rules(frame, table),
                FirewallSubTab::NatPolicies => self.render_nat_policies(frame, table),
            }
            if let Some((title, lines)) = detail {
                render_panel(frame, panel, &title, lines);
            }
        }

        let line = self
            .hint_line()
            .draw(layout[2], &mut self.hits.borrow_mut());
        frame.render_widget(Paragraph::new(line), layout[2]);
    }

    /// Why the table on show has nothing in it.
    fn empty_reason(&self) -> &'static str {
        let Some(access) = self.access else {
            return "Waiting for the controller\u{2026}";
        };
        match self.sub_tab {
            FirewallSubTab::NatPolicies if !access.session => {
                "NAT rules need a direct controller session, which this profile does not \
                 have. Cloud profiles cannot read them."
            }
            FirewallSubTab::NatPolicies => "No NAT rules on this site.",
            _ if !access.integration => {
                "Policies, zones, and ACL rules come from the Integration API, which this \
                 profile cannot reach. Add an API key to it to see them."
            }
            FirewallSubTab::Policies => "No firewall policies on this site.",
            FirewallSubTab::Zones => "No firewall zones on this site.",
            FirewallSubTab::AclRules => "No ACL rules on this site.",
        }
    }

    /// The keys that work right now, for the row and mode on screen.
    fn hint_line(&self) -> ClickLine<'static, Click> {
        if self.is_drafting() {
            return click::key_hints(&[
                Hint::info("K/J", "move"),
                Hint::button("Enter", "apply", KeyCode::Enter),
                Hint::button("Esc", "put back", KeyCode::Esc),
            ]);
        }
        let tabs = Hint::info("h/l", "tab");
        if self.active_len() == 0 {
            return click::key_hints(&[tabs]);
        }
        let mut hints = vec![
            Hint::info("j/k", "navigate"),
            if self.detail_open {
                Hint::button("Esc", "close", KeyCode::Esc)
            } else {
                Hint::button("Enter", "detail", KeyCode::Enter)
            },
        ];
        let toggle = Hint::button("E", "on/off", click::char_key('E'));
        let delete = Hint::button("D", "delete", click::char_key('D'));
        match self.sub_tab {
            FirewallSubTab::Policies => {
                hints.extend([toggle, Hint::button("L", "log", click::char_key('L'))]);
                // A system-defined policy can be neither moved nor deleted.
                if self
                    .selected_policy()
                    .is_some_and(|policy| !is_fixed(&policy))
                {
                    hints.extend([
                        Hint::button("K", "up", click::char_key('K')),
                        Hint::button("J", "down", click::char_key('J')),
                        delete,
                    ]);
                }
            }
            FirewallSubTab::AclRules | FirewallSubTab::NatPolicies => {
                hints.extend([toggle, delete]);
            }
            FirewallSubTab::Zones => {}
        }
        hints.push(tabs);
        click::key_hints(&hints)
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

/// A rule's place in its evaluation order, counted from one, behind the
/// selection arrow.
fn position_label(is_selected: bool, position: usize) -> String {
    let prefix = if is_selected { "▸" } else { " " };
    format!("{prefix}{position}")
}

fn header_row(labels: &[&'static str]) -> Row<'static> {
    Row::new(
        labels
            .iter()
            .map(|label| Cell::from(*label).style(theme::table_header())),
    )
}
