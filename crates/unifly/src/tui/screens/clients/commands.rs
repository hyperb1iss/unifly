//! Client commands: which ones a client's state allows, the prompts they
//! ask, and the actions they turn into.

use std::net::{IpAddr, Ipv4Addr};

use unifly_api::{Client, ClientType, Command, EntityId};

use super::ClientsScreen;
use super::view::client_name;
use crate::tui::action::{Action, ConfirmRequest, Notice, Notification, PromptRequest};

/// Minutes a guest is authorized for unless the prompt is changed.
const DEFAULT_GUEST_MINUTES: &str = "60";

/// What the open prompt's answer is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Asking {
    /// How many minutes to authorize this guest for.
    GuestMinutes { client: EntityId, name: String },
    /// The address to reserve for this client. `fixed` is whether it has
    /// one now, which an empty answer removes.
    FixedIp {
        client: EntityId,
        name: String,
        fixed: bool,
    },
}

fn info(message: String) -> Action {
    Action::Notify(Notification::info(message))
}

/// Whether the guest portal currently lets `client` through.
pub(super) fn is_authorized(client: &Client) -> bool {
    client
        .guest_auth
        .as_ref()
        .is_some_and(|auth| auth.authorized)
}

/// The address a fixed IP prompt starts from: the reservation, else the
/// address the client holds now.
fn current_ipv4(client: &Client) -> Option<Ipv4Addr> {
    client.fixed_ip.or(match client.ip {
        Some(IpAddr::V4(ip)) => Some(ip),
        _ => None,
    })
}

impl ClientsScreen {
    /// `b`: block the client, or unblock it when it is blocked.
    pub(super) fn request_block_toggle(&self) -> Option<Action> {
        let client = self.action_client()?;
        Some(if client.blocked {
            Action::RequestUnblockClient(client.id.clone())
        } else {
            Action::RequestBlockClient(client.id.clone())
        })
    }

    /// `x`: disconnect a wireless client. A wired port has no station to
    /// deauthenticate, so the controller's kick does nothing there.
    pub(super) fn request_kick(&self) -> Option<Action> {
        let client = self.action_client()?;
        if client.client_type != ClientType::Wireless {
            return Some(info(format!(
                "{} is not on WiFi; only wireless clients can be disconnected",
                client_name(client)
            )));
        }
        Some(Action::RequestKickClient(client.id.clone()))
    }

    pub(super) fn request_forget(&self) -> Option<Action> {
        let client = self.action_client()?;
        Some(Action::RequestForgetClient(client.id.clone()))
    }

    /// `A`: let a guest through the portal for a number of minutes, or
    /// revoke a guest who is through.
    pub(super) fn request_guest_access(&mut self) -> Option<Action> {
        let client = self.action_client()?;
        let name = client_name(client).to_owned();
        let id = client.id.clone();
        if !client.is_guest {
            return Some(info(format!(
                "{name} is not a guest, so it needs no authorizing"
            )));
        }
        if is_authorized(client) {
            return Some(Action::ShowConfirm(
                ConfirmRequest::new(
                    format!("Revoke guest access for {name}?"),
                    Action::Run {
                        command: Box::new(Command::UnauthorizeGuest { client_id: id }),
                        notice: Notice::new(
                            format!("Revoking guest access for {name}\u{2026}"),
                            format!("Revoked guest access for {name}"),
                        ),
                    },
                )
                .detail("It goes back to the guest portal."),
            ));
        }
        let prompt = minutes_prompt(&name, DEFAULT_GUEST_MINUTES, None);
        self.asking = Some(Asking::GuestMinutes { client: id, name });
        Some(prompt)
    }

    /// `I`: ask for the address to reserve for the client.
    pub(super) fn request_fixed_ip(&mut self) -> Option<Action> {
        let client = self.action_client()?;
        let name = client_name(client).to_owned();
        let fixed = client.use_fixedip;
        let initial = current_ipv4(client).map(|ip| ip.to_string());
        let prompt = fixed_ip_prompt(&name, initial.as_deref().unwrap_or(""), fixed, None);
        self.asking = Some(Asking::FixedIp {
            client: client.id.clone(),
            name,
            fixed,
        });
        Some(prompt)
    }

    /// The answer to the open prompt. A value that cannot be used opens
    /// the prompt again on the typed text, saying what is wrong with it.
    pub(super) fn submit_prompt(&mut self, text: &str) -> Option<Action> {
        let text = text.trim();
        match self.asking.take()? {
            Asking::GuestMinutes { client, name } => match text.parse::<u32>() {
                Ok(minutes) if minutes > 0 => Some(Action::Run {
                    command: Box::new(Command::AuthorizeGuest {
                        client_id: client,
                        time_limit_minutes: Some(minutes),
                        data_limit_mb: None,
                        rx_rate_kbps: None,
                        tx_rate_kbps: None,
                    }),
                    notice: Notice::new(
                        format!("Authorizing {name}\u{2026}"),
                        format!("Authorized {name} for {minutes} minutes"),
                    ),
                }),
                _ => {
                    let prompt = minutes_prompt(
                        &name,
                        text,
                        Some("That is not a number of minutes. Type a whole number above zero."),
                    );
                    self.asking = Some(Asking::GuestMinutes { client, name });
                    Some(prompt)
                }
            },
            Asking::FixedIp {
                client,
                name,
                fixed,
            } => {
                if text.is_empty() {
                    return if fixed {
                        Some(Action::RequestRemoveFixedIp(client))
                    } else {
                        Some(info(format!("{name} keeps taking its address from DHCP")))
                    };
                }
                if let Ok(ip) = text.parse::<Ipv4Addr>() {
                    return Some(Action::RequestSetFixedIp { client, ip });
                }
                let prompt = fixed_ip_prompt(
                    &name,
                    text,
                    fixed,
                    Some("That is not an IPv4 address. Type one like 10.0.20.5."),
                );
                self.asking = Some(Asking::FixedIp {
                    client,
                    name,
                    fixed,
                });
                Some(prompt)
            }
        }
    }
}

fn minutes_prompt(name: &str, initial: &str, problem: Option<&str>) -> Action {
    Action::OpenPrompt(PromptRequest {
        title: format!("Authorize {name}"),
        label: "Minutes of access".into(),
        initial: initial.to_owned(),
        hint: Some(
            problem
                .unwrap_or("The guest gets through the portal for this long.")
                .to_owned(),
        ),
        secret: false,
    })
}

fn fixed_ip_prompt(name: &str, initial: &str, fixed: bool, problem: Option<&str>) -> Action {
    let guidance = if fixed {
        "Its network is picked from the address. Clear the field to go back to DHCP."
    } else {
        "Its network is picked from the address. It applies from the next lease."
    };
    Action::OpenPrompt(PromptRequest {
        title: format!("Fixed IP for {name}"),
        label: "IPv4 address".into(),
        initial: initial.to_owned(),
        hint: Some(problem.unwrap_or(guidance).to_owned()),
        secret: false,
    })
}
