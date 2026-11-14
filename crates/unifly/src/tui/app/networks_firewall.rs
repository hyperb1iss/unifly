//! App-side work for the Networks and Firewall screens: saving, creating,
//! and deleting a network, and moving a firewall policy.

use color_eyre::eyre::Result;
use serde_json::Value;
use tracing::warn;

use unifly_api::{Command, ConnectionState, Controller, EntityId};

use super::App;
use crate::tui::action::{Action, ConfirmRequest, Notice, Notification};
use crate::tui::screen::ScreenId;

/// Whether the controller has a live connection. Commands fail at once
/// without one.
pub(super) fn is_connected(controller: &Controller) -> bool {
    matches!(
        *controller.connection_state().borrow(),
        ConnectionState::Connected
    )
}

/// Fetch the controller's data after a write, so the screens show it
/// before anything says it is done. Returns whether the data is current.
pub(super) async fn refresh_after_write(controller: &Controller) -> bool {
    match controller.full_refresh().await {
        Ok(()) => true,
        Err(error) => {
            warn!(%error, "refresh after a write failed");
            false
        }
    }
}

/// What uses a network, as a sentence for the delete confirmation. The
/// controller reports counts per kind of resource.
fn reference_summary(references: &Value) -> String {
    let uses: Vec<String> = references
        .get("referenceResources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|resource| {
            let count = resource.get("referenceCount")?.as_u64()?;
            let kind = resource.get("resourceType")?.as_str()?;
            let (one, many) = match kind {
                "CLIENT" => ("client", "clients"),
                "DEVICE" => ("device", "devices"),
                "WIFI" => ("WiFi network", "WiFi networks"),
                "NAT_RULE" => ("NAT rule", "NAT rules"),
                "STATIC_ROUTE" => ("static route", "static routes"),
                "OSPF_ROUTE" => ("OSPF route", "OSPF routes"),
                "SD_WAN" => ("SD-WAN link", "SD-WAN links"),
                _ => ("other setting", "other settings"),
            };
            (count > 0).then(|| format!("{count} {}", if count == 1 { one } else { many }))
        })
        .collect();
    if uses.is_empty() {
        "Nothing uses it. This cannot be undone.".to_owned()
    } else {
        format!(
            "In use by {}. They lose this network. This cannot be undone.",
            uses.join(", ")
        )
    }
}

impl App {
    pub(super) fn handle_networks_firewall_action(&mut self, action: &Action) -> Result<bool> {
        match action {
            Action::NetworkSave(id, update) => {
                let name = update.name.clone().unwrap_or_else(|| self.network_name(id));
                self.write_network(
                    action,
                    Command::UpdateNetwork {
                        id: id.clone(),
                        update: (**update).clone(),
                    },
                    format!("Saved {}", self.shown_name(&name)),
                )?;
            }
            Action::NetworkCreate(request) => {
                self.write_network(
                    action,
                    Command::CreateNetwork((**request).clone()),
                    format!("Created network {}", self.shown_name(&request.name)),
                )?;
            }
            // The form draws the failure, so it is sanitized like a toast.
            Action::NetworkEditResult(result) => {
                let shown = result.clone().map_err(|error| self.shown_text(&error));
                self.forward_to_screen(ScreenId::Networks, &Action::NetworkEditResult(shown))?;
            }
            Action::RequestDeleteNetwork(id) => self.request_delete_network(id),
            Action::MovePolicy { policy, plan } => self.move_firewall_policy(policy, plan),
            Action::SetRuleSwitch { id, switch, on } => self.set_rule_switch(id, *switch, *on),
            // The move's own task has already freed the keys.
            Action::PolicyMoveSettled { .. } => {
                self.forward_to_screen(ScreenId::Firewall, action)?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// A network's real name, or its ID when it has left the store.
    fn network_name(&self, id: &EntityId) -> String {
        self.controller
            .as_ref()
            .and_then(|controller| controller.store().network_by_id(id))
            .map_or_else(|| id.to_string(), |network| network.name.clone())
    }

    /// Send a network create or update for the open form. The form locks
    /// while the controller has it and hears how it went as
    /// [`Action::NetworkEditResult`]: it closes on success and keeps the
    /// draft, with the reason, on failure.
    fn write_network(&mut self, request: &Action, command: Command, done: String) -> Result<()> {
        self.forward_to_screen(ScreenId::Networks, request)?;
        let tx = self.action_tx.clone();
        let refused = |reason: &str| {
            let _ = tx.send(Action::NetworkEditResult(Err(reason.to_owned())));
        };
        let Some(controller) = self.controller.clone() else {
            refused("Not connected to a controller");
            return Ok(());
        };
        if !is_connected(&controller) {
            refused("The controller is not connected, so nothing was sent");
            return Ok(());
        }
        let tx = self.action_tx.clone();
        tokio::spawn(async move {
            let result = match controller.execute(command).await {
                Ok(_) => {
                    refresh_after_write(&controller).await;
                    Ok(())
                }
                Err(error) => {
                    warn!(%error, "network write failed");
                    Err(error.to_string())
                }
            };
            let saved = result.is_ok();
            let _ = tx.send(Action::NetworkEditResult(result));
            if saved {
                let _ = tx.send(Action::Notify(Notification::success(done)));
            }
        });
        Ok(())
    }

    /// Ask before deleting a network, once the controller has said what
    /// uses it: the answer goes in the question.
    fn request_delete_network(&mut self, id: &EntityId) {
        let Some(controller) = self.controller.clone() else {
            self.show_notification(Notification::error("Not connected"));
            return;
        };
        if controller.store().network_by_id(id).is_none() {
            self.report_gone("That network");
            return;
        }
        if self.pending_confirm.is_some() {
            return;
        }
        let name = self.shown_name(&self.network_name(id));
        let checking = self.begin_progress(&format!("Checking what uses {name}\u{2026}"));
        let id = id.clone();
        let tx = self.action_tx.clone();
        tokio::spawn(async move {
            let detail = match controller.get_network_references(&id).await {
                Ok(references) => reference_summary(&references),
                Err(error) => {
                    warn!(%error, "network reference check failed");
                    format!("What uses it could not be checked ({error}). This cannot be undone.")
                }
            };
            let delete = Action::Run {
                command: Box::new(Command::DeleteNetwork { id, force: false }),
                notice: Notice::new(
                    format!("Deleting {name}\u{2026}"),
                    format!("Deleted network {name}"),
                ),
            };
            let _ = tx.send(Action::DismissToast(checking));
            let _ = tx.send(Action::ShowConfirm(
                ConfirmRequest::new(format!("Delete network {name}?"), delete).detail(detail),
            ));
        });
    }
}

#[cfg(test)]
mod tests;
