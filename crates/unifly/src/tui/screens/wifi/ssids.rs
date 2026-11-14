//! The SSIDs sub-tab's commands: enable, hide, rename, change the
//! passphrase, and delete. Each is one write to one broadcast, asked about
//! first, since every one of them has the access points reprovision.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent};
use unifly_api::model::{WifiBroadcast, WifiSecurityMode};
use unifly_api::{EntityId, UpdateWifiBroadcastRequest};

use super::WifiScreen;
use crate::tui::action::{Action, ConfirmRequest, Notice, Notification, PromptRequest, SsidChange};

/// Longest network name the standard allows, in bytes.
const SSID_MAX_BYTES: usize = 32;
/// WPA passphrase length, in bytes of printable ASCII.
const PASSPHRASE_LENGTH: std::ops::RangeInclusive<usize> = 8..=63;

/// What the open prompt was asked for, and for which SSID. Not `Debug`:
/// the second passphrase prompt holds the first entry.
#[derive(Clone, PartialEq, Eq)]
pub(super) enum SsidPrompt {
    Rename(EntityId),
    Passphrase(EntityId),
    /// The passphrase again. It is typed hidden, so a slip in one entry
    /// would otherwise lock every client out behind a passphrase nobody
    /// knows.
    ConfirmPassphrase {
        id: EntityId,
        first: String,
    },
}

/// Why `text` cannot be a WPA passphrase, when it cannot.
fn passphrase_problem(text: &str) -> Option<String> {
    if !text.bytes().all(|byte| (0x20..=0x7e).contains(&byte)) {
        return Some(
            "A passphrase is plain ASCII: letters, digits, punctuation, and spaces".to_owned(),
        );
    }
    (!PASSPHRASE_LENGTH.contains(&text.len())).then(|| {
        format!(
            "A passphrase is 8 to 63 characters; that one is {}",
            text.len()
        )
    })
}

fn update(ssid: &WifiBroadcast, update: UpdateWifiBroadcastRequest, notice: Notice) -> Action {
    Action::WifiSsidWrite {
        id: ssid.id.clone(),
        change: SsidChange::Update(Box::new(update)),
        notice,
    }
}

fn warn(message: impl Into<String>) -> Action {
    Action::Notify(Notification::warning(message))
}

/// Who a change cuts off, for the cost line of a confirmation. Without a
/// count nobody can be promised to be unaffected.
fn clients_phrase(count: Option<usize>) -> String {
    match count {
        None => "every client on it is disconnected".to_owned(),
        Some(0) => "nobody is on it right now".to_owned(),
        Some(1) => "its 1 client is disconnected".to_owned(),
        Some(count) => format!("its {count} clients are disconnected"),
    }
}

impl WifiScreen {
    pub(super) fn handle_ssid_key(&mut self, key: KeyEvent) -> Option<Action> {
        let ssid = Arc::clone(self.selected_ssid()?);
        match key.code {
            KeyCode::Char('E') => Some(self.toggle_ssid_enabled(&ssid)),
            // Which way this goes is read from the controller first: the
            // list does not say whether an SSID hides its name.
            KeyCode::Char('H') => Some(Action::RequestSsidVisibilityToggle(ssid.id.clone())),
            KeyCode::Char('N') => Some(self.prompt_ssid_rename(&ssid, None)),
            KeyCode::Char('P') => Some(self.prompt_ssid_passphrase(&ssid, None)),
            KeyCode::Char('D') => Some(self.delete_ssid(&ssid)),
            _ => None,
        }
    }

    /// Both directions ask first. Disabling drops every client on the
    /// network, and enabling puts a network on the air that was off for a
    /// reason.
    fn toggle_ssid_enabled(&self, ssid: &WifiBroadcast) -> Action {
        let name = &ssid.name;
        let request = |enabled| UpdateWifiBroadcastRequest {
            enabled: Some(enabled),
            ..Default::default()
        };
        if ssid.enabled {
            let run = update(
                ssid,
                request(false),
                Notice::new(
                    format!("Disabling {name}\u{2026}"),
                    format!("Disabled {name}"),
                ),
            );
            return Action::ShowConfirm(
                ConfirmRequest::new(format!("Disable {name}?"), run).detail(format!(
                    "Access points reprovision and it stops broadcasting: {}.",
                    clients_phrase(self.ssid_client_count(name))
                )),
            );
        }
        let run = update(
            ssid,
            request(true),
            Notice::new(
                format!("Enabling {name}\u{2026}"),
                format!("Enabled {name}"),
            ),
        );
        let open = if ssid.security == WifiSecurityMode::Open {
            " It is an open network: anyone in range can join."
        } else {
            ""
        };
        Action::ShowConfirm(
            ConfirmRequest::new(format!("Enable {name}?"), run).detail(format!(
                "Access points reprovision and start broadcasting it, so clients on other \
                 SSIDs may drop for a moment.{open}"
            )),
        )
    }

    fn prompt_ssid_rename(&mut self, ssid: &WifiBroadcast, typed: Option<&str>) -> Action {
        self.ssid_prompt = Some(SsidPrompt::Rename(ssid.id.clone()));
        Action::OpenPrompt(PromptRequest {
            title: format!("Rename {}", ssid.name),
            label: "Network name (SSID)".to_owned(),
            initial: typed.unwrap_or(&ssid.name).to_owned(),
            hint: Some(
                "Up to 32 bytes: 32 plain characters, fewer with accents or emoji. Clients must \
                 rejoin under the new name."
                    .to_owned(),
            ),
            secret: false,
        })
    }

    /// Why `ssid` has no passphrase to change, when it has none.
    pub(super) fn ssid_passphrase_refusal(&self, ssid: &WifiBroadcast) -> Option<Notification> {
        let name = &ssid.name;
        let reason = match ssid.security {
            WifiSecurityMode::Open => Some("it is an open network"),
            WifiSecurityMode::Wpa2Enterprise
            | WifiSecurityMode::Wpa3Enterprise
            | WifiSecurityMode::Wpa2Wpa3Enterprise => Some("clients sign in through RADIUS"),
            _ => None,
        };
        if let Some(reason) = reason {
            return Some(Notification::info(format!(
                "{name} has no passphrase: {reason}"
            )));
        }
        // One passphrase written beside per-network keys could replace
        // them, be ignored, or be rejected, so it is not sent.
        let keys = self.ssid_preshared_keys(ssid);
        (keys > 0).then(|| {
            Notification::warning(format!(
                "{name} gives {} its own key, so it has no single passphrase to change. Edit \
                 its keys in the UniFi console.",
                if keys == 1 {
                    "1 network".to_owned()
                } else {
                    format!("each of {keys} networks")
                }
            ))
        })
    }

    fn prompt_ssid_passphrase(&mut self, ssid: &WifiBroadcast, typed: Option<&str>) -> Action {
        if let Some(refusal) = self.ssid_passphrase_refusal(ssid) {
            return Action::Notify(refusal);
        }
        self.ssid_prompt = Some(SsidPrompt::Passphrase(ssid.id.clone()));
        Action::OpenPrompt(PromptRequest {
            title: format!("Passphrase for {}", ssid.name),
            label: "New passphrase".to_owned(),
            initial: typed.unwrap_or_default().to_owned(),
            hint: Some(
                "8 to 63 ASCII characters. It is typed hidden, so it is asked for twice."
                    .to_owned(),
            ),
            secret: true,
        })
    }

    /// Ask for the passphrase a second time, holding the first entry
    /// until the answer comes.
    fn prompt_ssid_passphrase_again(&mut self, ssid: &WifiBroadcast, first: &str) -> Action {
        self.ssid_prompt = Some(SsidPrompt::ConfirmPassphrase {
            id: ssid.id.clone(),
            first: first.to_owned(),
        });
        Action::OpenPrompt(PromptRequest {
            title: format!("Passphrase for {}", ssid.name),
            label: "Type it again".to_owned(),
            initial: String::new(),
            hint: Some("It must match the first entry.".to_owned()),
            secret: true,
        })
    }

    fn delete_ssid(&self, ssid: &WifiBroadcast) -> Action {
        let name = &ssid.name;
        let run = Action::WifiSsidWrite {
            id: ssid.id.clone(),
            change: SsidChange::Delete,
            notice: Notice::new(
                format!("Deleting {name}\u{2026}"),
                format!("Deleted {name}"),
            ),
        };
        Action::ShowConfirm(
            ConfirmRequest::new(format!("Delete {name}?"), run).detail(format!(
                "The network is removed for good: {}.",
                clients_phrase(self.ssid_client_count(name))
            )),
        )
    }

    /// The answer to the open prompt. A value that cannot be used says why
    /// and reopens the prompt on what was typed; an empty one cancels.
    pub(super) fn ssid_prompt_submitted(&mut self, text: &str) -> Option<Action> {
        let prompt = self.ssid_prompt.take()?;
        if text.is_empty() {
            return None;
        }
        let (SsidPrompt::Rename(id)
        | SsidPrompt::Passphrase(id)
        | SsidPrompt::ConfirmPassphrase { id, .. }) = &prompt;
        let Some(ssid) = self.ssids.iter().find(|ssid| &ssid.id == id).cloned() else {
            return Some(warn("That SSID is no longer on the controller"));
        };
        match prompt {
            SsidPrompt::Rename(_) => self.submit_ssid_rename(&ssid, text),
            SsidPrompt::Passphrase(_) => Some(self.submit_ssid_passphrase(&ssid, text)),
            SsidPrompt::ConfirmPassphrase { first, .. } => {
                Some(self.confirm_ssid_passphrase(&ssid, &first, text))
            }
        }
    }

    fn submit_ssid_rename(&mut self, ssid: &WifiBroadcast, text: &str) -> Option<Action> {
        if text == ssid.name {
            return None;
        }
        if text.len() > SSID_MAX_BYTES {
            self.send(warn(format!(
                "A network name is at most {SSID_MAX_BYTES} bytes; that one is {}",
                text.len()
            )));
            return Some(self.prompt_ssid_rename(ssid, Some(text)));
        }
        if self.ssids.iter().any(|other| other.name == text) {
            self.send(warn(format!("Another SSID is already named {text}")));
            return Some(self.prompt_ssid_rename(ssid, Some(text)));
        }
        let old = &ssid.name;
        let run = update(
            ssid,
            UpdateWifiBroadcastRequest {
                name: Some(text.to_owned()),
                ..Default::default()
            },
            Notice::new(
                format!("Renaming {old} to {text}\u{2026}"),
                format!("Renamed {old} to {text}"),
            ),
        );
        Some(Action::ShowConfirm(
            ConfirmRequest::new(format!("Rename {old} to {text}?"), run)
                .detail("Clients drop off and must rejoin under the new name."),
        ))
    }

    fn submit_ssid_passphrase(&mut self, ssid: &WifiBroadcast, text: &str) -> Action {
        if let Some(problem) = passphrase_problem(text) {
            self.send(warn(problem));
            return self.prompt_ssid_passphrase(ssid, Some(text));
        }
        self.prompt_ssid_passphrase_again(ssid, text)
    }

    /// The second entry. Only a passphrase typed the same way twice is
    /// offered for writing; a mismatch starts over from an empty prompt.
    fn confirm_ssid_passphrase(
        &mut self,
        ssid: &WifiBroadcast,
        first: &str,
        again: &str,
    ) -> Action {
        if again != first {
            self.send(warn(
                "The two entries do not match, so nothing was changed. Type the passphrase again.",
            ));
            return self.prompt_ssid_passphrase(ssid, None);
        }
        let name = &ssid.name;
        let run = update(
            ssid,
            UpdateWifiBroadcastRequest {
                passphrase: Some(first.to_owned()),
                ..Default::default()
            },
            Notice::new(
                format!("Changing the passphrase for {name}\u{2026}"),
                format!("Changed the passphrase for {name}"),
            ),
        );
        Action::ShowConfirm(
            ConfirmRequest::new(format!("Change the passphrase for {name}?"), run).detail(
                "Access points reprovision, and every client on it must enter the new one to \
                 reconnect.",
            ),
        )
    }

    /// Queue an action beside the one being returned.
    fn send(&self, action: Action) {
        if let Some(tx) = &self.action_tx {
            let _ = tx.send(action);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crossterm::event::{KeyCode, KeyEvent};
    use serde_json::json;
    use unifly_api::model::WifiBroadcast;

    use super::{SsidPrompt, passphrase_problem};
    use crate::tui::action::{Access, Action, SsidChange, WifiSubTab};
    use crate::tui::component::Component;
    use crate::tui::screens::wifi::WifiScreen;
    use crate::tui::table::render_lines;

    /// An SSID as the controller lists it: the summary, with none of the
    /// settings only the full record carries.
    fn ssid(id: u128, name: &str, security: &str) -> Arc<WifiBroadcast> {
        Arc::new(
            serde_json::from_value(json!({
                "id": uuid::Uuid::from_u128(id).to_string(), "name": name,
                "enabled": true, "broadcast_type": "Standard", "security": security,
                "network_id": uuid::Uuid::from_u128(100).to_string(),
                "frequencies_ghz": [2.4, 5.0], "hotspot_enabled": false, "origin": null
            }))
            .expect("SSID fixture should deserialize"),
        )
    }

    /// The full record of `summary`, as its detail read returns it.
    fn record(summary: &WifiBroadcast, hidden: bool) -> Action {
        let mut full = summary.clone();
        full.hidden = Some(hidden);
        full.client_isolation = Some(true);
        full.band_steering = Some(false);
        full.mlo_enabled = Some(false);
        full.fast_roaming = Some(true);
        Action::WifiBroadcastDetailLoaded {
            id: full.id.clone(),
            detail: Arc::new(full),
        }
    }

    fn ssid_screen() -> WifiScreen {
        let mut screen = WifiScreen::new();
        screen.focused = true;
        screen.apply_action(&Action::AccessUpdated(Access {
            integration: true,
            session: false,
            live_events: false,
        }));
        screen.apply_action(&Action::WifiBroadcastsUpdated(Arc::new(vec![
            ssid(2, "Studio", "Wpa2Wpa3Personal"),
            ssid(1, "Guest", "Open"),
        ])));
        screen.apply_action(&Action::WifiSubTab(WifiSubTab::Ssids));
        screen
    }

    fn draw(screen: &WifiScreen) -> String {
        render_lines(120, 24, |frame, area| screen.render(frame, area)).join("\n")
    }

    /// What the table's Hidden column says for the SSID `name`, with the
    /// table at its full width.
    fn hidden_cell(screen: &mut WifiScreen, name: &str) -> String {
        let panel = std::mem::replace(&mut screen.ssid_detail_open, false);
        let drawn = draw(screen);
        screen.ssid_detail_open = panel;
        drawn
            .lines()
            .find(|line| line.contains(name) && line.contains("GHz"))
            .unwrap_or_else(|| panic!("no row for {name}"))
            .trim_end_matches(|ch: char| ch.is_whitespace() || ch == '\u{2502}')
            .rsplit(' ')
            .next()
            .unwrap_or_default()
            .to_owned()
    }

    fn press(screen: &mut WifiScreen, key: char) -> Option<Action> {
        screen.handle_key_input(KeyEvent::from(KeyCode::Char(key)))
    }

    fn update_of(action: &Action) -> &unifly_api::UpdateWifiBroadcastRequest {
        match action {
            Action::WifiSsidWrite {
                change: SsidChange::Update(update),
                ..
            } => update,
            other => panic!("not an SSID update: {other:?}"),
        }
    }

    fn confirmed(action: Option<Action>) -> (String, Action) {
        match action {
            Some(Action::ShowConfirm(confirm)) => (confirm.prompt, *confirm.action),
            other => panic!("expected a confirmation, got {other:?}"),
        }
    }

    #[test]
    fn the_table_lists_each_ssid_with_its_state_and_network() {
        let mut screen = ssid_screen();
        let network: unifly_api::Network = serde_json::from_value(json!({
            "id": uuid::Uuid::from_u128(100).to_string(), "name": "Studio LAN",
            "enabled": true, "management": null, "purpose": null, "is_default": false,
            "vlan_id": 20, "subnet": null, "gateway_ip": null, "dhcp": null,
            "ipv6_enabled": false, "ipv6_mode": null, "ipv6_prefix": null,
            "dhcpv6_enabled": false, "slaac_enabled": false, "ntp_server": null,
            "pxe_enabled": false, "tftp_server": null, "firewall_zone_id": null,
            "isolation_enabled": false, "internet_access_enabled": true,
            "mdns_forwarding_enabled": false, "cellular_backup_enabled": false,
            "origin": null
        }))
        .expect("network fixture should deserialize");
        screen.apply_action(&Action::NetworksUpdated(Arc::new(vec![Arc::new(network)])));

        let text = render_lines(120, 12, |frame, area| screen.render(frame, area)).join("\n");
        for expected in [
            "Guest",
            "Studio",
            "Open",
            "WPA2/WPA3",
            "2.4 · 5 GHz",
            "On",
            "Studio LAN",
        ] {
            assert!(text.contains(expected), "missing {expected:?}:\n{text}");
        }
    }

    #[test]
    fn disabling_and_enabling_both_ask_first_and_say_what_it_costs() {
        let mut screen = ssid_screen();
        let Some(Action::ShowConfirm(disable)) = press(&mut screen, 'E') else {
            panic!("E asks before disabling");
        };
        assert_eq!(disable.prompt, "Disable Guest?");
        assert_eq!(update_of(&disable.action).enabled, Some(false));
        let cost = disable.detail.as_deref().unwrap_or_default();
        assert!(cost.contains("reprovision"), "{cost}");

        let mut off = (*ssid(1, "Guest", "Open")).clone();
        off.enabled = false;
        screen.apply_action(&Action::WifiBroadcastsUpdated(Arc::new(vec![Arc::new(
            off,
        )])));
        let Some(Action::ShowConfirm(enable)) = press(&mut screen, 'E') else {
            panic!("E asks before enabling: one key must not put a network on the air");
        };
        assert_eq!(enable.prompt, "Enable Guest?");
        assert_eq!(update_of(&enable.action).enabled, Some(true));
        let cost = enable.detail.as_deref().unwrap_or_default();
        assert!(
            cost.contains("reprovision") && cost.contains("anyone in range can join"),
            "an open network says so: {cost}"
        );
    }

    #[test]
    fn hiding_writes_nothing_by_itself_and_asks_the_controller_which_way_to_go() {
        let mut screen = ssid_screen();
        let guest = ssid(1, "Guest", "Open").id.clone();
        // Whatever the screen last drew, the direction is not its to pick.
        for known in [None, Some(false), Some(true)] {
            if let Some(hidden) = known {
                screen.apply_action(&record(&ssid(1, "Guest", "Open"), hidden));
            }
            let asked = press(&mut screen, 'H');
            assert!(
                matches!(&asked, Some(Action::RequestSsidVisibilityToggle(id)) if *id == guest),
                "{asked:?}"
            );
        }
    }

    #[test]
    fn a_setting_the_list_leaves_out_is_drawn_as_unread_and_never_as_off() {
        let mut screen = ssid_screen();
        screen.handle_key_input(KeyEvent::from(KeyCode::Enter));
        let before = draw(&screen);
        assert!(before.contains("Hidden      \u{2026}"), "{before}");
        assert!(before.contains("Isolation   \u{2026}"), "{before}");
        assert!(!before.contains("Hidden      no"), "{before}");
        assert_eq!(hidden_cell(&mut screen, "Guest"), "\u{2026}");

        screen.apply_action(&record(&ssid(1, "Guest", "Open"), true));
        let read = draw(&screen);
        assert!(read.contains("Hidden      yes"), "{read}");
        assert!(read.contains("Isolation   yes"), "{read}");
        assert!(read.contains("Steering    no"), "{read}");
        assert!(read.contains("Fast roam   yes"), "{read}");
        assert_eq!(hidden_cell(&mut screen, "Guest"), "hidden");
        assert_eq!(
            hidden_cell(&mut screen, "Studio"),
            "\u{2026}",
            "the other SSID is still unread"
        );
    }

    #[test]
    fn a_record_that_cannot_be_read_says_so_instead_of_guessing() {
        let mut screen = ssid_screen();
        screen.handle_key_input(KeyEvent::from(KeyCode::Enter));
        screen.apply_action(&Action::WifiBroadcastDetailFailed {
            id: ssid(1, "Guest", "Open").id.clone(),
            error: "HTTP 503".into(),
        });
        let shown = draw(&screen);
        assert!(shown.contains("Hidden      unknown"), "{shown}");
        assert!(shown.contains("Settings unavailable: HTTP 503"), "{shown}");
        assert_eq!(hidden_cell(&mut screen, "Guest"), "?");
    }

    #[test]
    fn the_tab_reads_each_ssid_in_full_once_the_selected_one_first() {
        let mut screen = ssid_screen();
        press(&mut screen, 'j');
        let (guest, studio) = (
            ssid(1, "Guest", "Open").id.clone(),
            ssid(2, "Studio", "Wpa2Wpa3Personal").id.clone(),
        );

        let first = screen.apply_action(&Action::Tick);
        assert!(
            matches!(&first, Some(Action::RequestWifiBroadcastDetail(id)) if *id == studio),
            "the selected SSID is read first: {first:?}"
        );
        let second = screen.apply_action(&Action::Tick);
        assert!(
            matches!(&second, Some(Action::RequestWifiBroadcastDetail(id)) if *id == guest),
            "{second:?}"
        );
        assert!(
            screen.apply_action(&Action::Tick).is_none(),
            "a read in flight is not asked for again"
        );

        screen.apply_action(&record(&ssid(1, "Guest", "Open"), false));
        screen.apply_action(&record(&ssid(2, "Studio", "Wpa2Wpa3Personal"), false));
        assert!(
            screen.apply_action(&Action::Tick).is_none(),
            "a fresh record is kept"
        );
    }

    #[test]
    fn a_record_is_read_again_after_a_write_or_when_the_list_disagrees() {
        let mut screen = ssid_screen();
        let guest = ssid(1, "Guest", "Open");
        screen.apply_action(&record(&guest, false));
        screen.apply_action(&record(&ssid(2, "Studio", "Wpa2Wpa3Personal"), false));
        assert!(screen.apply_action(&Action::Tick).is_none());

        screen.apply_action(&Action::WifiSsidWriteSettled(guest.id.clone()));
        let after_write = screen.apply_action(&Action::Tick);
        assert!(
            matches!(&after_write, Some(Action::RequestWifiBroadcastDetail(id)) if *id == guest.id),
            "{after_write:?}"
        );
        screen.apply_action(&record(&guest, true));
        assert!(screen.apply_action(&Action::Tick).is_none());

        // Something else disabled it: the record on hand is out of date.
        let mut off = (*guest).clone();
        off.enabled = false;
        screen.apply_action(&Action::WifiBroadcastsUpdated(Arc::new(vec![
            Arc::new(off),
            ssid(2, "Studio", "Wpa2Wpa3Personal"),
        ])));
        let after_change = screen.apply_action(&Action::Tick);
        assert!(
            matches!(&after_change, Some(Action::RequestWifiBroadcastDetail(id)) if *id == guest.id),
            "{after_change:?}"
        );
    }

    #[test]
    fn nothing_is_read_off_the_tab_or_without_the_integration_api() {
        let mut screen = ssid_screen();
        screen.apply_action(&Action::WifiSubTab(WifiSubTab::Overview));
        assert!(screen.apply_action(&Action::Tick).is_none());

        let mut screen = ssid_screen();
        screen.apply_action(&Action::AccessUpdated(Access {
            integration: false,
            session: true,
            live_events: true,
        }));
        assert!(screen.apply_action(&Action::Tick).is_none());
    }

    #[test]
    fn the_hide_hint_names_a_direction_only_once_it_is_known() {
        let hint = |screen: &WifiScreen| {
            draw(screen)
                .lines()
                .rev()
                .find(|line| line.contains("H "))
                .unwrap_or_default()
                .to_owned()
        };
        let mut screen = ssid_screen();
        assert!(hint(&screen).contains("H hide/unhide"), "{}", hint(&screen));
        screen.apply_action(&record(&ssid(1, "Guest", "Open"), true));
        assert!(hint(&screen).contains("H unhide"), "{}", hint(&screen));
        screen.apply_action(&record(&ssid(1, "Guest", "Open"), false));
        assert!(hint(&screen).contains("H hide "), "{}", hint(&screen));
    }

    #[test]
    fn renaming_prompts_with_the_name_then_confirms_the_new_one() {
        let mut screen = ssid_screen();
        let prompt = press(&mut screen, 'N');
        assert!(
            matches!(&prompt, Some(Action::OpenPrompt(request)) if request.initial == "Guest" && !request.secret),
            "{prompt:?}"
        );

        let (question, run) =
            confirmed(screen.apply_action(&Action::PromptSubmitted("Visitors".into())));
        assert_eq!(question, "Rename Guest to Visitors?");
        assert_eq!(update_of(&run).name.as_deref(), Some("Visitors"));
    }

    #[test]
    fn a_name_already_taken_reopens_the_prompt_on_what_was_typed() {
        let mut screen = ssid_screen();
        press(&mut screen, 'N');
        let again = screen.apply_action(&Action::PromptSubmitted("Studio".into()));
        assert!(
            matches!(&again, Some(Action::OpenPrompt(request)) if request.initial == "Studio"),
            "{again:?}"
        );
    }

    fn submit(screen: &mut WifiScreen, text: &str) -> Option<Action> {
        screen.apply_action(&Action::PromptSubmitted(text.to_owned()))
    }

    fn prompt_of(action: Option<&Action>) -> &crate::tui::action::PromptRequest {
        match action {
            Some(Action::OpenPrompt(request)) => request,
            other => panic!("expected a prompt, got {other:?}"),
        }
    }

    #[test]
    fn a_passphrase_is_typed_twice_and_only_a_match_is_offered_for_writing() {
        let mut screen = ssid_screen();
        press(&mut screen, 'j');
        let first = press(&mut screen, 'P');
        let request = prompt_of(first.as_ref());
        assert!(request.secret && request.label == "New passphrase");
        let hint = request.hint.as_deref().unwrap_or_default();
        assert!(hint.contains("asked for twice"), "{hint}");
        assert!(
            !hint.contains("cannot be read back"),
            "the controller does return it: {hint}"
        );

        let second = submit(&mut screen, "correct horse");
        let request = prompt_of(second.as_ref());
        assert!(request.secret && request.initial.is_empty());
        assert_eq!(request.label, "Type it again");

        let (question, run) = confirmed(submit(&mut screen, "correct horse"));
        assert_eq!(question, "Change the passphrase for Studio?");
        assert_eq!(update_of(&run).passphrase.as_deref(), Some("correct horse"));
        assert!(screen.ssid_prompt.is_none(), "the first entry is not kept");
    }

    #[test]
    fn entries_that_differ_write_nothing_and_start_over_empty() {
        let mut screen = ssid_screen();
        press(&mut screen, 'j');
        press(&mut screen, 'P');
        submit(&mut screen, "correct horse");

        let again = submit(&mut screen, "correct hrose");
        let request = prompt_of(again.as_ref());
        assert_eq!(request.label, "New passphrase", "back to the first prompt");
        assert!(request.initial.is_empty(), "neither entry is carried over");
        assert!(
            matches!(&screen.ssid_prompt, Some(SsidPrompt::Passphrase(_))),
            "the mismatched entry is dropped"
        );
    }

    #[test]
    fn a_passphrase_is_held_to_8_to_63_printable_ascii_bytes() {
        for (text, problem) in [
            ("short", Some("that one is 5")),
            ("exactly8", None),
            (&"x".repeat(63), None),
            (&"x".repeat(64), Some("that one is 64")),
            // Eight characters, sixteen bytes: the controller would refuse it.
            ("pässwörd", Some("plain ASCII")),
            ("tab\tin the middle", Some("plain ASCII")),
            (" spaces at both ends ", None),
        ] {
            match (passphrase_problem(text), problem) {
                (None, None) => {}
                (Some(said), Some(expected)) => assert!(said.contains(expected), "{said}"),
                (said, expected) => panic!("{text:?}: {said:?}, expected {expected:?}"),
            }
        }

        let mut screen = ssid_screen();
        press(&mut screen, 'j');
        press(&mut screen, 'P');
        let again = submit(&mut screen, "pässwörd");
        let request = prompt_of(again.as_ref());
        assert!(
            request.secret && request.initial == "pässwörd" && request.label == "New passphrase",
            "a bad first entry reopens on what was typed"
        );
    }

    #[test]
    fn a_cancelled_passphrase_prompt_does_not_keep_the_first_entry() {
        let mut screen = ssid_screen();
        press(&mut screen, 'j');
        press(&mut screen, 'P');
        submit(&mut screen, "correct horse");
        assert!(matches!(
            &screen.ssid_prompt,
            Some(SsidPrompt::ConfirmPassphrase { .. })
        ));

        // Esc closes the prompt in the App and tells the screen nothing;
        // the next key to reach the screen shows no prompt is open.
        press(&mut screen, 'j');
        assert!(screen.ssid_prompt.is_none());
        assert!(
            submit(&mut screen, "correct horse").is_none(),
            "a stray answer finds nothing waiting for it"
        );
    }

    #[test]
    fn an_open_network_has_no_passphrase_to_change() {
        let mut screen = ssid_screen();
        let answer = press(&mut screen, 'P');
        assert!(
            matches!(&answer, Some(Action::Notify(note)) if note.message.contains("open network")),
            "{answer:?}"
        );
        assert!(screen.ssid_prompt.is_none());
    }

    #[test]
    fn an_ssid_with_per_network_keys_has_no_single_passphrase_to_change() {
        let mut keyed = (*ssid(2, "Studio", "Wpa2Personal")).clone();
        keyed.preshared_key_count = 3;
        let mut screen = ssid_screen();
        screen.apply_action(&Action::WifiBroadcastsUpdated(Arc::new(vec![Arc::new(
            keyed,
        )])));

        let answer = press(&mut screen, 'P');
        assert!(
            matches!(&answer, Some(Action::Notify(note))
                if note.message.contains("each of 3 networks")
                    && note.message.contains("UniFi console")),
            "{answer:?}"
        );
        assert!(screen.ssid_prompt.is_none(), "no prompt opens");
        assert!(
            !draw(&screen).contains("P passphrase"),
            "the hint bar does not offer what cannot be done"
        );
        screen.handle_key_input(KeyEvent::from(KeyCode::Enter));
        let shown = draw(&screen);
        assert!(shown.contains("3 per-network keys"), "{shown}");
    }

    #[test]
    fn keys_the_list_did_not_name_are_found_in_the_full_record() {
        let plain = ssid(2, "Studio", "Wpa2Personal");
        let mut screen = ssid_screen();
        screen.apply_action(&Action::WifiBroadcastsUpdated(Arc::new(vec![Arc::clone(
            &plain,
        )])));
        let mut full = (*plain).clone();
        full.hidden = Some(false);
        full.preshared_key_count = 1;
        screen.apply_action(&Action::WifiBroadcastDetailLoaded {
            id: full.id.clone(),
            detail: Arc::new(full),
        });

        let answer = press(&mut screen, 'P');
        assert!(
            matches!(&answer, Some(Action::Notify(note)) if note.message.contains("1 network")),
            "{answer:?}"
        );
    }

    #[test]
    fn deleting_always_asks_first() {
        let mut screen = ssid_screen();
        let (prompt, run) = confirmed(press(&mut screen, 'D'));
        assert_eq!(prompt, "Delete Guest?");
        assert!(matches!(
            run,
            Action::WifiSsidWrite {
                change: SsidChange::Delete,
                ..
            }
        ));
    }

    #[test]
    fn an_empty_answer_cancels_the_prompt() {
        let mut screen = ssid_screen();
        press(&mut screen, 'N');
        assert!(
            screen
                .apply_action(&Action::PromptSubmitted(String::new()))
                .is_none()
        );
        assert!(screen.ssid_prompt.is_none());
    }
}
