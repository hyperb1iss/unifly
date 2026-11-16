//! The WiFi screen's key documentation: the status bar's actions for the
//! current mode and the full list for the help overlay. Both live here,
//! next to each other, so a key that changes is changed in both.

use crossterm::event::KeyCode;

use super::ssid_detail::Setting;
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

/// Rank of the sort hint: the arrow on the sorted column already says
/// what the order is, so the key is the first to leave a narrow bar.
const SORT_RANK: u8 = 6;

fn sort_hint() -> (u8, Hint) {
    (SORT_RANK, Hint::button("s", "sort", click::char_key('s')))
}

impl WifiScreen {
    /// What the status bar offers for the active sub-tab and selection,
    /// ranked for a narrow bar: the way into a panel first, the way out
    /// last. Moving, switching tabs, and the band keys on the toolbar are
    /// in the help.
    pub(super) fn status_hints(&self) -> Vec<(u8, Hint)> {
        let mut hints = Vec::new();
        match self.sub_tab {
            WifiSubTab::Overview => self.overview_hints(&mut hints),
            WifiSubTab::Clients => self.clients_hints(&mut hints),
            WifiSubTab::Ssids => self.ssids_hints(&mut hints),
            WifiSubTab::Neighbors => {
                if !self.session_missing() {
                    hints.push(sort_hint());
                }
            }
            WifiSubTab::Roaming => {
                if self.roam_target.is_some() && !self.session_missing() {
                    hints.push((0, Hint::button("r", "refresh", click::char_key('r'))));
                }
            }
        }
        if self.panel_open() {
            hints.push((0, Hint::button("Esc", "close", KeyCode::Esc)));
        } else if self.focus_shown() {
            hints.push((1, Hint::button("Esc", "all APs", KeyCode::Esc)));
        }
        hints
    }

    /// Whether the active sub-tab has a panel open beside or under its
    /// list.
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

    fn overview_hints(&self, hints: &mut Vec<(u8, Hint)>) {
        let channels = (2, Hint::button("c", "channels", click::char_key('c')));
        let Some(ap) = self.selected_ap() else {
            if !self.channel_map_open() {
                hints.push(channels);
            }
            return;
        };
        if self.overview_panel != OverviewPanel::Detail {
            hints.push((0, Hint::button("Enter", "detail", KeyCode::Enter)));
        }
        let focused = self
            .focused_ap
            .as_ref()
            .is_some_and(|focused| focused.id == ap.id);
        hints.push((
            1,
            Hint::button(
                "Space",
                if focused { "unfocus" } else { "focus" },
                click::char_key(' '),
            ),
        ));
        if !self.channel_map_open() {
            hints.push(channels);
        }
        hints.push((4, Hint::button("o", "device", click::char_key('o'))));
        hints.push((3, Hint::button("R", "restart", click::char_key('R'))));
        hints.push((4, Hint::button("L", "locate", click::char_key('L'))));
        if ap.firmware_updatable {
            hints.push((3, Hint::button("U", "upgrade", click::char_key('U'))));
        }
        hints.push(sort_hint());
    }

    fn clients_hints(&self, hints: &mut Vec<(u8, Hint)>) {
        let Some(client) = self.selected_client() else {
            return;
        };
        if !self.client_detail_open {
            hints.push((0, Hint::button("Enter", "experience", KeyCode::Enter)));
        }
        hints.push((2, Hint::button("r", "roams", click::char_key('r'))));
        hints.push((3, Hint::button("o", "client", click::char_key('o'))));
        hints.push((
            1,
            Hint::button(
                "b",
                if client.blocked { "unblock" } else { "block" },
                click::char_key('b'),
            ),
        ));
        hints.push((1, Hint::button("x", "kick", click::char_key('x'))));
        hints.push((4, Hint::button("F", "forget", click::char_key('F'))));
        hints.push(sort_hint());
    }

    fn ssids_hints(&self, hints: &mut Vec<(u8, Hint)>) {
        let Some(ssid) = self.selected_ssid() else {
            return;
        };
        if !self.ssid_detail_open {
            hints.push((0, Hint::button("Enter", "detail", KeyCode::Enter)));
        }
        hints.push((
            1,
            Hint::button(
                "E",
                if ssid.enabled { "disable" } else { "enable" },
                click::char_key('E'),
            ),
        ));
        hints.push((2, Hint::button("N", "rename", click::char_key('N'))));
        // The key still answers on an SSID without one, with the reason.
        if self.ssid_passphrase_refusal(ssid).is_none() {
            hints.push((2, Hint::button("P", "passphrase", click::char_key('P'))));
        }
        hints.push((
            3,
            Hint::button(
                "H",
                match self.ssid_setting(ssid, |ssid| ssid.hidden) {
                    Setting::On => "unhide",
                    Setting::Off => "hide",
                    // The key reads the controller before it asks which.
                    _ => "hide/unhide",
                },
                click::char_key('H'),
            ),
        ));
        hints.push((4, Hint::button("D", "delete", click::char_key('D'))));
    }

    /// Every key the screen takes in its current sub-tab and mode, for the
    /// help overlay.
    pub(super) fn help_keys(&self) -> Vec<(&'static str, &'static str)> {
        let mut keys: Vec<(&'static str, &'static str)> = LIST_HELP.to_vec();
        keys.push(("h/l \u{2190}/\u{2192}", "Previous / next sub-tab"));
        match self.sub_tab {
            WifiSubTab::Overview => {
                // The keys that act on an AP need one under the cursor.
                let ap = self.selected_ap().is_some();
                if ap {
                    keys.extend([("Enter", "AP detail"), ("Space", "Focus tabs on this AP")]);
                }
                keys.push(("c", "Channel map"));
                if self.channel_map_open() {
                    keys.push(("[/]", "Channel map band"));
                }
                keys.push(("s", "Cycle the sort"));
                if ap {
                    keys.extend([
                        ("o", "Open on Devices screen"),
                        ("R", "Restart the AP"),
                        ("L", "Locate LED on / off"),
                        ("U", "Upgrade firmware"),
                    ]);
                }
                if self.overview_panel != OverviewPanel::None {
                    keys.push(("Esc", "Close the panel"));
                } else if self.focused_ap.is_some() {
                    keys.push(("Esc", "Clear the AP focus"));
                }
            }
            WifiSubTab::Clients => {
                // Without a session the table has no clients to act on.
                if self.selected_client().is_some() {
                    keys.extend([
                        ("Enter", "WiFi experience"),
                        ("o", "Open on Clients screen"),
                        ("r", "Roam history"),
                        ("s", "Cycle the sort"),
                        ("b", "Block / unblock"),
                        ("x", "Disconnect (kick)"),
                        ("F", "Forget the client"),
                    ]);
                }
                if self.client_detail_open {
                    keys.push(("Esc", "Close the panel"));
                } else if self.focused_ap.is_some() {
                    keys.push(("Esc", "Clear the AP focus"));
                }
            }
            WifiSubTab::Ssids => {
                // SSIDs come over the Integration API or not at all.
                if self.selected_ssid().is_some() {
                    keys.extend([
                        ("Enter", "SSID detail"),
                        ("E", "Enable / disable"),
                        ("N", "Rename"),
                        ("P", "Change passphrase"),
                        ("H", "Hide / announce name"),
                        ("D", "Delete"),
                    ]);
                }
                if self.ssid_detail_open {
                    keys.push(("Esc", "Close the panel"));
                }
            }
            WifiSubTab::Neighbors => {
                if !self.session_missing() {
                    keys.extend([("s", "Cycle the sort"), ("f  [/]", "Next / previous band")]);
                }
                if self.focused_ap.is_some() {
                    keys.push(("Esc", "Clear the AP focus"));
                }
            }
            WifiSubTab::Roaming => {
                if self.roam_target.is_some() && !self.session_missing() {
                    keys.push(("r  Enter", "Refresh the history"));
                }
            }
        }
        keys
    }
}
