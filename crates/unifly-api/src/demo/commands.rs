// ── Demo command handling ──
//
// Harmless device and client actions succeed against the simulation and
// emit the event a real controller would. Anything that would need a real
// controller to mean something (CRUD, adoption, site admin) is refused with
// a clear error instead of pretending.

use chrono::{DateTime, Utc};

use super::catalog::{self, AP_FIRMWARE};
use super::sim::{Delta, Pending, Simulation, client_label};
use crate::command::{Command, CommandResult};
use crate::core_error::CoreError;
use crate::model::{ClientType, DeviceState, EntityId, EventCategory, EventSeverity, MacAddress};

const DEVICE_INFO: (EventCategory, EventSeverity) = (EventCategory::Device, EventSeverity::Info);
const CLIENT_INFO: (EventCategory, EventSeverity) = (EventCategory::Client, EventSeverity::Info);
const ADMIN_INFO: (EventCategory, EventSeverity) = (EventCategory::Admin, EventSeverity::Info);

/// Ticks a restart takes before the device reports back.
const RESTART_TICKS: u64 = 20;
/// Ticks a firmware upgrade takes.
const UPGRADE_TICKS: u64 = 45;
/// Ticks before a kicked client rejoins.
const KICK_RETURN_TICKS: u64 = 6;
/// Ticks before the power-cycled camera comes back.
const CAMERA_REVIVE_TICKS: u64 = 8;

/// The variant name of a command, for error messages.
fn command_name(command: &Command) -> String {
    let debug = format!("{command:?}");
    debug
        .split(|ch: char| !ch.is_alphanumeric())
        .next()
        .unwrap_or("command")
        .to_owned()
}

fn unavailable(operation: &str) -> CoreError {
    CoreError::Unsupported {
        operation: format!("{operation} in demo mode"),
        required: "a real controller; the demo fleet only simulates device and client actions"
            .into(),
    }
}

impl Simulation {
    /// Apply a command to the simulation.
    pub(crate) fn apply_command(
        &mut self,
        command: Command,
        now: DateTime<Utc>,
    ) -> Result<(CommandResult, Delta), CoreError> {
        let mut delta = Delta::default();
        match command {
            Command::RestartDevice { id } => self.restart(&id, now, &mut delta)?,
            Command::UpgradeDevice { mac, .. } => self.upgrade(&mac, now, &mut delta)?,
            Command::LocateDevice { mac, enable } => {
                let index = self.require_device(&mac)?;
                let state = if enable { "on" } else { "off" };
                let message = format!("Locate LED turned {state} for {}", self.device_name(index));
                self.push_device_event(index, now, "EVT_AD_DeviceLocate", message, &mut delta);
            }
            Command::ProvisionDevice { mac } => {
                let index = self.require_device(&mac)?;
                let message = format!("{} was provisioned", self.device_name(index));
                self.push_device_event(index, now, "EVT_AP_Provisioned", message, &mut delta);
            }
            Command::SpeedtestDevice => {
                let event = self.event(
                    now,
                    (EventCategory::Network, EventSeverity::Info),
                    "EVT_GW_SpeedTest",
                    "Speed test complete: 2.31 Gbps down, 1.94 Gbps up, 6 ms".into(),
                );
                delta.events.push(event);
            }
            Command::PowerCyclePort {
                device_id,
                port_idx,
            } => self.power_cycle(&device_id, port_idx, now, &mut delta)?,
            Command::BlockClient { mac } => self.set_blocked(&mac, true, now, &mut delta)?,
            Command::UnblockClient { mac } => self.set_blocked(&mac, false, now, &mut delta)?,
            Command::KickClient { mac } => {
                let index = self.require_client(&mac)?;
                // UniFi kicks wireless stations off their AP; a wired port
                // has nothing to deauthenticate.
                let client = &self.fleet.clients[index].client;
                if client.client_type != ClientType::Wireless {
                    return Err(CoreError::Rejected {
                        message: format!(
                            "{} is wired; only wireless clients can be kicked",
                            client_label(client)
                        ),
                    });
                }
                if let Some(mac) = self.disconnect(index, now, &mut delta, "EVT_WU_Kicked") {
                    self.returns.push((self.tick + KICK_RETURN_TICKS, mac));
                }
            }
            Command::PatchFirewallPolicy {
                id,
                enabled,
                logging,
            } => self.patch_policy(&id, enabled, logging, now, &mut delta)?,
            other => return Err(unavailable(&command_name(&other))),
        }
        Ok((CommandResult::Ok, delta))
    }

    fn require_device(&self, mac: &MacAddress) -> Result<usize, CoreError> {
        self.device_index_by_mac(mac)
            .ok_or_else(|| CoreError::DeviceNotFound {
                identifier: mac.to_string(),
            })
    }

    fn require_device_id(&self, id: &EntityId) -> Result<usize, CoreError> {
        self.fleet
            .devices
            .iter()
            .position(|device| &device.id == id || device.mac.as_str() == id.to_string())
            .ok_or_else(|| CoreError::DeviceNotFound {
                identifier: id.to_string(),
            })
    }

    fn require_client(&self, mac: &MacAddress) -> Result<usize, CoreError> {
        self.client_index_by_mac(mac)
            .ok_or_else(|| CoreError::ClientNotFound {
                identifier: mac.to_string(),
            })
    }

    fn push_device_event(
        &mut self,
        index: usize,
        now: DateTime<Utc>,
        key: &str,
        message: String,
        delta: &mut Delta,
    ) {
        let mut event = self.event(now, DEVICE_INFO, key, message);
        event.device_mac = self
            .fleet
            .devices
            .get(index)
            .map(|device| device.mac.clone());
        delta.events.push(event);
        delta.devices.extend(self.fleet.devices.get(index).cloned());
    }

    fn restart(
        &mut self,
        id: &EntityId,
        now: DateTime<Utc>,
        delta: &mut Delta,
    ) -> Result<(), CoreError> {
        let index = self.require_device_id(id)?;
        let name = self.device_name(index);
        let device = &mut self.fleet.devices[index];
        if device.state == DeviceState::Offline {
            return Err(CoreError::Rejected {
                message: format!("{name} is offline and cannot be restarted"),
            });
        }
        device.state = DeviceState::GettingReady;
        self.schedule(RESTART_TICKS, Pending::Restart { device: index });
        let message = format!("{name} was restarted by crew-lead@neonharbor.example");
        self.push_device_event(index, now, "EVT_AD_DeviceRestart", message, delta);
        Ok(())
    }

    fn upgrade(
        &mut self,
        mac: &MacAddress,
        now: DateTime<Utc>,
        delta: &mut Delta,
    ) -> Result<(), CoreError> {
        let index = self.require_device(mac)?;
        let name = self.device_name(index);
        let device = &mut self.fleet.devices[index];
        if !device.firmware_updatable {
            return Err(CoreError::Rejected {
                message: format!("{name} is already running the latest firmware"),
            });
        }
        device.state = DeviceState::Updating;
        device.firmware_updatable = false;
        self.schedule(UPGRADE_TICKS, Pending::Upgrade { device: index });
        let message = format!("{name} is upgrading to {AP_FIRMWARE}");
        self.push_device_event(index, now, "EVT_AP_Upgrade_Scheduled", message, delta);
        Ok(())
    }

    fn power_cycle(
        &mut self,
        device_id: &EntityId,
        port_index: u32,
        now: DateTime<Utc>,
        delta: &mut Delta,
    ) -> Result<(), CoreError> {
        let index = self.require_device_id(device_id)?;
        let name = self.device_name(index);
        let port = self.fleet.devices[index]
            .ports
            .iter()
            .find(|port| port.index == port_index)
            .ok_or_else(|| CoreError::ValidationFailed {
                message: format!("{name} has no port {port_index}"),
            })?;
        if port.poe.as_ref().is_none_or(|poe| !poe.enabled) {
            return Err(CoreError::Rejected {
                message: format!("{name} port {port_index} is not supplying PoE"),
            });
        }
        let port_label = port.name.clone();
        let (camera, camera_port) = catalog::OFFLINE_CAMERA;
        let camera_port_hit = index == super::fleet::SWITCH_BASE && port_index == camera_port;
        if camera_port_hit {
            self.schedule(CAMERA_REVIVE_TICKS, Pending::CameraRevive);
        }
        // Name what the port powers when we know it: the dark camera on its
        // port, otherwise the port's own label if it has one.
        let message = if camera_port_hit {
            format!("Power cycled {name} port {port_index} ({camera})")
        } else if let Some(label) = port_label {
            format!("Power cycled {name} port {port_index} ({label})")
        } else {
            format!("Power cycled {name} port {port_index}")
        };
        self.push_device_event(index, now, "EVT_SW_PoePowerCycle", message, delta);
        Ok(())
    }

    fn set_blocked(
        &mut self,
        mac: &MacAddress,
        blocked: bool,
        now: DateTime<Utc>,
        delta: &mut Delta,
    ) -> Result<(), CoreError> {
        let index = self.require_client(mac)?;
        let client = &mut self.fleet.clients[index].client;
        client.blocked = blocked;
        let label = client_label(client);
        let updated = client.clone();
        let (key, verb) = if blocked {
            ("EVT_AD_ClientBlocked", "blocked")
        } else {
            ("EVT_AD_ClientUnblocked", "unblocked")
        };
        let message = format!("{label} was {verb} by crew-lead@neonharbor.example");
        let mut event = self.event(now, CLIENT_INFO, key, message);
        event.client_mac = Some(updated.mac.clone());
        delta.events.push(event);
        delta.clients.push(updated);
        Ok(())
    }

    fn patch_policy(
        &mut self,
        id: &EntityId,
        enabled: Option<bool>,
        logging: Option<bool>,
        now: DateTime<Utc>,
        delta: &mut Delta,
    ) -> Result<(), CoreError> {
        let policy = self
            .fleet
            .policies
            .iter_mut()
            .find(|policy| &policy.id == id)
            .ok_or_else(|| CoreError::NotFound {
                entity_type: "firewall policy".into(),
                identifier: id.to_string(),
            })?;
        if let Some(enabled) = enabled {
            policy.enabled = enabled;
        }
        if let Some(logging) = logging {
            policy.logging_enabled = logging;
        }
        let updated = policy.clone();
        let state = if updated.enabled {
            "enabled"
        } else {
            "disabled"
        };
        let message = format!("Firewall policy \"{}\" {state}", updated.name);
        delta
            .events
            .push(self.event(now, ADMIN_INFO, "EVT_AD_FirewallPolicyChanged", message));
        delta.policies.push(updated);
        Ok(())
    }
}
