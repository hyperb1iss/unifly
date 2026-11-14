//! The SSIDs sub-tab's commands: enable, hide, rename, change the
//! passphrase, and delete. Each is a partial update of one broadcast.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent};
use unifly_api::model::{WifiBroadcast, WifiSecurityMode};
use unifly_api::{Command, EntityId, UpdateWifiBroadcastRequest};

use super::WifiScreen;
use crate::tui::action::{Action, ConfirmRequest, Notice, Notification, PromptRequest};

/// Longest network name the standard allows, in bytes.
const SSID_MAX_BYTES: usize = 32;
/// WPA passphrase length, in characters.
const PASSPHRASE_LENGTH: std::ops::RangeInclusive<usize> = 8..=63;

/// What the open prompt was asked for, and for which SSID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SsidPrompt {
    Rename(EntityId),
    Passphrase(EntityId),
}

fn update(ssid: &WifiBroadcast, update: UpdateWifiBroadcastRequest, notice: Notice) -> Action {
    Action::Run {
        command: Box::new(Command::UpdateWifiBroadcast {
            id: ssid.id.clone(),
            update,
        }),
        notice,
    }
}

fn warn(message: impl Into<String>) -> Action {
    Action::Notify(Notification::warning(message))
}

/// "its 3 clients", for the cost line of a confirmation.
fn clients_phrase(count: usize) -> String {
    match count {
        0 => "nobody is on it right now".to_owned(),
        1 => "its 1 client is disconnected".to_owned(),
        count => format!("its {count} clients are disconnected"),
    }
}

impl WifiScreen {
    pub(super) fn handle_ssid_key(&mut self, key: KeyEvent) -> Option<Action> {
        let ssid = Arc::clone(self.selected_ssid()?);
        match key.code {
            KeyCode::Char('E') => Some(self.toggle_ssid_enabled(&ssid)),
            KeyCode::Char('H') => Some(toggle_ssid_hidden(&ssid)),
            KeyCode::Char('N') => Some(self.prompt_ssid_rename(&ssid, None)),
            KeyCode::Char('P') => Some(self.prompt_ssid_passphrase(&ssid, None)),
            KeyCode::Char('D') => Some(self.delete_ssid(&ssid)),
            _ => None,
        }
    }

    /// Enabling runs at once. Disabling asks first: it drops every client
    /// on the network.
    fn toggle_ssid_enabled(&self, ssid: &WifiBroadcast) -> Action {
        let name = &ssid.name;
        let request = |enabled| UpdateWifiBroadcastRequest {
            enabled: Some(enabled),
            ..Default::default()
        };
        if !ssid.enabled {
            return update(
                ssid,
                request(true),
                Notice::new(
                    format!("Enabling {name}\u{2026}"),
                    format!("Enabled {name}"),
                ),
            );
        }
        let run = update(
            ssid,
            request(false),
            Notice::new(
                format!("Disabling {name}\u{2026}"),
                format!("Disabled {name}"),
            ),
        );
        Action::ShowConfirm(
            ConfirmRequest::new(format!("Disable {name}?"), run).detail(format!(
                "It stops broadcasting: {}.",
                clients_phrase(self.ssid_client_count(name))
            )),
        )
    }

    fn prompt_ssid_rename(&mut self, ssid: &WifiBroadcast, typed: Option<&str>) -> Action {
        self.ssid_prompt = Some(SsidPrompt::Rename(ssid.id.clone()));
        Action::OpenPrompt(PromptRequest {
            title: format!("Rename {}", ssid.name),
            label: "Network name (SSID)".to_owned(),
            initial: typed.unwrap_or(&ssid.name).to_owned(),
            hint: Some("Up to 32 characters. Clients must rejoin under the new name.".to_owned()),
            secret: false,
        })
    }

    fn prompt_ssid_passphrase(&mut self, ssid: &WifiBroadcast, typed: Option<&str>) -> Action {
        let reason = match ssid.security {
            WifiSecurityMode::Open => Some("it is an open network"),
            WifiSecurityMode::Wpa2Enterprise
            | WifiSecurityMode::Wpa3Enterprise
            | WifiSecurityMode::Wpa2Wpa3Enterprise => Some("clients sign in through RADIUS"),
            _ => None,
        };
        if let Some(reason) = reason {
            return Action::Notify(Notification::info(format!(
                "{} has no passphrase: {reason}",
                ssid.name
            )));
        }
        self.ssid_prompt = Some(SsidPrompt::Passphrase(ssid.id.clone()));
        Action::OpenPrompt(PromptRequest {
            title: format!("Passphrase for {}", ssid.name),
            label: "New passphrase".to_owned(),
            initial: typed.unwrap_or_default().to_owned(),
            hint: Some(
                "8 to 63 characters. The current one cannot be read back from the controller."
                    .to_owned(),
            ),
            secret: true,
        })
    }

    fn delete_ssid(&self, ssid: &WifiBroadcast) -> Action {
        let name = &ssid.name;
        let run = Action::Run {
            command: Box::new(Command::DeleteWifiBroadcast {
                id: ssid.id.clone(),
                force: false,
            }),
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
        let (SsidPrompt::Rename(id) | SsidPrompt::Passphrase(id)) = &prompt;
        let Some(ssid) = self.ssids.iter().find(|ssid| &ssid.id == id).cloned() else {
            return Some(warn("That SSID is no longer on the controller"));
        };
        match prompt {
            SsidPrompt::Rename(_) => self.submit_ssid_rename(&ssid, text),
            SsidPrompt::Passphrase(_) => Some(self.submit_ssid_passphrase(&ssid, text)),
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
        let length = text.chars().count();
        if !PASSPHRASE_LENGTH.contains(&length) {
            self.send(warn(format!(
                "A passphrase is 8 to 63 characters; that one is {length}"
            )));
            return self.prompt_ssid_passphrase(ssid, Some(text));
        }
        let name = &ssid.name;
        let run = update(
            ssid,
            UpdateWifiBroadcastRequest {
                passphrase: Some(text.to_owned()),
                ..Default::default()
            },
            Notice::new(
                format!("Changing the passphrase for {name}\u{2026}"),
                format!("Changed the passphrase for {name}"),
            ),
        );
        Action::ShowConfirm(
            ConfirmRequest::new(format!("Change the passphrase for {name}?"), run)
                .detail("Every client on it must enter the new one to reconnect."),
        )
    }

    /// Queue an action beside the one being returned.
    fn send(&self, action: Action) {
        if let Some(tx) = &self.action_tx {
            let _ = tx.send(action);
        }
    }
}

/// Hiding only stops the name being announced, so it runs without asking.
fn toggle_ssid_hidden(ssid: &WifiBroadcast) -> Action {
    let name = &ssid.name;
    let notice = if ssid.hidden {
        Notice::new(
            format!("Showing {name}\u{2026}"),
            format!("{name} announces its name again"),
        )
    } else {
        Notice::new(
            format!("Hiding {name}\u{2026}"),
            format!("{name} is hidden: clients must know its name to join"),
        )
    };
    update(
        ssid,
        UpdateWifiBroadcastRequest {
            hide_ssid: Some(!ssid.hidden),
            ..Default::default()
        },
        notice,
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crossterm::event::{KeyCode, KeyEvent};
    use serde_json::json;
    use unifly_api::Command;
    use unifly_api::model::WifiBroadcast;

    use crate::tui::action::{Action, WifiSubTab};
    use crate::tui::component::Component;
    use crate::tui::screens::wifi::WifiScreen;
    use crate::tui::table::render_lines;

    fn ssid(id: u128, name: &str, security: &str) -> Arc<WifiBroadcast> {
        Arc::new(
            serde_json::from_value(json!({
                "id": uuid::Uuid::from_u128(id).to_string(), "name": name,
                "enabled": true, "broadcast_type": "Standard", "security": security,
                "network_id": uuid::Uuid::from_u128(100).to_string(),
                "frequencies_ghz": [2.4, 5.0], "hidden": false,
                "client_isolation": false, "band_steering": true,
                "mlo_enabled": false, "fast_roaming": false,
                "hotspot_enabled": false, "origin": null
            }))
            .expect("SSID fixture should deserialize"),
        )
    }

    fn ssid_screen() -> WifiScreen {
        let mut screen = WifiScreen::new();
        screen.apply_action(&Action::WifiBroadcastsUpdated(Arc::new(vec![
            ssid(2, "Studio", "Wpa2Wpa3Personal"),
            ssid(1, "Guest", "Open"),
        ])));
        screen.apply_action(&Action::WifiSubTab(WifiSubTab::Ssids));
        screen
    }

    fn press(screen: &mut WifiScreen, key: char) -> Option<Action> {
        screen.handle_key_input(KeyEvent::from(KeyCode::Char(key)))
    }

    fn update_of(action: &Action) -> &unifly_api::UpdateWifiBroadcastRequest {
        match action {
            Action::Run { command, .. } => match command.as_ref() {
                Command::UpdateWifiBroadcast { update, .. } => update,
                other => panic!("not an SSID update: {other:?}"),
            },
            other => panic!("not a command: {other:?}"),
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
    fn disabling_asks_first_and_enabling_does_not() {
        let mut screen = ssid_screen();
        let (prompt, run) = confirmed(press(&mut screen, 'E'));
        assert_eq!(prompt, "Disable Guest?");
        assert_eq!(update_of(&run).enabled, Some(false));

        let mut off = (*ssid(1, "Guest", "Open")).clone();
        off.enabled = false;
        screen.apply_action(&Action::WifiBroadcastsUpdated(Arc::new(vec![Arc::new(
            off,
        )])));
        let run = press(&mut screen, 'E').expect("E enables");
        assert_eq!(update_of(&run).enabled, Some(true));
    }

    #[test]
    fn hiding_flips_the_hidden_flag() {
        let mut screen = ssid_screen();
        let run = press(&mut screen, 'H').expect("H hides");
        assert_eq!(update_of(&run).hide_ssid, Some(true));
        assert_eq!(update_of(&run).enabled, None, "only the one field is sent");
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

    #[test]
    fn a_passphrase_is_masked_and_held_to_8_to_63_characters() {
        let mut screen = ssid_screen();
        press(&mut screen, 'j');
        let prompt = press(&mut screen, 'P');
        assert!(
            matches!(&prompt, Some(Action::OpenPrompt(request))
                if request.secret && request.hint.as_deref().is_some_and(|hint| hint.contains("cannot be read back"))),
            "{prompt:?}"
        );

        let short = screen.apply_action(&Action::PromptSubmitted("short".into()));
        assert!(
            matches!(&short, Some(Action::OpenPrompt(request)) if request.initial == "short" && request.secret),
            "{short:?}"
        );
        let long = screen.apply_action(&Action::PromptSubmitted("x".repeat(64)));
        assert!(matches!(long, Some(Action::OpenPrompt(_))), "{long:?}");

        let (question, run) =
            confirmed(screen.apply_action(&Action::PromptSubmitted("correct horse".into())));
        assert_eq!(question, "Change the passphrase for Studio?");
        assert_eq!(update_of(&run).passphrase.as_deref(), Some("correct horse"));
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
    fn deleting_always_asks_first() {
        let mut screen = ssid_screen();
        let (prompt, run) = confirmed(press(&mut screen, 'D'));
        assert_eq!(prompt, "Delete Guest?");
        assert!(matches!(
            run,
            Action::Run { command, .. } if matches!(*command, Command::DeleteWifiBroadcast { .. })
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
