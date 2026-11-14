//! Device and port commands: which ones a device's state allows, and the
//! actions they turn into.

use unifly_api::model::{PoeMode, Port, PortProfile};
use unifly_api::{Command, Device, DeviceState, DeviceType};

use super::DevicesScreen;
use super::detail::Profiles;
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

/// Whether the port's PoE is switched on, which is what a power cycle
/// needs and what `O` turns off. The controller's profile says so where
/// it is loaded; a port can be on and idle, so the live state only
/// answers without one.
pub(super) fn port_powers(port: &Port, profile: Option<&PortProfile>) -> bool {
    let Some(poe) = &port.poe else {
        return false;
    };
    match profile.and_then(|profile| profile.poe_mode) {
        Some(PoeMode::Off) => false,
        Some(PoeMode::Auto | PoeMode::Passive24V | PoeMode::Passthrough) => true,
        Some(PoeMode::Other) | None => poe.enabled,
    }
}

/// Why the port settings cannot be written from here.
const NO_PORT_WRITES: &str =
    "Port settings need a direct controller session, which a cloud profile does not have";

/// Why a command that only the Session API serves cannot run here.
const NEEDS_SESSION: &str =
    "needs a direct controller session, which a cloud profile does not have";

/// Why a command that only the Integration API serves cannot run here.
const NEEDS_INTEGRATION: &str = "needs the Integration API, which this profile cannot reach";

/// A PoE mode as the confirmations and toasts name it.
pub(in crate::tui) fn poe_mode_label(mode: PoeMode) -> &'static str {
    match mode {
        PoeMode::Off => "off",
        PoeMode::Passive24V => "passive 24V",
        PoeMode::Passthrough => "passthrough",
        PoeMode::Auto | PoeMode::Other => "auto",
    }
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
        if !self.has_session() {
            return Some(info(format!("A firmware upgrade {NEEDS_SESSION}")));
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
        if let Some(refused) = Self::refusal(device, "be provisioned") {
            return Some(refused);
        }
        if !self.has_session() {
            return Some(info(format!("Provisioning {NEEDS_SESSION}")));
        }
        Some(Action::RequestProvision(device.id.clone()))
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
        if !self.has_session() {
            return Some(info(format!("The speed test {NEEDS_SESSION}")));
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

    /// Whether `device` can be removed from the site: it is adopted, and
    /// the Integration API that removes it is within reach.
    pub(super) fn can_remove(&self, device: &Device) -> bool {
        !is_pending(device) && self.has_integration()
    }

    /// Remove the device the open detail shows from the site, after a
    /// confirmation that says what happens to it in its state.
    pub(super) fn request_forget(&self) -> Option<Action> {
        let device = self.detail_device()?;
        let name = device_name(device);
        if is_pending(device) {
            return Some(info(format!(
                "{name} is not adopted, so there is nothing to remove"
            )));
        }
        if !self.has_integration() {
            return Some(info(format!("Removing a device {NEEDS_INTEGRATION}")));
        }
        let detail = if is_down(device.state) {
            "It is offline, so it is only dropped from the site. \
             Reset it by hand before adopting it again."
        } else {
            "It is factory reset and has to be adopted again."
        };
        Some(Action::ShowConfirm(
            ConfirmRequest::new(
                format!("Remove {name} from this site?"),
                Action::Run {
                    command: Box::new(Command::RemoveDevice {
                        id: device.id.clone(),
                    }),
                    notice: Notice::new(
                        format!("Removing {name}\u{2026}"),
                        format!("Removed {name}"),
                    ),
                },
            )
            .detail(detail),
        ))
    }

    /// Whether the commands that only the Session API serves can run: not
    /// once the controller has said it has none.
    pub(super) fn has_session(&self) -> bool {
        !matches!(self.access, Some(access) if !access.session)
    }

    /// Whether the commands that only the Integration API serves can run.
    pub(super) fn has_integration(&self) -> bool {
        !matches!(self.access, Some(access) if !access.integration)
    }

    /// Whether the port under the cursor has its PoE switched on.
    pub(super) fn cursor_port_powers(&self) -> bool {
        self.cursor_port().is_some_and(|(_, port)| {
            let profile = self
                .detail
                .as_ref()
                .and_then(|detail| detail.profile(port.index));
            port_powers(port, profile)
        })
    }

    /// Whether port settings can be written: they live in the Session API.
    pub(super) fn ports_writable(&self) -> bool {
        self.has_session()
    }

    pub(super) fn request_power_cycle(&self) -> Option<Action> {
        let (device, port) = self.cursor_port()?;
        if !self.cursor_port_powers() {
            return Some(info(format!("Port {} is not supplying PoE", port.index)));
        }
        if !self.has_integration() {
            return Some(info(format!("A power cycle {NEEDS_INTEGRATION}")));
        }
        Some(Action::RequestPortPowerCycle(device.id.clone(), port.index))
    }

    /// The PoE mode the controller holds for the port under the cursor,
    /// once its profile is loaded.
    pub(super) fn cursor_poe_mode(&self) -> Option<PoeMode> {
        let (_, port) = self.cursor_port()?;
        port.poe.as_ref()?;
        self.detail.as_ref()?.profile(port.index)?.poe_mode
    }

    /// Turn the port's PoE off, or back on in the mode it had. Both ways
    /// wait for the port's profile: the live state cannot tell a passive
    /// 24V port from a negotiated one, and writing the wrong mode leaves
    /// the device dark or feeds it the wrong voltage.
    pub(super) fn request_poe_toggle(&self) -> Option<Action> {
        let (device, port) = self.cursor_port()?;
        let index = port.index;
        if port.poe.is_none() {
            return Some(info(format!("Port {index} has no PoE")));
        }
        if !self.ports_writable() {
            return Some(info(NO_PORT_WRITES.into()));
        }
        let Some(mode) = self.cursor_poe_mode() else {
            let detail = self.detail.as_ref()?;
            return Some(info(match &detail.profiles {
                Profiles::Unavailable(reason) => format!(
                    "PoE on port {index} stays as it is: its settings cannot be read ({reason})"
                ),
                Profiles::Loaded(_) => format!(
                    "PoE on port {index} stays as it is: the controller reports no PoE mode for it"
                ),
                Profiles::Unasked | Profiles::Loading => format!(
                    "The settings of port {index} are still loading; press O again in a moment"
                ),
            }));
        };
        let name = device_name(device);
        let update = |mode| Action::RequestPortUpdate {
            device: device.id.clone(),
            port: index,
            change: PortChange::Poe(mode),
        };
        match mode {
            PoeMode::Other => Some(info(format!(
                "Port {index} uses a PoE mode unifly does not know; change it in the controller"
            ))),
            PoeMode::Auto | PoeMode::Passive24V | PoeMode::Passthrough => {
                Some(Action::ShowConfirm(
                    ConfirmRequest::new(
                        format!("Turn PoE off on port {index} of {name}?"),
                        update(PoeMode::Off),
                    )
                    .detail("Whatever it powers shuts down."),
                ))
            }
            PoeMode::Off => match self.poe_on_modes.get(&(device.id.clone(), index)) {
                Some(previous) => Some(update(*previous)),
                // Nothing here saw the mode the port had, and the
                // controller does not keep it.
                None => Some(Action::ShowConfirm(
                    ConfirmRequest::new(
                        format!("Turn PoE on for port {index} of {name} in auto mode?"),
                        update(PoeMode::Auto),
                    )
                    .detail(
                        "The mode it had before is not known. A passive 24V device \
                         needs that mode set in the controller instead.",
                    ),
                )),
            },
        }
    }

    /// Ask for a new name for the port under the cursor.
    pub(super) fn request_port_rename(&mut self) -> Option<Action> {
        let (device, port) = self.cursor_port()?;
        if !self.ports_writable() {
            return Some(info(NO_PORT_WRITES.into()));
        }
        let (device_id, index) = (device.id.clone(), port.index);
        let current = self
            .detail
            .as_ref()
            .and_then(|detail| detail.profile(index))
            .and_then(|profile| profile.name.clone())
            .or_else(|| port.name.clone())
            .unwrap_or_default();
        let title = format!("Rename port {index} on {}", device_name(device));
        self.renaming = Some((device_id, index, current.clone()));
        Some(rename_prompt(title, current))
    }

    /// The answer to the rename prompt.
    pub(super) fn submit_prompt(&mut self, text: &str) -> Option<Action> {
        let (device, port, current) = self.renaming.take()?;
        let name = text.trim();
        // The prompt starts on the name the port has, which under redaction
        // is a stand-in: writing that back would rename the real port.
        if name.is_empty() || name == current {
            return Some(info(format!("Port {port} keeps its name")));
        }
        if name.chars().count() > MAX_PORT_NAME {
            self.renaming = Some((device, port, current));
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
