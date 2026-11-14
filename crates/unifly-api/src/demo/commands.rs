// ── Demo command handling ──
//
// Device, port, and client actions succeed against the simulation and
// emit the event a real controller would, and so do the network and
// firewall edits in `config.rs` and edits to an SSID's name, enabled
// state, and visibility. Anything else that would need a real controller
// to mean something (adoption, other WiFi and VPN CRUD, site admin) is
// refused with a clear error instead of pretending.
// with a clear error instead of pretending.

use chrono::{DateTime, Utc};

use super::catalog::{self, AP_FIRMWARE};
use super::sim::{Delta, Pending, Simulation, client_label};
use crate::command::{Command, CommandResult, UpdateWifiBroadcastRequest};
use crate::controller::PortProfileUpdate;
use crate::core_error::CoreError;
use crate::model::{
    ClientType, DeviceState, EntityId, EventCategory, EventSeverity, GuestAuth, MacAddress,
    PoeMode, PortState,
};

const DEVICE_INFO: (EventCategory, EventSeverity) = (EventCategory::Device, EventSeverity::Info);
const CLIENT_INFO: (EventCategory, EventSeverity) = (EventCategory::Client, EventSeverity::Info);
pub(super) const ADMIN_INFO: (EventCategory, EventSeverity) =
    (EventCategory::Admin, EventSeverity::Info);

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

/// What an admin decided about a guest's portal access.
#[derive(Debug, Clone, Copy)]
enum GuestAccess {
    /// Through the portal, for `minutes` when a limit was set.
    Granted { minutes: Option<u32> },
    /// Back to the portal.
    Revoked,
}

fn unavailable(operation: &str) -> CoreError {
    CoreError::Unsupported {
        operation: format!("{operation} in demo mode"),
        required: "a real controller; the demo fleet simulates device and client actions \
                   and network and firewall edits"
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
            Command::ForgetClient { mac } => self.forget_client(&mac, now, &mut delta)?,
            Command::AuthorizeGuest {
                client_id,
                time_limit_minutes,
                ..
            } => {
                let access = GuestAccess::Granted {
                    minutes: time_limit_minutes,
                };
                self.set_guest_access(&client_id, access, now, &mut delta)?;
            }
            Command::UnauthorizeGuest { client_id } => {
                self.set_guest_access(&client_id, GuestAccess::Revoked, now, &mut delta)?;
            }
            Command::SetClientFixedIp { mac, ip, .. } => {
                self.set_fixed_ip(&mac, Some(ip), now, &mut delta)?;
            }
            Command::RemoveClientFixedIp { mac, .. } => {
                self.set_fixed_ip(&mac, None, now, &mut delta)?;
            }
            Command::PatchFirewallPolicy {
                id,
                enabled,
                logging,
                report_flow_enabled,
            } => self.patch_policy(&id, enabled, logging, report_flow_enabled, now, &mut delta)?,
            Command::CreateNetwork(request) => {
                let network = self.create_network(request, now, &mut delta)?;
                return Ok((CommandResult::Network(network), delta));
            }
            Command::UpdateNetwork { id, update } => {
                self.update_network(&id, update, now, &mut delta)?;
            }
            Command::DeleteNetwork { id, .. } => self.delete_network(&id, now, &mut delta)?,
            Command::DeleteFirewallPolicy { id } => self.delete_policy(&id, now, &mut delta)?,
            Command::ReorderFirewallPolicies {
                zone_pair,
                before_system_ids,
                after_system_ids,
            } => self.reorder_policies(
                &zone_pair,
                &before_system_ids,
                &after_system_ids,
                now,
                &mut delta,
            )?,
            Command::UpdateAclRule { id, update } => {
                self.update_acl(&id, update, now, &mut delta)?;
            }
            Command::DeleteAclRule { id } => self.delete_acl(&id, now, &mut delta)?,
            Command::UpdateNatPolicy { id, update } => {
                self.update_nat(&id, update, now, &mut delta)?;
            }
            Command::DeleteNatPolicy { id } => self.delete_nat(&id, now, &mut delta)?,
            Command::UpdateWifiBroadcast { id, update } => {
                self.update_wifi(&id, &update, now, &mut delta)?;
            }
            Command::ArchiveAlarm { id } => self.archive_alarm(&id)?,
            Command::ArchiveAllAlarms => self.archive_all_alarms(),
            other => return Err(unavailable(&command_name(&other))),
        }
        Ok((CommandResult::Ok, delta))
    }

    fn archive_alarm(&mut self, id: &EntityId) -> Result<(), CoreError> {
        let id = id.to_string();
        if !self.alarms().iter().any(|alarm| alarm.id.to_string() == id) {
            return Err(CoreError::Rejected {
                message: format!("no alarm {id}"),
            });
        }
        self.archived_alarms.insert(id);
        Ok(())
    }

    fn archive_all_alarms(&mut self) {
        let ids: Vec<String> = self
            .alarms()
            .iter()
            .map(|alarm| alarm.id.to_string())
            .collect();
        self.archived_alarms.extend(ids);
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

    /// Apply an SSID edit. The name, enabled state, and visibility change;
    /// a passphrase or security mode is accepted and has nothing to show.
    /// Clients stay associated to a disabled SSID: the simulation does not
    /// model the radios dropping them.
    fn update_wifi(
        &mut self,
        id: &EntityId,
        update: &UpdateWifiBroadcastRequest,
        now: DateTime<Utc>,
        delta: &mut Delta,
    ) -> Result<(), CoreError> {
        let wifi = self
            .fleet
            .wifi
            .iter_mut()
            .find(|wifi| &wifi.id == id)
            .ok_or_else(|| CoreError::NotFound {
                entity_type: "WiFi broadcast".into(),
                identifier: id.to_string(),
            })?;
        let before = wifi.name.clone();
        let mut changes = Vec::new();
        if let Some(name) = update.name.as_ref().or(update.ssid.as_ref())
            && name != &before
        {
            wifi.name.clone_from(name);
            changes.push(format!("renamed to \"{name}\""));
        }
        if let Some(enabled) = update.enabled.filter(|enabled| *enabled != wifi.enabled) {
            wifi.enabled = enabled;
            changes.push(if enabled { "enabled" } else { "disabled" }.to_owned());
        }
        if let Some(hidden) = update.hide_ssid.filter(|hidden| *hidden != wifi.hidden) {
            wifi.hidden = hidden;
            changes.push(if hidden { "hidden" } else { "unhidden" }.to_owned());
        }
        if update.passphrase.is_some() {
            changes.push("passphrase changed".to_owned());
        }
        let updated = wifi.clone();

        if updated.name != before {
            // Later renames chain from the generated name clients join by.
            match self
                .ssid_renames
                .iter_mut()
                .find(|(_, current)| current == &before)
            {
                Some((_, current)) => current.clone_from(&updated.name),
                None => self
                    .ssid_renames
                    .push((before.clone(), updated.name.clone())),
            }
            let fleet = &mut self.fleet;
            for demo in fleet.clients.iter_mut().chain(fleet.away.iter_mut()) {
                if let Some(wireless) = demo.client.wireless.as_mut()
                    && wireless.ssid.as_deref() == Some(before.as_str())
                {
                    wireless.ssid = Some(updated.name.clone());
                }
            }
            delta.clients = self
                .fleet
                .clients
                .iter()
                .map(|client| client.client.clone())
                .collect();
        }
        if !changes.is_empty() {
            let message = format!("WiFi \"{before}\" {}", changes.join(", "));
            delta
                .events
                .push(self.event(now, ADMIN_INFO, "EVT_AD_WlanChanged", message));
        }
        delta.wifi.push(updated);
        Ok(())
    }

    /// Drop a client and everything the controller kept about it. Unlike
    /// a disconnect it does not come back.
    fn forget_client(
        &mut self,
        mac: &MacAddress,
        now: DateTime<Utc>,
        delta: &mut Delta,
    ) -> Result<(), CoreError> {
        let index = self.require_client(mac)?;
        let client = self.fleet.clients.remove(index).client;
        let message = format!(
            "{} was forgotten by crew-lead@neonharbor.example",
            client_label(&client)
        );
        let mut event = self.event(now, CLIENT_INFO, "EVT_AD_ClientForgotten", message);
        event.client_mac = Some(client.mac.clone());
        delta.events.push(event);
        delta.removed.push(client.mac);
        Ok(())
    }

    /// Let a guest through the portal, or send it back there.
    fn set_guest_access(
        &mut self,
        id: &EntityId,
        access: GuestAccess,
        now: DateTime<Utc>,
        delta: &mut Delta,
    ) -> Result<(), CoreError> {
        let client = self
            .fleet
            .clients
            .iter_mut()
            .map(|client| &mut client.client)
            .find(|client| &client.id == id)
            .ok_or_else(|| CoreError::ClientNotFound {
                identifier: id.to_string(),
            })?;
        let label = client_label(client);
        if !client.is_guest {
            return Err(CoreError::Rejected {
                message: format!("{label} is not a guest"),
            });
        }
        let (key, verb, expires_at) = match access {
            GuestAccess::Granted { minutes } => (
                "EVT_AD_GuestAuthorized",
                "authorized",
                minutes.map(|minutes| now + chrono::Duration::minutes(i64::from(minutes))),
            ),
            GuestAccess::Revoked => ("EVT_AD_GuestUnauthorized", "unauthorized", None),
        };
        let authorized = matches!(access, GuestAccess::Granted { .. });
        client.guest_auth = Some(GuestAuth {
            authorized,
            method: authorized.then(|| "admin".into()),
            expires_at,
            tx_bytes: None,
            rx_bytes: None,
            elapsed_minutes: None,
        });
        let updated = client.clone();
        let message = format!("Guest {label} was {verb} by crew-lead@neonharbor.example");
        let mut event = self.event(now, CLIENT_INFO, key, message);
        event.client_mac = Some(updated.mac.clone());
        delta.events.push(event);
        delta.clients.push(updated);
        Ok(())
    }

    /// Reserve `ip` for a client, or go back to DHCP with `None`.
    fn set_fixed_ip(
        &mut self,
        mac: &MacAddress,
        ip: Option<std::net::Ipv4Addr>,
        now: DateTime<Utc>,
        delta: &mut Delta,
    ) -> Result<(), CoreError> {
        let index = self.require_client(mac)?;
        let client = &mut self.fleet.clients[index].client;
        client.use_fixedip = ip.is_some();
        client.fixed_ip = ip;
        let label = client_label(client);
        let updated = client.clone();
        let message = match ip {
            Some(ip) => format!("{label} was given the fixed IP {ip}"),
            None => format!("{label} went back to DHCP"),
        };
        let mut event = self.event(now, CLIENT_INFO, "EVT_AD_ClientFixedIp", message);
        event.client_mac = Some(updated.mac.clone());
        delta.events.push(event);
        delta.clients.push(updated);
        Ok(())
    }

    /// Apply a port edit from `update_device_port`: the label and the PoE
    /// mode are simulated, and anything else is refused.
    pub(crate) fn apply_port_update(
        &mut self,
        mac: &MacAddress,
        port_index: u32,
        update: &PortProfileUpdate,
        now: DateTime<Utc>,
    ) -> Result<Delta, CoreError> {
        if update.mode.is_some()
            || update.native_network_id.is_some()
            || update.tagged_network_ids.is_some()
            || update.speed_setting.is_some()
        {
            return Err(unavailable("port VLAN and speed changes"));
        }
        if update.name.is_none() && update.poe_mode.is_none() {
            return Ok(Delta::default());
        }
        let index = self.require_device(mac)?;
        let name = self.device_name(index);
        let port = self.fleet.devices[index]
            .ports
            .iter_mut()
            .find(|port| port.index == port_index)
            .ok_or_else(|| CoreError::ValidationFailed {
                message: format!("{name} has no port {port_index}"),
            })?;
        let mut changes = Vec::new();
        if let Some(mode) = update.poe_mode {
            let link = port.state;
            let poe = port.poe.as_mut().ok_or_else(|| CoreError::Rejected {
                message: format!("{name} port {port_index} has no PoE"),
            })?;
            poe.enabled = mode != PoeMode::Off;
            poe.state = if poe.enabled { link } else { PortState::Down };
            changes.push(format!(
                "PoE turned {}",
                if poe.enabled { "on" } else { "off" }
            ));
        }
        let edit = self.port_edits.entry((index, port_index)).or_default();
        if let Some(label) = &update.name {
            *edit = Some(label.clone());
            changes.push(format!("renamed to \"{label}\""));
        }
        let mut delta = Delta::default();
        let message = format!("{name} port {port_index}: {}", changes.join(", "));
        self.push_device_event(index, now, "EVT_SW_PortConfigChanged", message, &mut delta);
        Ok(delta)
    }

    fn patch_policy(
        &mut self,
        id: &EntityId,
        enabled: Option<bool>,
        logging: Option<bool>,
        report_flow_enabled: Option<bool>,
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
        if let Some(report_flow_enabled) = report_flow_enabled {
            policy.report_flow_enabled = Some(report_flow_enabled);
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
