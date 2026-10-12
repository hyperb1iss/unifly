use std::time::Instant;

use color_eyre::eyre::Result;
use tracing::warn;

use unifly_api::{Command, EntityId, MacAddress};

use super::App;
use crate::tui::action::{Action, ConfirmAction, Notification};

impl App {
    pub(super) fn handle_command_action(&mut self, action: &Action) -> Result<bool> {
        match action {
            Action::RequestRestart(id) => {
                let name = self.resolve_device_name(id);
                self.queue_confirm(ConfirmAction::RestartDevice {
                    id: id.clone(),
                    name,
                })?;
            }
            Action::RequestAdopt(mac) => {
                self.queue_confirm(ConfirmAction::AdoptDevice { mac: mac.clone() })?;
            }
            Action::RequestUnadopt(id) => {
                let name = self.resolve_device_name(id);
                self.queue_confirm(ConfirmAction::UnadoptDevice {
                    id: id.clone(),
                    name,
                })?;
            }
            Action::RequestLocate(id) => {
                if let Some(mac) = self.resolve_device_mac(id) {
                    self.execute_command(
                        Command::LocateDevice {
                            mac: mac.clone(),
                            enable: true,
                        },
                        format!("Locating {mac}"),
                    );
                }
            }
            Action::RequestUpgrade(id) => {
                let name = self.resolve_device_name(id);
                if let Some(mac) = self.resolve_device_mac(id) {
                    self.queue_confirm(ConfirmAction::UpgradeDevice { mac, name })?;
                }
            }
            Action::RequestPortPowerCycle(device_id, port_idx) => {
                self.queue_confirm(ConfirmAction::PowerCyclePort {
                    device_id: device_id.clone(),
                    port_idx: *port_idx,
                })?;
            }
            Action::RequestBlockClient(id) => {
                let name = self.resolve_client_name(id);
                self.queue_confirm(ConfirmAction::BlockClient {
                    id: id.clone(),
                    name,
                })?;
            }
            Action::RequestUnblockClient(id) => {
                let name = self.resolve_client_name(id);
                self.queue_confirm(ConfirmAction::UnblockClient {
                    id: id.clone(),
                    name,
                })?;
            }
            Action::RequestForgetClient(id) => {
                let name = self.resolve_client_name(id);
                self.queue_confirm(ConfirmAction::ForgetClient {
                    id: id.clone(),
                    name,
                })?;
            }
            Action::RequestKickClient(id) => {
                if let Some(mac) = self.resolve_client_mac(id) {
                    let name = self.resolve_client_name(id);
                    self.execute_command(Command::KickClient { mac }, format!("Kicked {name}"));
                }
            }
            Action::ShowConfirm(confirm) => {
                self.pending_confirm = Some(confirm.clone());
            }
            Action::ConfirmYes => {
                if let Some(confirm) = self.pending_confirm.take() {
                    self.execute_confirm(confirm);
                }
            }
            Action::ConfirmNo => {
                self.pending_confirm = None;
            }
            Action::NetworkSave(id, update) => {
                self.execute_command(
                    Command::UpdateNetwork {
                        id: id.clone(),
                        update: *update.clone(),
                    },
                    "Updated network".into(),
                );
            }
            _ => return Ok(false),
        }

        Ok(true)
    }

    pub(super) fn resolve_device_name(&self, id: &EntityId) -> String {
        self.controller
            .as_ref()
            .and_then(|c| c.store().device_by_id(id))
            .and_then(|d| d.name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    pub(super) fn resolve_device_mac(&self, id: &EntityId) -> Option<MacAddress> {
        self.controller
            .as_ref()
            .and_then(|c| c.store().device_by_id(id))
            .map(|d| d.mac.clone())
    }

    pub(super) fn resolve_client_name(&self, id: &EntityId) -> String {
        self.controller
            .as_ref()
            .and_then(|c| c.store().client_by_id(id))
            .and_then(|c| c.name.clone().or(c.hostname.clone()))
            .unwrap_or_else(|| id.to_string())
    }

    pub(super) fn resolve_client_mac(&self, id: &EntityId) -> Option<MacAddress> {
        self.controller
            .as_ref()
            .and_then(|c| c.store().client_by_id(id))
            .map(|c| c.mac.clone())
    }

    /// Spawn a command execution task. Sends a Notify action on completion.
    pub(super) fn execute_command(&self, cmd: Command, success_msg: String) {
        let Some(controller) = self.controller.clone() else {
            let _ = self
                .action_tx
                .send(Action::Notify(Notification::error("Not connected")));
            return;
        };

        let tx = self.action_tx.clone();
        tokio::spawn(async move {
            match controller.execute(cmd).await {
                Ok(_) => {
                    let _ = tx.send(Action::Notify(Notification::success(success_msg)));
                }
                Err(e) => {
                    warn!(error = %e, "command execution failed");
                    let _ = tx.send(Action::Notify(Notification::error(format!("{e}"))));
                }
            }
        });
    }

    /// Map a confirmed action to its `Command` and execute it.
    pub(super) fn execute_confirm(&self, action: ConfirmAction) {
        match action {
            ConfirmAction::RestartDevice { id, name } => {
                self.execute_command(Command::RestartDevice { id }, format!("Restarting {name}"));
            }
            ConfirmAction::UpgradeDevice { mac, name } => {
                self.execute_command(
                    Command::UpgradeDevice {
                        mac,
                        firmware_url: None,
                    },
                    format!("Upgrading {name}"),
                );
            }
            ConfirmAction::UnadoptDevice { id, name } => {
                self.execute_command(Command::RemoveDevice { id }, format!("Removed {name}"));
            }
            ConfirmAction::AdoptDevice { mac } => {
                self.execute_command(
                    Command::AdoptDevice {
                        mac: MacAddress::new(&mac),
                        ignore_device_limit: false,
                    },
                    format!("Adopting {mac}"),
                );
            }
            ConfirmAction::PowerCyclePort {
                device_id,
                port_idx,
            } => {
                self.execute_command(
                    Command::PowerCyclePort {
                        device_id,
                        port_idx,
                    },
                    format!("Power cycling port {port_idx}"),
                );
            }
            ConfirmAction::BlockClient { id, name } => {
                if let Some(mac) = self.resolve_client_mac(&id) {
                    self.execute_command(Command::BlockClient { mac }, format!("Blocked {name}"));
                }
            }
            ConfirmAction::UnblockClient { id, name } => {
                if let Some(mac) = self.resolve_client_mac(&id) {
                    self.execute_command(
                        Command::UnblockClient { mac },
                        format!("Unblocked {name}"),
                    );
                }
            }
            ConfirmAction::ForgetClient { id, name } => {
                if let Some(mac) = self.resolve_client_mac(&id) {
                    self.execute_command(Command::ForgetClient { mac }, format!("Forgot {name}"));
                }
            }
            ConfirmAction::DeleteFirewallPolicy { id, name } => {
                self.execute_command(
                    Command::DeleteFirewallPolicy { id },
                    format!("Deleted policy {name}"),
                );
            }
        }
    }

    pub(super) fn show_notification(&mut self, notification: Notification) {
        self.notification = Some((notification, Instant::now()));
    }

    fn queue_confirm(&self, confirm: ConfirmAction) -> Result<()> {
        self.action_tx.send(Action::ShowConfirm(confirm))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_adopt_queues_confirmation() {
        let mut app = App::new(None, None, false, true);

        app.process_action(&Action::RequestAdopt("aa-bb-cc-dd-ee-ff".into()))
            .expect("request handling should succeed");

        let queued = app
            .action_rx
            .try_recv()
            .expect("confirm action should be queued");
        assert!(matches!(
            queued,
            Action::ShowConfirm(ConfirmAction::AdoptDevice { ref mac })
                if mac == "aa-bb-cc-dd-ee-ff"
        ));
    }

    #[test]
    fn request_port_power_cycle_queues_confirmation() {
        let mut app = App::new(None, None, false, true);
        let device_id = EntityId::from("507f1f77bcf86cd799439011");

        app.process_action(&Action::RequestPortPowerCycle(device_id.clone(), 7))
            .expect("request handling should succeed");

        let queued = app
            .action_rx
            .try_recv()
            .expect("confirm action should be queued");
        assert!(matches!(
            queued,
            Action::ShowConfirm(ConfirmAction::PowerCyclePort {
                ref device_id,
                port_idx: 7,
            }) if device_id == &EntityId::from("507f1f77bcf86cd799439011")
        ));
    }
}
