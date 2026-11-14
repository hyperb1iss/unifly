mod detail;
mod policies;
mod rules;

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Row};

use crate::tui::action::FirewallSubTab;
use crate::tui::theme;
use crate::tui::widgets::click::{self, Click, ClickLine, Hint};
use crate::tui::widgets::detail_grid::{render_empty, render_panel, split_detail};
use crate::tui::widgets::{sub_tabs, toolbar};

use super::FirewallScreen;
use super::reorder::Stage;

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

        let layout = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(inner);

        let labels = [
            format!("Policies ({})", self.policies.len()),
            format!("Zones ({})", self.zones.len()),
            format!("ACL Rules ({})", self.acl_rules.len()),
            format!("NAT ({})", self.nat_policies.len()),
        ];
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let strip = sub_tabs::sub_tab_strip(&labels, self.sub_tab_index());
        toolbar::render(
            frame,
            layout[0],
            strip,
            self.reorder_chip(),
            &mut self.hits.borrow_mut(),
        );

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
    }

    /// A policy move under way, for the right of the toolbar: how far the
    /// picked-up policy has gone, then that the controller is taking it.
    /// Until `Enter` the move is on screen only, which the list alone
    /// does not say.
    fn reorder_chip(&self) -> ClickLine<'static, Click> {
        let Some(reorder) = &self.reorder else {
            return ClickLine::default();
        };
        if reorder.stage != Stage::Draft {
            return toolbar::chip(None, "saving", "new order".into());
        }
        let places = reorder.steps.unsigned_abs();
        let moved = match reorder.steps {
            0 => "in place".to_owned(),
            ..0 => format!("{places} up"),
            _ => format!("{places} down"),
        };
        toolbar::chip(None, "moving", format!("{moved}, not saved"))
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

    /// What the status bar offers: the writes the selected row takes,
    /// behind the way into its detail or out of it. While a policy is
    /// picked up, only what moves, lands, or drops it.
    pub(in crate::tui::screens::firewall) fn status_hints(&self) -> Vec<(u8, Hint)> {
        let up = Hint::button("K", "up", click::char_key('K'));
        let down = Hint::button("J", "down", click::char_key('J'));
        if self.is_drafting() {
            return vec![
                (1, up),
                (1, down),
                (0, Hint::button("Enter", "apply", KeyCode::Enter)),
                (0, Hint::button("Esc", "put back", KeyCode::Esc)),
            ];
        }
        if self.active_len() == 0 {
            return Vec::new();
        }
        let mut hints = Vec::new();
        if !self.detail_open {
            hints.push((0, Hint::button("Enter", "detail", KeyCode::Enter)));
        }
        // A rule that is not an admin's takes no switch, move, or delete;
        // a built-in policy still takes logging.
        if let Some(enabled) = self.selected_enabled().filter(|_| self.can_toggle()) {
            let label = if enabled { "disable" } else { "enable" };
            hints.push((1, Hint::button("E", label, click::char_key('E'))));
        }
        if self.sub_tab == FirewallSubTab::Policies
            && let Some(policy) = self.selected_policy()
        {
            let label = if policy.logging_enabled {
                "log off"
            } else {
                "log on"
            };
            hints.push((4, Hint::button("L", label, click::char_key('L'))));
            if self.can_move(-1) {
                hints.push((3, up));
            }
            if self.can_move(1) {
                hints.push((3, down));
            }
        }
        if self.can_toggle() {
            hints.push((2, Hint::button("D", "delete", click::char_key('D'))));
        }
        if self.detail_open {
            hints.push((0, Hint::button("Esc", "close", KeyCode::Esc)));
        }
        hints
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
