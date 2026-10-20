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
                    let name = self.resolve_device_name(id);
                    self.execute_command(
                        Command::LocateDevice { mac, enable: true },
                        format!("Locating {name}"),
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
            Action::ReorderPolicy(policy, direction) => {
                self.reorder_policy(policy, *direction);
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

    /// A device's name for a dialog or toast, sanitized like the screens'
    /// copy of it, since the store holds the real one.
    pub(super) fn resolve_device_name(&self, id: &EntityId) -> String {
        self.controller
            .as_ref()
            .and_then(|c| c.store().device_by_id(id))
            .and_then(|d| d.name.clone())
            .map_or_else(
                || self.shown_text(&id.to_string()),
                |name| self.shown_name(&name),
            )
    }

    pub(super) fn resolve_device_mac(&self, id: &EntityId) -> Option<MacAddress> {
        self.controller
            .as_ref()
            .and_then(|c| c.store().device_by_id(id))
            .map(|d| d.mac.clone())
    }

    /// A client's name for a dialog or toast, sanitized like the screens'
    /// copy of it.
    pub(super) fn resolve_client_name(&self, id: &EntityId) -> String {
        self.controller
            .as_ref()
            .and_then(|c| c.store().client_by_id(id))
            .and_then(|c| c.name.clone().or(c.hostname.clone()))
            .map_or_else(
                || self.shown_text(&id.to_string()),
                |name| self.shown_name(&name),
            )
    }

    /// An entity name as the screens show it: the sanitizer, when one is
    /// on, maps a whole name to the same stand-in it gives the lists.
    fn shown_name(&self, name: &str) -> String {
        self.sanitizer.as_ref().map_or_else(
            || name.to_owned(),
            |sanitizer| sanitizer.sanitize_name(name),
        )
    }

    /// Text as it may be drawn. With a sanitizer on, SSIDs, names, WAN
    /// addresses, and MACs inside it become the stand-ins the screens use,
    /// so an error carrying a real identifier cannot leak one onto the
    /// screen.
    pub(super) fn shown_text(&self, text: &str) -> String {
        self.sanitizer.as_ref().map_or_else(
            || text.to_owned(),
            |sanitizer| sanitizer.sanitize_text(text),
        )
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

    /// Show a toast. Every toast passes through here, so its text is
    /// sanitized once for all of them.
    pub(super) fn show_notification(&mut self, notification: Notification) {
        let Notification { message, level } = notification;
        let message = self.shown_text(&message);
        self.notification = Some((Notification { message, level }, Instant::now()));
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
    fn moving_a_policy_never_reorders_the_table_on_its_own() {
        use std::sync::Arc;

        use crossterm::event::{KeyCode, KeyEvent};

        use crate::tui::screen::ScreenId;
        use crate::tui::table::render_lines;

        let zone = |name: &str| {
            Arc::new(
                serde_json::from_value(serde_json::json!({
                    "id": name, "name": name, "network_ids": [], "origin": null
                }))
                .expect("zone fixture"),
            )
        };
        let policy = |name: &str, index: i32| {
            Arc::new(
                serde_json::from_value(serde_json::json!({
                    "id": name, "name": name, "description": null, "enabled": true,
                    "index": index, "action": "Allow", "ip_version": "Both",
                    "source": { "zone_id": "lan", "filter": null },
                    "destination": { "zone_id": "wan", "filter": null },
                    "source_summary": null, "destination_summary": null,
                    "protocol_summary": null, "schedule": null, "ipsec_mode": null,
                    "connection_states": [], "logging_enabled": false,
                    "origin": "UserDefined"
                }))
                .expect("policy fixture"),
            )
        };
        let mut app = App::new(None, None, false, true);
        app.active_screen = ScreenId::Dashboard;
        app.process_action(&Action::FirewallZonesUpdated(Arc::new(vec![
            zone("lan"),
            zone("wan"),
        ])))
        .expect("zones");
        app.process_action(&Action::FirewallPoliciesUpdated(Arc::new(vec![
            policy("allow-first", 10_000),
            policy("block-second", 10_001),
        ])))
        .expect("policies");
        app.switch_screen(ScreenId::Firewall).expect("switch");
        let order = |app: &mut App| {
            let rows = render_lines(120, 24, |frame, _| app.render(frame));
            let at = |name: &str| rows.iter().position(|row| row.contains(name));
            (at("allow-first"), at("block-second"))
        };
        let before = order(&mut app);

        for key in ['j', 'K'] {
            if let Some(action) = app
                .handle_key_event(KeyEvent::from(KeyCode::Char(key)))
                .expect("key")
            {
                app.process_action(&action).expect("action");
            }
        }

        assert_eq!(
            order(&mut app),
            before,
            "the table moved without the controller"
        );
        let reported = std::iter::from_fn(|| app.action_rx.try_recv().ok())
            .any(|action| matches!(action, Action::Notify(ref n) if n.message == "Not connected"));
        assert!(reported, "the failed move went unreported");
    }

    #[test]
    fn drawn_text_keeps_macs_times_paths_and_stand_ins() {
        use std::sync::Arc;

        use crate::config::DemoConfig;
        use crate::sanitizer::Sanitizer;

        let app = App::new(
            None,
            Some(Arc::new(Sanitizer::new(&DemoConfig::default()))),
            false,
            true,
        );
        let text = "Device not found: aa:bb:cc:dd:ee:ff at 12:34:56 via unifly::tui";
        assert_eq!(app.shown_text(text), text);

        let sanitizer = Arc::new(Sanitizer::new(&DemoConfig {
            redact_macs: true,
            ..DemoConfig::default()
        }));
        let stand_in = sanitizer.sanitize_mac(&MacAddress::new("aa:bb:cc:dd:ee:ff"));
        let app = App::new(None, Some(sanitizer), false, true);
        let toast = format!("WiFi roam history: no client {stand_in}");
        assert_eq!(app.shown_text(&toast), toast, "a stand-in was mapped again");
    }

    /// A connected demo fleet, for tests that need real-looking entities.
    async fn demo_fleet() -> unifly_api::Controller {
        let demo = unifly_api::Controller::demo(unifly_api::DemoFleetOptions::default());
        demo.connect().await.expect("demo fleet connects");
        demo
    }

    /// A named wireless client with an IP and an SSID, other than `except`.
    fn wireless_client(
        store: &unifly_api::DataStore,
        except: Option<&EntityId>,
    ) -> std::sync::Arc<unifly_api::Client> {
        store
            .clients_snapshot()
            .iter()
            .find(|client| {
                Some(&client.id) != except
                    && client.name.is_some()
                    && client.ip.is_some()
                    && client.wireless.as_ref().is_some_and(|w| w.ssid.is_some())
            })
            .cloned()
            .expect("a named wireless client")
    }

    /// An App over `demo` that redacts `names`, SSIDs, and MACs, and the
    /// sanitizer it shares with the data bridge.
    fn sanitized_app(
        demo: unifly_api::Controller,
        names: &[&str],
    ) -> (App, std::sync::Arc<crate::sanitizer::Sanitizer>) {
        let sanitizer = std::sync::Arc::new(crate::sanitizer::Sanitizer::new(
            &crate::config::DemoConfig {
                enabled: true,
                redact_names: names.iter().map(ToString::to_string).collect(),
                redact_ssids: true,
                redact_macs: true,
                ..crate::config::DemoConfig::default()
            },
        ));
        let mut app = App::new(
            Some(demo),
            Some(std::sync::Arc::clone(&sanitizer)),
            false,
            true,
        );
        app.active_screen = crate::tui::screen::ScreenId::Dashboard;
        (app, sanitizer)
    }

    /// Fail if the frame `app` draws shows any of `secrets`.
    fn assert_hidden(app: &mut App, secrets: &[&str], what: &str) {
        let text =
            crate::tui::table::render_lines(140, 30, |frame, _| app.render(frame)).join("\n");
        for secret in secrets {
            assert!(!text.contains(secret), "{what} shows {secret}:\n{text}");
        }
    }

    async fn next_action(app: &mut App) -> Action {
        tokio::time::timeout(std::time::Duration::from_secs(10), app.action_rx.recv())
            .await
            .expect("the request reports back")
            .expect("the action channel stays open")
    }

    #[tokio::test]
    async fn under_a_sanitizer_dialogs_and_toasts_show_no_real_names_or_macs() {
        let demo = demo_fleet().await;
        let store = std::sync::Arc::clone(demo.store());
        let device = store
            .devices_snapshot()
            .iter()
            .find(|device| device.name.is_some())
            .cloned()
            .expect("a named device");
        let client = wireless_client(&store, None);
        let device_name = device.name.clone().unwrap_or_default();
        let client_name = client.name.clone().unwrap_or_default();
        let (device_mac, client_mac) = (device.mac.to_string(), client.mac.to_string());
        let secrets = [
            device_name.as_str(),
            device_mac.as_str(),
            client_name.as_str(),
            client_mac.as_str(),
        ];
        let (mut app, _) = sanitized_app(demo, &[&device_name, &client_name]);

        app.process_action(&Action::RequestRestart(device.id.clone()))
            .expect("restart request");
        let confirm = app.action_rx.try_recv().expect("a confirmation is queued");
        app.process_action(&confirm).expect("show the confirmation");
        assert!(app.pending_confirm.is_some());
        assert_hidden(&mut app, &secrets, "the restart confirmation");
        app.process_action(&Action::ConfirmNo).expect("dismiss");

        for request in [
            Action::RequestLocate(device.id.clone()),
            Action::RequestKickClient(client.id.clone()),
        ] {
            app.process_action(&request).expect("request");
            let toast = next_action(&mut app).await;
            assert!(matches!(toast, Action::Notify(_)), "{toast:?}");
            app.process_action(&toast).expect("show the toast");
            assert_hidden(&mut app, &secrets, &format!("the toast for {request:?}"));
        }

        // An error from the controller can carry real identifiers too.
        app.process_action(&Action::Notify(Notification::error(format!(
            "{device_name} ({device_mac}) did not answer"
        ))))
        .expect("show the error");
        assert_hidden(&mut app, &secrets, "an error toast");
    }

    #[tokio::test]
    async fn under_a_sanitizer_wifi_strings_show_no_real_ssids_or_bssids() {
        let demo = demo_fleet().await;
        let store = std::sync::Arc::clone(demo.store());
        let client = wireless_client(&store, None);
        let client_name = client.name.clone().unwrap_or_default();
        let ssid = client
            .wireless
            .as_ref()
            .and_then(|wireless| wireless.ssid.clone())
            .unwrap_or_default();
        let ap_macs: Vec<String> = store
            .devices_snapshot()
            .iter()
            .filter(|device| device.device_type == unifly_api::DeviceType::AccessPoint)
            .map(|device| device.mac.to_string())
            .collect();
        let client_mac = client.mac.to_string();
        let secrets: Vec<&str> = [ssid.as_str(), client_mac.as_str(), client_name.as_str()]
            .into_iter()
            .chain(ap_macs.iter().map(String::as_str))
            .collect();
        let (mut app, sanitizer) = sanitized_app(demo, &[&client_name]);

        // The WiFi detail payload is what the WiFi screen's panel draws.
        let ip = client.ip.map(|ip| ip.to_string()).unwrap_or_default();
        app.process_action(&Action::RequestWifiClientDetail(ip))
            .expect("detail request");
        let loaded = next_action(&mut app).await;
        let Action::WifiClientDetailLoaded { data, .. } = loaded else {
            panic!("expected the detail, got {loaded:?}");
        };
        let detail = data.to_string();
        for secret in &secrets {
            assert!(
                !detail.contains(secret),
                "the WiFi detail carries {secret}: {detail}"
            );
        }

        // An event, sanitized as the data bridge passes it on.
        let event: unifly_api::Event = serde_json::from_value(serde_json::json!({
            "id": "evt-ssid", "timestamp": "2026-10-12T09:00:00Z",
            "category": "Client", "severity": "Info", "event_type": "EVT_WU_Connected",
            "message": format!("{client_name} connected to \"{ssid}\" via {client_mac}"),
            "device_mac": null, "client_mac": null, "site_id": null, "raw_key": null
        }))
        .expect("event fixture");
        let event = std::sync::Arc::new(sanitizer.sanitize_event(&event));
        app.process_action(&Action::EventReceived(event))
            .expect("event");
        app.switch_screen(crate::tui::screen::ScreenId::Events)
            .expect("switch to Events");
        let events =
            crate::tui::table::render_lines(140, 30, |frame, _| app.render(frame)).join("\n");
        assert!(
            events.contains("connected to"),
            "the event is not drawn:\n{events}"
        );
        assert_hidden(&mut app, &secrets, "the Events screen");

        app.process_action(&Action::Notify(Notification::error(format!(
            "roamed to {ssid} via {}",
            ap_macs.first().map_or("", String::as_str)
        ))))
        .expect("show the toast");
        assert_hidden(&mut app, &secrets, "a toast");
    }

    #[tokio::test]
    async fn restart_goes_through_the_cloud_connector() {
        use std::time::Duration;

        use wiremock::matchers::{body_json, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        use crate::tui::screen::ScreenId;

        const SITE: &str = "550e8400-e29b-41d4-a716-446655440000";
        const DEVICE: &str = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
        let server = MockServer::start().await;
        let base = "/v1/connector/consoles/console-1/proxy/network/integration/v1";
        Mock::given(method("GET"))
            .and(path(format!("{base}/sites")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "offset": 0, "limit": 50, "count": 1, "totalCount": 1,
                "data": [{ "id": SITE, "internalReference": "default", "name": "Default" }],
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!(
                "{base}/sites/{SITE}/devices/{DEVICE}/actions"
            )))
            .and(body_json(serde_json::json!({ "action": "RESTART" })))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let controller = unifly_api::Controller::new(unifly_api::ControllerConfig {
            url: server.uri().parse().expect("mock server URL"),
            auth: unifly_api::AuthCredentials::Cloud {
                api_key: "cloud-key".to_owned().into(),
                host_id: "console-1".into(),
            },
            site: "default".into(),
            timeout: Duration::from_secs(5),
            refresh_interval_secs: 0,
            websocket_enabled: false,
            ..unifly_api::ControllerConfig::default()
        });
        controller
            .connect_lightweight()
            .await
            .expect("the connector resolves the site");
        let mut app = App::new(Some(controller), None, false, true);
        app.active_screen = ScreenId::Dashboard;

        app.process_action(&Action::RequestRestart(EntityId::from(DEVICE)))
            .expect("restart request");
        let confirm = app.action_rx.try_recv().expect("a confirmation is queued");
        app.process_action(&confirm).expect("show the confirmation");
        app.process_action(&Action::ConfirmYes).expect("confirm");

        let toast = tokio::time::timeout(Duration::from_secs(10), app.action_rx.recv())
            .await
            .expect("the restart reports back")
            .expect("the action channel stays open");
        assert!(
            matches!(toast, Action::Notify(ref n) if n.level == crate::tui::action::NotificationLevel::Success),
            "{toast:?}"
        );
        server.verify().await;
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
