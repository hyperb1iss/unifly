use std::future::Future;

use color_eyre::eyre::Result;
use tracing::warn;

use unifly_api::{Command, EntityId, MacAddress};

use super::App;
use crate::tui::action::{Action, ConfirmRequest, Notice, Notification};

/// The action that runs `command` with `pending` and `done` toasts.
fn run(command: Command, pending: String, done: String) -> Action {
    Action::Run {
        command: Box::new(command),
        notice: Notice::new(pending, done),
    }
}

impl App {
    #[allow(clippy::too_many_lines)]
    pub(super) fn handle_command_action(&mut self, action: &Action) -> Result<bool> {
        match action {
            Action::RequestRestart(id) => {
                let name = self.resolve_device_name(id);
                self.confirm(
                    ConfirmRequest::new(
                        format!("Restart {name}?"),
                        run(
                            Command::RestartDevice { id: id.clone() },
                            format!("Restarting {name}\u{2026}"),
                            format!("Restart sent to {name}"),
                        ),
                    )
                    .detail("Everything connected through it drops until it is back."),
                );
            }
            Action::RequestAdopt(mac) => {
                self.confirm(ConfirmRequest::new(
                    format!("Adopt device {mac}?"),
                    run(
                        Command::AdoptDevice {
                            mac: MacAddress::new(mac),
                            ignore_device_limit: false,
                        },
                        format!("Adopting {mac}\u{2026}"),
                        format!("Adoption started for {mac}"),
                    ),
                ));
            }
            Action::RequestUnadopt(id) => {
                let name = self.resolve_device_name(id);
                self.confirm(
                    ConfirmRequest::new(
                        format!("Remove {name} from this site?"),
                        run(
                            Command::RemoveDevice { id: id.clone() },
                            format!("Removing {name}\u{2026}"),
                            format!("Removed {name}"),
                        ),
                    )
                    .detail("It is factory reset and has to be adopted again."),
                );
            }
            Action::RequestLocate(id) => {
                let name = self.resolve_device_name(id);
                let Some(mac) = self.resolve_device_mac(id) else {
                    self.report_gone(&name);
                    return Ok(true);
                };
                // The controller does not report the locate LED, so the
                // app tracks what it switched on.
                let enable = !self.locating.remove(id);
                if enable {
                    self.locating.insert(id.clone());
                }
                let (pending, done) = if enable {
                    (
                        format!("Flashing the LED on {name}\u{2026}"),
                        format!("Locating {name}: its LED is flashing (L again to stop)"),
                    )
                } else {
                    (
                        format!("Stopping the LED on {name}\u{2026}"),
                        format!("Stopped locating {name}"),
                    )
                };
                self.run_command(
                    Command::LocateDevice { mac, enable },
                    &Notice::new(pending, done),
                );
            }
            Action::RequestUpgrade(id) => {
                let name = self.resolve_device_name(id);
                let Some(mac) = self.resolve_device_mac(id) else {
                    self.report_gone(&name);
                    return Ok(true);
                };
                self.confirm(
                    ConfirmRequest::new(
                        format!("Upgrade firmware on {name}?"),
                        run(
                            Command::UpgradeDevice {
                                mac,
                                firmware_url: None,
                            },
                            format!("Starting the upgrade on {name}\u{2026}"),
                            format!("Upgrade started on {name}"),
                        ),
                    )
                    .detail("It restarts when the new firmware is in place."),
                );
            }
            Action::RequestPortPowerCycle(device_id, port_idx) => {
                let name = self.resolve_device_name(device_id);
                self.confirm(
                    ConfirmRequest::new(
                        format!("Power cycle port {port_idx} on {name}?"),
                        run(
                            Command::PowerCyclePort {
                                device_id: device_id.clone(),
                                port_idx: *port_idx,
                            },
                            format!("Power cycling port {port_idx} on {name}\u{2026}"),
                            format!("Power cycled port {port_idx} on {name}"),
                        ),
                    )
                    .detail("The device it powers restarts."),
                );
            }
            Action::RequestBlockClient(id) => {
                self.confirm_client(id, |name, mac| {
                    ConfirmRequest::new(
                        format!("Block {name}?"),
                        run(
                            Command::BlockClient { mac },
                            format!("Blocking {name}\u{2026}"),
                            format!("Blocked {name}"),
                        ),
                    )
                    .detail("It is disconnected and cannot rejoin until unblocked.")
                });
            }
            Action::RequestUnblockClient(id) => {
                self.confirm_client(id, |name, mac| {
                    ConfirmRequest::new(
                        format!("Unblock {name}?"),
                        run(
                            Command::UnblockClient { mac },
                            format!("Unblocking {name}\u{2026}"),
                            format!("Unblocked {name}"),
                        ),
                    )
                });
            }
            Action::RequestForgetClient(id) => {
                self.confirm_client(id, |name, mac| {
                    ConfirmRequest::new(
                        format!("Forget {name}?"),
                        run(
                            Command::ForgetClient { mac },
                            format!("Forgetting {name}\u{2026}"),
                            format!("Forgot {name}"),
                        ),
                    )
                    .detail("Its history, name, and fixed IP are deleted.")
                });
            }
            Action::RequestKickClient(id) => {
                self.confirm_client(id, |name, mac| {
                    ConfirmRequest::new(
                        format!("Disconnect {name}?"),
                        run(
                            Command::KickClient { mac },
                            format!("Disconnecting {name}\u{2026}"),
                            format!("Disconnected {name}"),
                        ),
                    )
                    .detail("It may reconnect right away.")
                });
            }
            Action::ShowConfirm(confirm) => {
                self.pending_confirm = Some(confirm.clone());
            }
            Action::ConfirmYes => {
                if let Some(confirm) = self.pending_confirm.take() {
                    self.action_tx.send(*confirm.action)?;
                }
            }
            Action::ConfirmNo => {
                self.pending_confirm = None;
            }
            Action::Run { command, notice } => {
                self.run_command((**command).clone(), notice);
            }
            Action::OpFinished {
                progress,
                notification,
            } => {
                self.finish_progress(*progress, notification);
            }
            Action::ReorderPolicy(policy, direction) => {
                self.reorder_policy(policy, *direction);
            }
            _ => return Ok(false),
        }

        Ok(true)
    }

    /// Ask before `request` runs. Opens on the next action, so a second
    /// request cannot replace a dialog the user is reading.
    pub(super) fn confirm(&mut self, request: ConfirmRequest) {
        if self.pending_confirm.is_none() {
            let _ = self.action_tx.send(Action::ShowConfirm(request));
        }
    }

    /// Ask before a client command, built from the client's name and real
    /// MAC. A client that left since the key was pressed is reported.
    fn confirm_client(
        &mut self,
        id: &EntityId,
        build: impl FnOnce(&str, MacAddress) -> ConfirmRequest,
    ) {
        let name = self.resolve_client_name(id);
        match self.resolve_client_mac(id) {
            Some(mac) => self.confirm(build(&name, mac)),
            None => self.report_gone(&name),
        }
    }

    /// Say that the entity a key was pressed for is no longer there.
    pub(super) fn report_gone(&mut self, name: &str) {
        self.show_notification(Notification::warning(format!(
            "{name} is no longer on the controller"
        )));
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
    pub(super) fn shown_name(&self, name: &str) -> String {
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

    /// Run `command` on the controller. A progress toast shows at once;
    /// when the controller accepts the command its data is fetched again,
    /// so the screens show the change before the toast says it is done.
    pub(super) fn run_command(&mut self, command: Command, notice: &Notice) {
        let Some(controller) = self.controller.clone() else {
            self.show_notification(Notification::error("Not connected"));
            return;
        };
        let done = notice.done.clone();
        self.spawn_op(&notice.pending, async move {
            match controller.execute(command).await {
                Ok(_) => {
                    if let Err(error) = controller.full_refresh().await {
                        warn!(%error, "refresh after a command failed");
                    }
                    Notification::success(done)
                }
                Err(error) => {
                    warn!(%error, "command execution failed");
                    Notification::error(format!("{error}"))
                }
            }
        });
    }

    /// Run controller work in the background under a progress toast that
    /// the notification it returns takes over.
    pub(super) fn spawn_op(
        &mut self,
        pending: &str,
        op: impl Future<Output = Notification> + Send + 'static,
    ) {
        let progress = Some(self.begin_progress(pending));
        let tx = self.action_tx.clone();
        tokio::spawn(async move {
            let notification = op.await;
            let _ = tx.send(Action::OpFinished {
                progress,
                notification,
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn kick_waits_for_confirmation_and_cancel_clears_it() {
        let demo = demo_fleet().await;
        let client = wireless_client(demo.store(), None);
        let mut app = App::new(Some(demo), None, false, true);
        app.process_action(&Action::RequestKickClient(client.id.clone()))
            .expect("request");
        let queued = app.action_rx.try_recv().expect("confirmation queued");
        assert!(
            matches!(&queued, Action::ShowConfirm(confirm) if confirm.prompt.starts_with("Disconnect ")),
            "{queued:?}"
        );
        assert!(app.action_rx.try_recv().is_err());
        app.process_action(&queued).expect("show confirmation");
        assert!(app.pending_confirm.is_some());
        app.process_action(&Action::ConfirmNo).expect("cancel");
        assert!(app.pending_confirm.is_none());
        assert!(app.action_rx.try_recv().is_err());
    }

    #[test]
    fn a_command_for_an_entity_that_left_says_so() {
        let mut app = App::new(None, None, false, true);
        let id = EntityId::Legacy("client-1".into());
        for request in [
            Action::RequestKickClient(id.clone()),
            Action::RequestBlockClient(id.clone()),
            Action::RequestLocate(id.clone()),
            Action::RequestUpgrade(id.clone()),
        ] {
            app.notices.dismiss_sticky();
            app.process_action(&request).expect("request");
            assert!(app.action_rx.try_recv().is_err(), "{request:?} queued work");
            let toast = app.notices.latest().expect("a toast says why");
            assert!(
                toast.message.contains("no longer on the controller"),
                "{request:?}: {}",
                toast.message
            );
        }
    }

    #[tokio::test]
    async fn a_command_shows_progress_then_its_result_and_locate_toggles() {
        let demo = demo_fleet().await;
        let device = demo
            .store()
            .devices_snapshot()
            .iter()
            .find(|device| device.name.is_some())
            .cloned()
            .expect("a named device");
        let name = device.name.clone().unwrap_or_default();
        let mut app = App::new(Some(demo), None, false, true);

        app.process_action(&Action::RequestLocate(device.id.clone()))
            .expect("locate on");
        assert_eq!(
            app.notices.messages(),
            [format!("Flashing the LED on {name}\u{2026}")],
            "progress shows while the command runs"
        );
        let finished = next_action(&mut app).await;
        app.process_action(&finished).expect("result");
        let messages = app.notices.messages();
        assert_eq!(
            messages.len(),
            1,
            "the result takes over the progress toast"
        );
        assert!(
            messages[0].starts_with(&format!("Locating {name}")),
            "{messages:?}"
        );

        app.process_action(&Action::RequestLocate(device.id.clone()))
            .expect("locate off");
        let finished = next_action(&mut app).await;
        app.process_action(&finished).expect("result");
        assert!(
            app.notices
                .messages()
                .contains(&format!("Stopped locating {name}").as_str()),
            "{:?}",
            app.notices.messages()
        );
    }

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
            Action::ShowConfirm(ref confirm)
                if confirm.prompt == "Adopt device aa-bb-cc-dd-ee-ff?"
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
            let mut next = next_action(&mut app).await;
            if matches!(next, Action::ShowConfirm(_)) {
                app.process_action(&next).expect("show confirmation");
                assert_hidden(&mut app, &secrets, "the kick confirmation");
                app.process_action(&Action::ConfirmYes)
                    .expect("confirm kick");
                let run = next_action(&mut app).await;
                app.process_action(&run).expect("run the command");
                next = next_action(&mut app).await;
            }
            assert_hidden(&mut app, &secrets, &format!("progress for {request:?}"));
            assert!(matches!(next, Action::OpFinished { .. }), "{next:?}");
            app.process_action(&next).expect("show the toast");
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
    async fn under_a_sanitizer_roam_history_loads_by_the_real_mac() {
        let demo = demo_fleet().await;
        let client = wireless_client(demo.store(), None);
        let real_mac = client.mac.to_string();
        let (mut app, sanitizer) = sanitized_app(demo, &[]);
        let shown = sanitizer.sanitize_mac(&client.mac).to_string();
        assert_ne!(shown, real_mac);

        app.process_action(&Action::RequestWifiRoamHistory {
            mac: shown.clone(),
            limit: Some(100),
        })
        .expect("roam history request");

        let loaded = next_action(&mut app).await;
        let Action::WifiRoamHistoryLoaded { mac, events } = loaded else {
            panic!("expected the roam history, got {loaded:?}");
        };
        assert_eq!(mac, shown, "the screen knows the client by its stand-in");
        assert!(!events.is_empty());
        let drawn = serde_json::to_string(&*events).unwrap_or_default();
        assert!(!drawn.contains(&real_mac), "{drawn}");
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
        let run = app.action_rx.try_recv().expect("the command is queued");
        app.process_action(&run).expect("run the command");

        let toast = tokio::time::timeout(Duration::from_secs(10), app.action_rx.recv())
            .await
            .expect("the restart reports back")
            .expect("the action channel stays open");
        assert!(
            matches!(toast, Action::OpFinished { ref notification, .. }
                if notification.level == crate::tui::action::NotificationLevel::Success),
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
        let Action::ShowConfirm(confirm) = queued else {
            panic!("expected a confirmation, got {queued:?}");
        };
        assert!(
            confirm.prompt.starts_with("Power cycle port 7 on "),
            "{}",
            confirm.prompt
        );
        assert!(matches!(
            *confirm.action,
            Action::Run { ref command, .. }
                if matches!(**command, Command::PowerCyclePort { ref device_id, port_idx: 7 }
                    if device_id == &EntityId::from("507f1f77bcf86cd799439011"))
        ));
    }
}
