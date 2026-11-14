//! App-side work for the Devices and Clients screens: commands that need
//! a real MAC from the store, port profiles, and port writes.

use std::net::Ipv4Addr;
use std::sync::Arc;

use color_eyre::eyre::Result;
use tracing::warn;

use unifly_api::model::{PoeMode, PortProfile};
use unifly_api::{Command, CoreError, EntityId, PortProfileUpdate};

use super::App;
use crate::sanitizer::Sanitizer;
use crate::tui::action::{Action, ConfirmRequest, Notice, Notification, PortChange};
use crate::tui::screen::ScreenId;

/// Why port profiles and port writes are out of reach without a session.
const NEEDS_SESSION: &str = "port settings need a direct controller session, \
     which a cloud profile does not have";

fn run(command: Command, pending: String, done: String) -> Action {
    Action::Run {
        command: Box::new(command),
        notice: Notice::new(pending, done),
    }
}

impl App {
    pub(super) fn handle_devices_clients_action(&mut self, action: &Action) -> Result<bool> {
        match action {
            Action::RequestAdoptDevice(id) => self.confirm_adopt(id),
            Action::RequestProvision(id) => self.confirm_provision(id),
            Action::RequestPortProfiles(id) => self.fetch_port_profiles(id),
            Action::PortProfilesLoaded { .. } | Action::PortProfilesFailed { .. } => {
                self.forward_to_screen(ScreenId::Devices, action)?;
            }
            Action::RequestPortUpdate {
                device,
                port,
                change,
            } => self.update_port(device, *port, change),
            Action::RequestSetFixedIp { client, ip } => self.set_fixed_ip(client, *ip),
            Action::RequestRemoveFixedIp(id) => self.confirm_remove_fixed_ip(id),
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn confirm_adopt(&mut self, id: &EntityId) {
        let name = self.resolve_device_name(id);
        let Some(mac) = self.resolve_device_mac(id) else {
            self.report_gone(&name);
            return;
        };
        self.confirm(
            ConfirmRequest::new(
                format!("Adopt {name}?"),
                run(
                    Command::AdoptDevice {
                        mac,
                        ignore_device_limit: false,
                    },
                    format!("Adopting {name}\u{2026}"),
                    format!("Adoption started for {name}"),
                ),
            )
            .detail("It joins this site and takes the site's configuration."),
        );
    }

    fn confirm_provision(&mut self, id: &EntityId) {
        let name = self.resolve_device_name(id);
        let Some(mac) = self.resolve_device_mac(id) else {
            self.report_gone(&name);
            return;
        };
        self.confirm(
            ConfirmRequest::new(
                format!("Provision {name} again?"),
                run(
                    Command::ProvisionDevice { mac },
                    format!("Provisioning {name}\u{2026}"),
                    format!("Provisioning sent to {name}"),
                ),
            )
            .detail("Its configuration is pushed again; clients may drop for a moment."),
        );
    }

    /// Fetch a device's port profiles and hand them to the Devices screen,
    /// or tell it why they cannot be read.
    fn fetch_port_profiles(&self, id: &EntityId) {
        let tx = self.action_tx.clone();
        let device = id.clone();
        let failed = |error: &str| Action::PortProfilesFailed {
            device: id.clone(),
            error: error.to_owned(),
        };
        let Some(controller) = self.controller.clone() else {
            let _ = tx.send(failed("not connected"));
            return;
        };
        if !self.access.session {
            let _ = tx.send(failed(NEEDS_SESSION));
            return;
        }
        let Some(mac) = self.resolve_device_mac(id) else {
            let _ = tx.send(failed("the device is no longer on the controller"));
            return;
        };
        let sanitizer = self.sanitizer.clone();
        tokio::spawn(async move {
            let action = match controller.list_device_ports(&mac).await {
                Ok(ports) => Action::PortProfilesLoaded {
                    device,
                    ports: Arc::new(shown_profiles(ports, sanitizer.as_deref())),
                },
                Err(error) => {
                    warn!(%error, "port profiles could not be read");
                    let error = error.to_string();
                    Action::PortProfilesFailed {
                        device,
                        error: sanitizer
                            .as_ref()
                            .map_or_else(|| error.clone(), |s| s.sanitize_text(&error)),
                    }
                }
            };
            let _ = tx.send(action);
        });
    }

    /// Write one port's change, refresh the controller's data, and reload
    /// the port profiles the Devices screen shows.
    fn update_port(&mut self, id: &EntityId, port: u32, change: &PortChange) {
        let Some(controller) = self.controller.clone() else {
            self.show_notification(Notification::error("Not connected"));
            return;
        };
        let name = self.resolve_device_name(id);
        if !self.access.session {
            self.show_notification(Notification::warning(format!(
                "Port {port} on {name} was not changed: {NEEDS_SESSION}"
            )));
            return;
        }
        let Some(mac) = self.resolve_device_mac(id) else {
            self.report_gone(&name);
            return;
        };
        let (update, pending, done) = match change {
            PortChange::Rename(label) => (
                PortProfileUpdate {
                    name: Some(label.clone()),
                    ..PortProfileUpdate::default()
                },
                format!("Renaming port {port} on {name}\u{2026}"),
                format!("Port {port} on {name} is now \"{label}\""),
            ),
            PortChange::Poe(mode) => {
                let state = if *mode == PoeMode::Off { "off" } else { "on" };
                (
                    PortProfileUpdate {
                        poe_mode: Some(*mode),
                        ..PortProfileUpdate::default()
                    },
                    format!("Turning PoE {state} for port {port} of {name}\u{2026}"),
                    format!("PoE turned {state} for port {port} of {name}"),
                )
            }
        };
        let tx = self.action_tx.clone();
        let device = id.clone();
        self.spawn_op(&pending, async move {
            if let Err(error) = controller.update_device_port(&mac, port, &update).await {
                warn!(%error, "port update failed");
                return Notification::error(format!("{error}"));
            }
            if let Err(error) = controller.full_refresh().await {
                warn!(%error, "refresh after a port update failed");
            }
            let _ = tx.send(Action::RequestPortProfiles(device));
            Notification::success(done)
        });
    }

    /// Reserve `ip` for the client. The controller's own network records
    /// pick the network whose subnet holds the address, as `unifly clients
    /// set-ip` does, and the toast reports what the refreshed record shows.
    fn set_fixed_ip(&mut self, id: &EntityId, ip: Ipv4Addr) {
        let name = self.resolve_client_name(id);
        let Some(controller) = self.controller.clone() else {
            self.show_notification(Notification::error("Not connected"));
            return;
        };
        let Some(mac) = self.resolve_client_mac(id) else {
            self.report_gone(&name);
            return;
        };
        let command = Command::SetClientFixedIp {
            mac: mac.clone(),
            ip,
            network_id: None,
        };
        self.spawn_op(&format!("Reserving {ip} for {name}\u{2026}"), async move {
            match controller.execute(command).await {
                Ok(_) => {}
                Err(CoreError::ValidationFailed { message }) => {
                    return Notification::warning(format!("{name} keeps its address: {message}"));
                }
                Err(error) => {
                    warn!(%error, "setting a fixed IP failed");
                    return Notification::error(error.to_string());
                }
            }
            if let Err(error) = controller.full_refresh().await {
                warn!(%error, "refresh after setting a fixed IP failed");
            }
            let took = controller
                .store()
                .client_by_mac(&mac)
                .is_some_and(|client| client.use_fixedip && client.fixed_ip == Some(ip));
            if took {
                Notification::success(format!("{name} is fixed at {ip} from its next lease"))
            } else {
                Notification::warning(format!(
                    "The controller accepted {ip} for {name}, but its record does not show it yet"
                ))
            }
        });
    }

    fn confirm_remove_fixed_ip(&mut self, id: &EntityId) {
        let name = self.resolve_client_name(id);
        let Some(mac) = self.resolve_client_mac(id) else {
            self.report_gone(&name);
            return;
        };
        self.confirm(
            ConfirmRequest::new(
                format!("Remove the fixed IP of {name}?"),
                run(
                    Command::RemoveClientFixedIp {
                        mac,
                        network_id: None,
                    },
                    format!("Removing the fixed IP of {name}\u{2026}"),
                    format!("{name} takes its address from DHCP again"),
                ),
            )
            .detail("Its address may change at the next lease."),
        );
    }
}

/// Port profiles as they may be drawn: under a sanitizer the labels and
/// network names become the stand-ins the other screens show.
fn shown_profiles(mut ports: Vec<PortProfile>, sanitizer: Option<&Sanitizer>) -> Vec<PortProfile> {
    if let Some(sanitizer) = sanitizer {
        for port in &mut ports {
            port.name = sanitizer.sanitize_name_opt(&port.name);
            port.native_network_name = sanitizer.sanitize_name_opt(&port.native_network_name);
            for name in &mut port.tagged_network_names {
                *name = sanitizer.sanitize_name(name);
            }
        }
    }
    ports
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use unifly_api::{ClientType, DeviceType};

    use super::*;
    use crate::tui::action::NotificationLevel;

    async fn demo_fleet() -> unifly_api::Controller {
        let demo = unifly_api::Controller::demo(unifly_api::DemoFleetOptions::default());
        demo.connect().await.expect("demo fleet connects");
        demo
    }

    /// What the data bridge reports for the demo fleet: everything.
    fn demo_access() -> crate::tui::action::Access {
        crate::tui::action::Access {
            integration: true,
            session: true,
            live_events: true,
        }
    }

    async fn next_action(app: &mut App) -> Action {
        tokio::time::timeout(Duration::from_secs(10), app.action_rx.recv())
            .await
            .expect("the request reports back")
            .expect("the action channel stays open")
    }

    /// Run queued actions until one matches `wanted`, and return it.
    async fn run_until(app: &mut App, wanted: impl Fn(&Action) -> bool) -> Action {
        loop {
            let action = next_action(app).await;
            if wanted(&action) {
                return action;
            }
            app.process_action(&action).expect("action");
        }
    }

    fn first_switch(demo: &unifly_api::Controller) -> Arc<unifly_api::Device> {
        demo.store()
            .devices_snapshot()
            .iter()
            .find(|device| device.device_type == DeviceType::Switch)
            .cloned()
            .expect("a switch")
    }

    #[tokio::test]
    async fn setting_a_fixed_ip_runs_against_the_store_and_refuses_a_stray_address() {
        let demo = demo_fleet().await;
        let client = demo
            .store()
            .clients_snapshot()
            .iter()
            .find(|client| {
                !client.use_fixedip && matches!(client.ip, Some(std::net::IpAddr::V4(_)))
            })
            .cloned()
            .expect("a client on DHCP");
        let Some(std::net::IpAddr::V4(ip)) = client.ip else {
            panic!("an IPv4 client");
        };
        let store = Arc::clone(demo.store());
        let mut app = App::new(Some(demo), None, false, true);

        app.process_action(&Action::RequestSetFixedIp {
            client: client.id.clone(),
            ip: Ipv4Addr::new(192, 0, 2, 9),
        })
        .expect("stray address");
        let refused = next_action(&mut app).await;
        assert!(
            matches!(&refused, Action::OpFinished { notification, .. }
                if notification.level == NotificationLevel::Warning
                    && notification.message.contains("not inside any network's subnet")),
            "{refused:?}"
        );
        assert!(
            !store
                .client_by_id(&client.id)
                .expect("still connected")
                .use_fixedip
        );

        app.process_action(&Action::RequestSetFixedIp {
            client: client.id.clone(),
            ip,
        })
        .expect("set");
        let finished = next_action(&mut app).await;
        assert!(
            matches!(&finished, Action::OpFinished { notification, .. }
                if notification.level == NotificationLevel::Success),
            "{finished:?}"
        );
        let stored = store.client_by_id(&client.id).expect("still connected");
        assert!(stored.use_fixedip);
        assert_eq!(stored.fixed_ip, Some(ip));

        app.process_action(&Action::RequestRemoveFixedIp(client.id.clone()))
            .expect("remove");
        let confirm = app.action_rx.try_recv().expect("removal asks first");
        assert!(
            matches!(&confirm, Action::ShowConfirm(confirm) if confirm.prompt.starts_with("Remove the fixed IP of ")),
            "{confirm:?}"
        );
    }

    #[tokio::test]
    async fn a_port_rename_is_written_and_the_profiles_reload_with_it() {
        let demo = demo_fleet().await;
        let switch = first_switch(&demo);
        let mut app = App::new(Some(demo), None, false, true);

        app.access = demo_access();
        app.process_action(&Action::RequestPortUpdate {
            device: switch.id.clone(),
            port: 3,
            change: PortChange::Rename("cam-dock".into()),
        })
        .expect("rename");
        assert_eq!(app.notices.messages().len(), 1, "progress shows at once");

        let loaded = run_until(&mut app, |action| {
            matches!(action, Action::PortProfilesLoaded { .. })
        })
        .await;
        let Action::PortProfilesLoaded { device, ports } = loaded else {
            unreachable!();
        };
        assert_eq!(device, switch.id);
        let renamed = ports.iter().find(|port| port.index == 3).expect("port 3");
        assert_eq!(renamed.name.as_deref(), Some("cam-dock"));
        assert!(
            app.notices
                .messages()
                .iter()
                .any(|message| message.ends_with("is now \"cam-dock\"")),
            "{:?}",
            app.notices.messages()
        );
    }

    #[tokio::test]
    async fn turning_poe_off_reaches_the_device_list() {
        let demo = demo_fleet().await;
        // The store lists devices in no fixed order, and not every switch
        // supplies PoE.
        let powers =
            |port: &unifly_api::model::Port| port.poe.as_ref().is_some_and(|poe| poe.enabled);
        let switch = demo
            .store()
            .devices_snapshot()
            .iter()
            .find(|device| device.ports.iter().any(powers))
            .cloned()
            .expect("a device supplying PoE");
        let port = switch
            .ports
            .iter()
            .find(|port| powers(port))
            .map(|port| port.index)
            .expect("a port supplying PoE");
        let store = Arc::clone(demo.store());
        let mut app = App::new(Some(demo), None, false, true);

        app.access = demo_access();
        app.process_action(&Action::RequestPortUpdate {
            device: switch.id.clone(),
            port,
            change: PortChange::Poe(PoeMode::Off),
        })
        .expect("PoE off");
        let finished = run_until(&mut app, |action| {
            matches!(action, Action::OpFinished { .. })
        })
        .await;
        assert!(
            matches!(&finished, Action::OpFinished { notification, .. }
                if notification.level == NotificationLevel::Success),
            "{finished:?}"
        );
        let poe = store
            .device_by_id(&switch.id)
            .and_then(|device| {
                device
                    .ports
                    .iter()
                    .find(|candidate| candidate.index == port)
                    .and_then(|port| port.poe.clone())
            })
            .expect("the port still has PoE");
        assert!(!poe.enabled);
    }

    #[tokio::test]
    async fn without_a_session_port_profiles_say_so_and_writes_are_refused() {
        let mut app = App::new(None, None, false, true);
        let id = EntityId::from("device-1");
        app.process_action(&Action::RequestPortProfiles(id.clone()))
            .expect("profiles");
        let answer = app.action_rx.try_recv().expect("the screen gets an answer");
        assert!(
            matches!(&answer, Action::PortProfilesFailed { error, .. } if error == "not connected"),
            "{answer:?}"
        );

        // A controller without Session access, as a cloud profile is.
        let controller = unifly_api::Controller::new(unifly_api::ControllerConfig::default());
        let mut app = App::new(Some(controller), None, false, true);
        app.process_action(&Action::RequestPortProfiles(id.clone()))
            .expect("profiles");
        let answer = app.action_rx.try_recv().expect("the screen gets an answer");
        assert!(
            matches!(&answer, Action::PortProfilesFailed { error, .. } if error.contains("direct controller session")),
            "{answer:?}"
        );
        app.process_action(&Action::RequestPortUpdate {
            device: id,
            port: 1,
            change: PortChange::Poe(PoeMode::Off),
        })
        .expect("update");
        let toast = app.notices.latest().expect("a toast says why");
        assert_eq!(toast.level, NotificationLevel::Warning);
        assert!(
            toast.message.contains("was not changed"),
            "{}",
            toast.message
        );
    }

    #[tokio::test]
    async fn adopt_and_provision_ask_first_and_carry_the_real_mac() {
        let demo = demo_fleet().await;
        let switch = first_switch(&demo);
        let name = switch.name.clone().unwrap_or_default();
        let mut app = App::new(Some(demo), None, false, true);

        app.process_action(&Action::RequestAdoptDevice(switch.id.clone()))
            .expect("adopt");
        let Action::ShowConfirm(confirm) = app.action_rx.try_recv().expect("adopt asks") else {
            panic!("expected a confirmation");
        };
        assert_eq!(confirm.prompt, format!("Adopt {name}?"));
        assert!(matches!(
            *confirm.action,
            Action::Run { ref command, .. }
                if matches!(**command, Command::AdoptDevice { ref mac, .. } if *mac == switch.mac)
        ));

        app.process_action(&Action::RequestProvision(switch.id.clone()))
            .expect("provision");
        let Action::ShowConfirm(confirm) = app.action_rx.try_recv().expect("provision asks") else {
            panic!("expected a confirmation");
        };
        assert!(matches!(
            *confirm.action,
            Action::Run { ref command, .. }
                if matches!(**command, Command::ProvisionDevice { ref mac } if *mac == switch.mac)
        ));

        // A device that left is reported instead of asked about.
        app.process_action(&Action::RequestProvision(EntityId::from("gone")))
            .expect("provision");
        assert!(app.action_rx.try_recv().is_err());
        let toast = app.notices.latest().expect("a toast says why");
        assert!(toast.message.contains("no longer on the controller"));
    }

    #[tokio::test]
    async fn the_demo_fleet_takes_the_client_commands_the_screen_offers() {
        let demo = demo_fleet().await;
        let store = Arc::clone(demo.store());
        let guest = store
            .clients_snapshot()
            .iter()
            .find(|client| client.is_guest && client.client_type == ClientType::Wireless)
            .cloned()
            .expect("a guest");

        demo.execute(Command::UnauthorizeGuest {
            client_id: guest.id.clone(),
        })
        .await
        .expect("revoke");
        let revoked = store.client_by_id(&guest.id).expect("still connected");
        assert!(
            !revoked
                .guest_auth
                .as_ref()
                .is_some_and(|auth| auth.authorized)
        );

        demo.execute(Command::AuthorizeGuest {
            client_id: guest.id.clone(),
            time_limit_minutes: Some(90),
            data_limit_mb: None,
            rx_rate_kbps: None,
            tx_rate_kbps: None,
        })
        .await
        .expect("authorize");
        let auth = store
            .client_by_id(&guest.id)
            .and_then(|client| client.guest_auth.clone())
            .expect("guest access");
        assert!(auth.authorized && auth.expires_at.is_some());

        demo.execute(Command::ForgetClient {
            mac: guest.mac.clone(),
        })
        .await
        .expect("forget");
        assert!(store.client_by_id(&guest.id).is_none());
    }
}
