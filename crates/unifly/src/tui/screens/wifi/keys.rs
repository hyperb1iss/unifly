//! The WiFi screen's key documentation: the hint bar for the current mode
//! and the full list for the help overlay. Both live here, next to each
//! other, so a key that changes is changed in both.

use crossterm::event::KeyCode;

use super::{OverviewPanel, WifiScreen};
use crate::tui::action::{WifiSortField, WifiSubTab};
use crate::tui::component::LIST_HELP;
use crate::tui::widgets::click::{self, Hint};

/// The sort field `s` moves to from `current` on `sub_tab`.
pub(super) fn next_sort_field(sub_tab: WifiSubTab, current: WifiSortField) -> WifiSortField {
    let cycle: &[WifiSortField] = match sub_tab {
        WifiSubTab::Overview => &[
            WifiSortField::Health,
            WifiSortField::Clients,
            WifiSortField::Channel,
            WifiSortField::Name,
        ],
        WifiSubTab::Clients => &[
            WifiSortField::Health,
            WifiSortField::Signal,
            WifiSortField::Name,
        ],
        WifiSubTab::Neighbors => &[
            WifiSortField::Signal,
            WifiSortField::Channel,
            WifiSortField::Security,
            WifiSortField::Name,
        ],
        WifiSubTab::Ssids | WifiSubTab::Roaming => return current,
    };
    let index = cycle.iter().position(|field| *field == current);
    cycle[index.map_or(0, |index| (index + 1) % cycle.len())]
}

impl WifiScreen {
    /// The keys worth showing for what is on screen right now, most useful
    /// first: the bar is cut at the panel's width.
    pub(super) fn hints(&self) -> Vec<Hint> {
        let mut hints = Vec::new();
        self.panel_hints(&mut hints);
        hints.push(Hint::button("h/l", "tab", click::char_key('l')));
        match self.sub_tab {
            WifiSubTab::Overview => self.overview_hints(&mut hints),
            WifiSubTab::Clients => self.clients_hints(&mut hints),
            WifiSubTab::Ssids => self.ssids_hints(&mut hints),
            WifiSubTab::Neighbors => {
                if !self.session_missing() {
                    hints.push(Hint::button("s", "sort", click::char_key('s')));
                    hints.push(Hint::button("f", "band", click::char_key('f')));
                }
            }
            WifiSubTab::Roaming => {
                if self.roam_target.is_some() && !self.session_missing() {
                    hints.push(Hint::button("r", "refresh", click::char_key('r')));
                }
            }
        }
        if self.focus_shown() && !self.panel_open() {
            hints.push(Hint::button("Esc", "all APs", KeyCode::Esc));
        }
        if self.active_len() > 1 {
            hints.push(Hint::info("j/k", "move"));
        }
        hints
    }

    /// Whether the active sub-tab has a panel open over or beside its list.
    pub(super) fn panel_open(&self) -> bool {
        match self.sub_tab {
            WifiSubTab::Overview => self.overview_panel != OverviewPanel::None,
            WifiSubTab::Clients => self.client_detail_open,
            WifiSubTab::Ssids => self.ssid_detail_open,
            WifiSubTab::Neighbors | WifiSubTab::Roaming => false,
        }
    }

    /// Whether the active sub-tab is narrowed to a focused AP and shows it.
    pub(super) fn focus_shown(&self) -> bool {
        self.focused_ap.is_some()
            && matches!(
                self.sub_tab,
                WifiSubTab::Overview | WifiSubTab::Clients | WifiSubTab::Neighbors
            )
    }

    /// The keys of the open panel, ahead of everything else.
    fn panel_hints(&self, hints: &mut Vec<Hint>) {
        if !self.panel_open() {
            return;
        }
        if self.channel_map_open() {
            hints.push(Hint::button("[/]", "band", click::char_key(']')));
        }
        hints.push(Hint::button("Esc", "close", KeyCode::Esc));
    }

    fn overview_hints(&self, hints: &mut Vec<Hint>) {
        let channels = Hint::button("c", "channels", click::char_key('c'));
        let Some(ap) = self.selected_ap() else {
            if !self.channel_map_open() {
                hints.push(channels);
            }
            return;
        };
        if self.overview_panel != OverviewPanel::Detail {
            hints.push(Hint::button("Enter", "detail", KeyCode::Enter));
        }
        let focused = self
            .focused_ap
            .as_ref()
            .is_some_and(|focused| focused.id == ap.id);
        hints.push(Hint::button(
            "Space",
            if focused { "unfocus" } else { "focus" },
            click::char_key(' '),
        ));
        if !self.channel_map_open() {
            hints.push(channels);
        }
        hints.push(Hint::button("s", "sort", click::char_key('s')));
        hints.push(Hint::button("o", "device", click::char_key('o')));
        hints.push(Hint::button("R", "restart", click::char_key('R')));
        hints.push(Hint::button("L", "locate", click::char_key('L')));
        if ap.firmware_updatable {
            hints.push(Hint::button("U", "upgrade", click::char_key('U')));
        }
    }

    fn clients_hints(&self, hints: &mut Vec<Hint>) {
        let Some(client) = self.selected_client() else {
            return;
        };
        if !self.client_detail_open {
            hints.push(Hint::button("Enter", "experience", KeyCode::Enter));
        }
        hints.push(Hint::button("o", "client", click::char_key('o')));
        hints.push(Hint::button("r", "roams", click::char_key('r')));
        hints.push(Hint::button("s", "sort", click::char_key('s')));
        hints.push(Hint::button(
            "b",
            if client.blocked { "unblock" } else { "block" },
            click::char_key('b'),
        ));
        hints.push(Hint::button("x", "kick", click::char_key('x')));
        hints.push(Hint::button("F", "forget", click::char_key('F')));
    }

    fn ssids_hints(&self, hints: &mut Vec<Hint>) {
        let Some(ssid) = self.selected_ssid() else {
            return;
        };
        if !self.ssid_detail_open {
            hints.push(Hint::button("Enter", "detail", KeyCode::Enter));
        }
        hints.push(Hint::button(
            "E",
            if ssid.enabled { "disable" } else { "enable" },
            click::char_key('E'),
        ));
        hints.push(Hint::button("N", "rename", click::char_key('N')));
        hints.push(Hint::button("P", "passphrase", click::char_key('P')));
        hints.push(Hint::button(
            "H",
            if ssid.hidden { "unhide" } else { "hide" },
            click::char_key('H'),
        ));
        hints.push(Hint::button("D", "delete", click::char_key('D')));
    }

    /// Every key the screen takes in its current sub-tab and mode, for the
    /// help overlay.
    pub(super) fn help_keys(&self) -> Vec<(&'static str, &'static str)> {
        let mut keys: Vec<(&'static str, &'static str)> = LIST_HELP.to_vec();
        keys.push(("h/l \u{2190}/\u{2192}", "Previous / next sub-tab"));
        match self.sub_tab {
            WifiSubTab::Overview => {
                keys.extend([
                    ("Enter", "Open or close the AP detail"),
                    ("Space", "Focus Clients and Neighbors on this AP"),
                    ("c", "Open or close the channel map"),
                ]);
                if self.channel_map_open() {
                    keys.push(("[/]", "Channel map band"));
                }
                keys.extend([
                    ("s", "Sort: health, clients, channel, name"),
                    ("o", "Open this AP on the Devices screen"),
                    ("R", "Restart the AP"),
                    ("L", "Flash or stop its locate LED"),
                    ("U", "Upgrade its firmware, when one is waiting"),
                ]);
                if self.overview_panel != OverviewPanel::None {
                    keys.push(("Esc", "Close the panel"));
                } else if self.focused_ap.is_some() {
                    keys.push(("Esc", "Clear the AP focus"));
                }
            }
            WifiSubTab::Clients => {
                keys.extend([
                    ("Enter", "Open or close WiFi experience"),
                    ("o", "Open this client on the Clients screen"),
                    ("r", "Show its roam history"),
                    ("s", "Sort: health, signal, name"),
                    ("b", "Block, or unblock a blocked client"),
                    ("x", "Disconnect (kick)"),
                    ("F", "Forget the client"),
                ]);
                if self.client_detail_open {
                    keys.push(("Esc", "Close the panel"));
                } else if self.focused_ap.is_some() {
                    keys.push(("Esc", "Clear the AP focus"));
                }
            }
            WifiSubTab::Ssids => {
                keys.extend([
                    ("Enter", "Open or close the SSID detail"),
                    ("E", "Enable or disable the SSID"),
                    ("N", "Rename it"),
                    ("P", "Change its passphrase"),
                    ("H", "Hide or announce its name"),
                    ("D", "Delete it"),
                ]);
                if self.ssid_detail_open {
                    keys.push(("Esc", "Close the panel"));
                }
            }
            WifiSubTab::Neighbors => {
                keys.extend([
                    ("s", "Sort: signal, channel, security, name"),
                    ("f  [/]", "Next / previous band"),
                ]);
                if self.focused_ap.is_some() {
                    keys.push(("Esc", "Clear the AP focus"));
                }
            }
            WifiSubTab::Roaming => keys.push(("r  Enter", "Refresh the history")),
        }
        keys
    }
}
