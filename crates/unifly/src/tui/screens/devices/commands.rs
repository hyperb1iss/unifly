//! Device and port commands: which ones a device's state allows, and the
//! actions they turn into.

use unifly_api::model::{PoeMode, Port};
use unifly_api::{Command, Device, DeviceState, DeviceType};

use super::DevicesScreen;
use super::view::is_down;
use crate::tui::action::{Action, ConfirmRequest, Notice, Notification, PortChange, PromptRequest};

/// A device's name as the screen shows it.
pub(super) fn device_name(device: &Device) -> &str {
    device.name.as_deref().unwrap_or("Unknown")
}

fn info(message: String) -> Action {
    Action::Notify(Notification::info(message))
}

/// Whether `device` can take a command that needs it adopted and reachable.
pub(super) fn is_reachable(device: &Device) -> bool {
    !is_down(device.state) && !is_pending(device)
}

pub(super) fn is_pending(device: &Device) -> bool {
    device.state == DeviceState::PendingAdoption
}

pub(super) fn is_gateway(device: &Device) -> bool {
    device.device_type == DeviceType::Gateway
}

/// Whether the port is supplying PoE, which is what a power cycle needs.
pub(super) fn port_powers(port: &Port) -> bool {
    port.poe.as_ref().is_some_and(|poe| poe.enabled)
}

/// What a command key on `device` does, or a toast saying why not.
impl DevicesScreen {
    /// Why a command that needs a reachable device cannot run, if so.
    fn refusal(device: &Device, verb: &str) -> Option<Action> {
        let name = device_name(device);
        if is_pending(device) {
            return Some(info(format!(
                "{name} is not adopted yet; press A to adopt it"
            )));
        }
        if is_down(device.state) {
            return Some(info(format!("{name} is offline and cannot {verb}")));
        }
        None
    }

    pub(super) fn request_restart(&self) -> Option<Action> {
        let device = self.action_device()?;
        Self::refusal(device, "be restarted")
            .or_else(|| Some(Action::RequestRestart(device.id.clone())))
    }

    pub(super) fn request_locate(&self) -> Option<Action> {
        let device = self.action_device()?;
        Self::refusal(device, "flash its LED")
            .or_else(|| Some(Action::RequestLocate(device.id.clone())))
    }

    pub(super) fn request_upgrade(&self) -> Option<Action> {
        let device = self.action_device()?;
        if let Some(refused) = Self::refusal(device, "be upgraded") {
            return Some(refused);
        }
        if !device.firmware_updatable {
            return Some(info(format!(
                "{} has no firmware update waiting",
                device_name(device)
            )));
        }
        Some(Action::RequestUpgrade(device.id.clone()))
    }

    pub(super) fn request_adopt(&self) -> Option<Action> {
        let device = self.action_device()?;
        if !is_pending(device) {
            return Some(info(format!("{} is already adopted", device_name(device))));
        }
        Some(Action::RequestAdoptDevice(device.id.clone()))
    }

    pub(super) fn request_provision(&self) -> Option<Action> {
        let device = self.action_device()?;
        Self::refusal(device, "be provisioned")
            .or_else(|| Some(Action::RequestProvision(device.id.clone())))
    }

    pub(super) fn request_speedtest(&self) -> Option<Action> {
        let device = self.action_device()?;
        let name = device_name(device);
        if !is_gateway(device) {
            return Some(info(format!(
                "{name} is not a gateway; the speed test runs on one"
            )));
        }
        if let Some(refused) = Self::refusal(device, "run a speed test") {
            return Some(refused);
        }
        Some(Action::ShowConfirm(
            ConfirmRequest::new(
                format!("Run a speed test on {name}?"),
                Action::Run {
                    command: Box::new(Command::SpeedtestDevice),
                    notice: Notice::new(
                        format!("Starting a speed test on {name}\u{2026}"),
                        "Speed test started; the result arrives as an event",
                    ),
                },
            )
            .detail("It fills the WAN link for about a minute."),
        ))
    }

    /// Forget the device the open detail shows.
    pub(super) fn request_forget(&self) -> Option<Action> {
        let device = self.detail_device()?;
        Some(Action::RequestUnadopt(device.id.clone()))
    }

    pub(super) fn request_power_cycle(&self) -> Option<Action> {
        let (device, port) = self.cursor_port()?;
        if !port_powers(port) {
            return Some(info(format!("Port {} is not supplying PoE", port.index)));
        }
        Some(Action::RequestPortPowerCycle(device.id.clone(), port.index))
    }

    /// Flip the port's PoE between off and auto. Turning it off asks
    /// first, since whatever the port powers goes dark.
    pub(super) fn request_poe_toggle(&self) -> Option<Action> {
        let (device, port) = self.cursor_port()?;
        let Some(poe) = &port.poe else {
            return Some(info(format!("Port {} has no PoE", port.index)));
        };
        let name = device_name(device);
        let update = |mode| Action::RequestPortUpdate {
            device: device.id.clone(),
            port: port.index,
            change: PortChange::Poe(mode),
        };
        if !poe.enabled {
            return Some(update(PoeMode::Auto));
        }
        Some(Action::ShowConfirm(
            ConfirmRequest::new(
                format!("Turn PoE off on port {} of {name}?", port.index),
                update(PoeMode::Off),
            )
            .detail("Whatever it powers shuts down."),
        ))
    }

    /// Ask for a new name for the port under the cursor.
    pub(super) fn request_port_rename(&mut self) -> Option<Action> {
        let (device, port) = self.cursor_port()?;
        let (device_id, index) = (device.id.clone(), port.index);
        let current = self
            .detail
            .as_ref()
            .and_then(|detail| detail.profile(index))
            .and_then(|profile| profile.name.clone())
            .or_else(|| port.name.clone())
            .unwrap_or_default();
        let title = format!("Rename port {index} on {}", device_name(device));
        self.renaming = Some((device_id, index));
        Some(rename_prompt(title, current))
    }

    /// The answer to the rename prompt.
    pub(super) fn submit_prompt(&mut self, text: &str) -> Option<Action> {
        let (device, port) = self.renaming.take()?;
        let name = text.trim();
        if name.is_empty() {
            return Some(info("The port keeps its name: nothing was typed".into()));
        }
        if name.chars().count() > MAX_PORT_NAME {
            self.renaming = Some((device, port));
            return Some(Action::OpenPrompt(PromptRequest {
                hint: Some(format!(
                    "That is too long: a port name holds {MAX_PORT_NAME} characters."
                )),
                ..rename_request(format!("Rename port {port}"), name.to_owned())
            }));
        }
        Some(Action::RequestPortUpdate {
            device,
            port,
            change: PortChange::Rename(name.to_owned()),
        })
    }
}

/// The longest port label the controller keeps.
const MAX_PORT_NAME: usize = 64;

fn rename_request(title: String, initial: String) -> PromptRequest {
    PromptRequest {
        title,
        label: "Port name".into(),
        initial,
        hint: Some("Shown in the port list and on the switch's page.".into()),
        secret: false,
    }
}

fn rename_prompt(title: String, initial: String) -> Action {
    Action::OpenPrompt(rename_request(title, initial))
}
