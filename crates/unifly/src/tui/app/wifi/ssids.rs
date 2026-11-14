//! App-side work on SSIDs: reading one in full, and writing a change so
//! that it is checked against what the controller then reports.

use std::sync::Arc;

use color_eyre::eyre::Result;
use tracing::warn;
use unifly_api::model::WifiBroadcast;
use unifly_api::{Command, Controller, EntityId, UpdateWifiBroadcastRequest};

use crate::tui::action::{Action, ConfirmRequest, Notice, Notification, SsidChange};
use crate::tui::app::App;
use crate::tui::screen::ScreenId;

/// What every SSID change costs before its own effect.
const REPROVISION: &str = "Access points reprovision, so clients may drop for a moment.";

impl App {
    /// An SSID as the screens show it: under redaction its name is the
    /// stand-in the lists use.
    fn shown_wifi(&self, wifi: &WifiBroadcast) -> WifiBroadcast {
        self.sanitizer
            .as_ref()
            .map_or_else(|| wifi.clone(), |sanitizer| sanitizer.sanitize_wifi(wifi))
    }

    /// The name the screens show for the SSID `id`, from the store.
    fn shown_ssid_name(&self, id: &EntityId) -> Option<String> {
        let controller = self.controller.as_ref()?;
        let snapshot = controller.wifi_broadcasts_snapshot();
        let ssid = snapshot.iter().find(|ssid| &ssid.id == id)?;
        Some(self.shown_wifi(ssid).name)
    }

    /// Read one SSID's full record for the WiFi screen.
    pub(super) fn fetch_wifi_broadcast_detail(&self, id: &EntityId) {
        let reply = self.wifi_reply();
        let id = id.clone();
        let Some(controller) = self.controller.clone() else {
            reply(Action::WifiBroadcastDetailFailed {
                id,
                error: "not connected".to_owned(),
            });
            return;
        };
        let sanitizer = self.sanitizer.clone();
        tokio::spawn(async move {
            reply(match controller.get_wifi_broadcast_detail(&id).await {
                Ok(detail) => Action::WifiBroadcastDetailLoaded {
                    id,
                    detail: Arc::new(
                        sanitizer
                            .as_ref()
                            .map_or_else(|| detail.clone(), |san| san.sanitize_wifi(&detail)),
                    ),
                },
                Err(error) => {
                    let error = error.to_string();
                    Action::WifiBroadcastDetailFailed {
                        id,
                        error: sanitizer
                            .as_ref()
                            .map_or_else(|| error.clone(), |san| san.sanitize_text(&error)),
                    }
                }
            });
        });
    }

    /// Whether a write to the SSID `id` is still running, said in a toast
    /// when one is.
    fn ssid_write_in_flight(&mut self, id: &EntityId, name: &str) -> bool {
        let busy = self.wifi.writes.contains(id);
        if busy {
            self.show_notification(Notification::warning(format!(
                "A change to {name} is still being applied. Try again when it is done."
            )));
        }
        busy
    }

    /// Read from the controller whether the SSID hides its name. The list
    /// does not say, so the toggle's direction comes from this read and
    /// never from what a screen last drew.
    pub(super) fn read_ssid_visibility(&mut self, id: &EntityId) {
        let Some(name) = self.shown_ssid_name(id) else {
            self.report_gone("That SSID");
            return;
        };
        if self.ssid_write_in_flight(id, &name) {
            return;
        }
        let Some(controller) = self.controller.clone() else {
            return;
        };
        let reply = self.wifi_reply();
        let id = id.clone();
        tokio::spawn(async move {
            reply(match controller.get_wifi_broadcast_detail(&id).await {
                Ok(detail) => Action::WifiSsidVisibilityRead {
                    id,
                    detail: Arc::new(detail),
                },
                Err(error) => {
                    warn!(%error, "an SSID could not be read before a visibility change");
                    Action::Notify(Notification::error(format!(
                        "Could not read {name} from the controller: {error}"
                    )))
                }
            });
        });
    }

    /// Ask before flipping the visibility that was just read.
    pub(super) fn confirm_ssid_visibility(
        &mut self,
        id: &EntityId,
        detail: &WifiBroadcast,
    ) -> Result<()> {
        let shown = self.shown_wifi(detail);
        let name = shown.name.clone();
        self.forward_to_screen(
            ScreenId::Wifi,
            &Action::WifiBroadcastDetailLoaded {
                id: id.clone(),
                detail: Arc::new(shown),
            },
        )?;
        // The answer to a key pressed on the WiFi screen: it opens nothing
        // once the user has moved on or is typing into something else.
        if self.active_screen != ScreenId::Wifi || self.prompt.is_some() {
            return Ok(());
        }
        let Some(hidden) = detail.hidden else {
            self.show_notification(Notification::error(format!(
                "The controller did not say whether {name} hides its name, so nothing was changed"
            )));
            return Ok(());
        };
        let (prompt, effect, notice) = if hidden {
            (
                format!("Announce {name} again?"),
                "Its name shows up in scans again.",
                Notice::new(
                    format!("Showing {name}\u{2026}"),
                    format!("{name} announces its name again"),
                ),
            )
        } else {
            (
                format!("Hide {name}?"),
                "After that only devices that already know its name can join.",
                Notice::new(
                    format!("Hiding {name}\u{2026}"),
                    format!("{name} is hidden: clients must know its name to join"),
                ),
            )
        };
        let write = Action::WifiSsidWrite {
            id: id.clone(),
            change: SsidChange::Update(Box::new(UpdateWifiBroadcastRequest {
                hide_ssid: Some(!hidden),
                ..Default::default()
            })),
            notice,
        };
        self.confirm(ConfirmRequest::new(prompt, write).detail(format!("{REPROVISION} {effect}")));
        Ok(())
    }

    /// Write `change` to the SSID `id`. The toast reports success only
    /// once the controller's own record shows the change.
    pub(super) fn write_ssid(&mut self, id: &EntityId, change: &SsidChange, notice: &Notice) {
        let Some(controller) = self.controller.clone() else {
            self.show_notification(Notification::error("Not connected"));
            return;
        };
        let name = self
            .shown_ssid_name(id)
            .unwrap_or_else(|| "the SSID".to_owned());
        if self.ssid_write_in_flight(id, &name) {
            return;
        }
        self.wifi.writes.insert(id.clone());

        let reply = self.wifi_reply();
        let sanitizer = self.sanitizer.clone();
        let (id, change, done) = (id.clone(), change.clone(), notice.done.clone());
        self.spawn_op(&notice.pending, async move {
            let (notification, read) = apply(&controller, &id, change, &name, done).await;
            reply(Action::WifiSsidWriteSettled(id.clone()));
            if let Some(detail) = read {
                let detail = sanitizer
                    .as_ref()
                    .map_or_else(|| detail.clone(), |san| san.sanitize_wifi(&detail));
                reply(Action::WifiBroadcastDetailLoaded {
                    id,
                    detail: Arc::new(detail),
                });
            }
            notification
        });
    }

    /// A write ended: the SSID takes the next one, and the WiFi screen
    /// reads it again.
    pub(super) fn ssid_write_settled(&mut self, id: &EntityId) -> Result<()> {
        self.wifi.writes.remove(id);
        self.forward_to_screen(ScreenId::Wifi, &Action::WifiSsidWriteSettled(id.clone()))
    }
}

/// Run one SSID write, refresh the controller's data, and compare what the
/// controller now reports with what was asked for. Returns the toast, and
/// the record read back when there is one.
async fn apply(
    controller: &Controller,
    id: &EntityId,
    change: SsidChange,
    name: &str,
    done: String,
) -> (Notification, Option<WifiBroadcast>) {
    let (command, update) = match change {
        SsidChange::Update(update) => (
            Command::UpdateWifiBroadcast {
                id: id.clone(),
                update: (*update).clone(),
            },
            Some(update),
        ),
        SsidChange::Delete => (
            Command::DeleteWifiBroadcast {
                id: id.clone(),
                force: false,
            },
            None,
        ),
    };
    if let Err(error) = controller.execute(command).await {
        warn!(%error, "an SSID write failed");
        return (Notification::error(format!("{error}")), None);
    }
    let refreshed = controller.full_refresh().await;
    if let Err(error) = &refreshed {
        warn!(%error, "refresh after an SSID write failed");
    }

    let Some(update) = update else {
        let listed = controller
            .wifi_broadcasts_snapshot()
            .iter()
            .any(|ssid| &ssid.id == id);
        let notification = match refreshed {
            Err(error) => Notification::warning(format!(
                "{done}, but the SSID list could not be read again to check: {error}"
            )),
            Ok(()) if listed => Notification::error(format!(
                "The controller accepted the delete but still lists {name}"
            )),
            Ok(()) => Notification::success(done),
        };
        return (notification, None);
    };
    match controller.get_wifi_broadcast_detail(id).await {
        Ok(detail) => {
            let notification = match unapplied(&update, &detail) {
                Some(still) => Notification::error(format!(
                    "The controller accepted the change to {name} but still reports {still}"
                )),
                None => Notification::success(done),
            };
            (notification, Some(detail))
        }
        Err(error) => (
            Notification::warning(format!(
                "{done}, but it could not be read back to check: {error}"
            )),
            None,
        ),
    }
}

/// What `detail` still says against `update`, when the change did not
/// take. A passphrase is not compared: the record's copy stays out of
/// the model.
fn unapplied(update: &UpdateWifiBroadcastRequest, detail: &WifiBroadcast) -> Option<String> {
    if let Some(enabled) = update.enabled
        && detail.enabled != enabled
    {
        return Some(format!("it {}", if detail.enabled { "on" } else { "off" }));
    }
    if let Some(hidden) = update.hide_ssid
        && detail.hidden != Some(hidden)
    {
        return Some(
            match detail.hidden {
                Some(true) => "its name hidden",
                Some(false) => "its name announced",
                None => "nothing about its visibility",
            }
            .to_owned(),
        );
    }
    if let Some(name) = update.name.as_ref().or(update.ssid.as_ref())
        && &detail.name != name
    {
        return Some("its old name".to_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crossterm::event::{KeyCode, KeyEvent};
    use unifly_api::DemoFleetOptions;

    use super::*;
    use crate::tui::action::{NotificationLevel, WifiSubTab};

    /// The demo fleet on the WiFi screen's SSIDs tab, with the list the
    /// controller gave: summaries only.
    async fn ssids_app() -> (App, Controller) {
        let controller = Controller::demo(DemoFleetOptions::default());
        controller.connect().await.expect("demo");
        let mut app = App::new(Some(controller.clone()), None, false, true);
        app.active_screen = ScreenId::Wifi;
        app.process_action(&Action::WifiBroadcastsUpdated(
            controller.wifi_broadcasts_snapshot(),
        ))
        .expect("SSIDs reach the screen");
        app.process_action(&Action::WifiSubTab(WifiSubTab::Ssids))
            .expect("sub-tab");
        (app, controller)
    }

    fn drain(app: &mut App) {
        while let Ok(queued) = app.action_rx.try_recv() {
            app.process_action(&queued).expect("queued action");
        }
    }

    /// Handle what comes in until `stop` has been handled, then whatever
    /// that queued.
    async fn handle_until(app: &mut App, stop: fn(&Action) -> bool) {
        loop {
            let action = tokio::time::timeout(Duration::from_secs(10), app.action_rx.recv())
                .await
                .expect("the controller work reports back")
                .expect("the action channel stays open");
            let last = stop(&action);
            app.process_action(&action).expect("action");
            if last {
                break;
            }
        }
        drain(app);
    }

    async fn finish_write(app: &mut App) {
        handle_until(app, |action| matches!(action, Action::OpFinished { .. })).await;
    }

    fn press(app: &mut App, key: char) {
        let pressed = app
            .handle_key_event(KeyEvent::from(KeyCode::Char(key)))
            .expect("key")
            .expect("the key does something");
        app.process_action(&pressed).expect("key action");
        drain(app);
    }

    fn hide(hidden: bool) -> SsidChange {
        SsidChange::Update(Box::new(UpdateWifiBroadcastRequest {
            hide_ssid: Some(hidden),
            ..Default::default()
        }))
    }

    #[tokio::test]
    async fn an_ssid_key_runs_through_the_app_and_changes_the_controller() {
        let (mut app, controller) = ssids_app().await;

        // The first SSID in name order is enabled, so `E` asks to disable.
        press(&mut app, 'E');
        assert!(app.pending_confirm.is_some(), "E asks first");
        press(&mut app, 'y');
        finish_write(&mut app).await;

        let disabled: Vec<_> = controller
            .wifi_broadcasts_snapshot()
            .iter()
            .filter(|ssid| !ssid.enabled)
            .map(|ssid| ssid.name.clone())
            .collect();
        assert_eq!(disabled.len(), 1, "{disabled:?}");
        let toast = app.notices.latest().expect("a toast reports the result");
        assert_eq!(toast.message, format!("Disabled {}", disabled[0]));
        assert!(app.wifi.writes.is_empty(), "the SSID takes the next write");
        controller.disconnect().await;
    }

    #[tokio::test]
    async fn hide_reads_the_controller_and_flips_what_it_read_in_both_directions() {
        let (mut app, controller) = ssids_app().await;
        let listed = controller.wifi_broadcasts_snapshot();
        assert!(
            listed.iter().all(|ssid| ssid.hidden.is_none()),
            "the list does not say which SSIDs hide their name"
        );
        let mut crew = None;
        for ssid in listed.iter() {
            let detail = controller
                .get_wifi_broadcast_detail(&ssid.id)
                .await
                .expect("detail");
            if detail.hidden == Some(true) {
                crew = Some(detail);
            }
        }
        let crew = crew.expect("the fleet has one hidden SSID");
        let name = &crew.name;

        for (asks, hidden_after, says) in [
            (
                format!("Announce {name} again?"),
                false,
                "announces its name again",
            ),
            (format!("Hide {name}?"), true, "is hidden"),
        ] {
            app.process_action(&Action::RequestSsidVisibilityToggle(crew.id.clone()))
                .expect("H");
            assert!(
                app.pending_confirm.is_none(),
                "nothing is asked before the read"
            );
            handle_until(&mut app, |action| {
                matches!(action, Action::WifiAnswer { .. })
            })
            .await;
            let confirm = app.pending_confirm.clone().expect("a confirmation");
            assert_eq!(confirm.prompt, asks);
            assert!(
                confirm
                    .detail
                    .as_deref()
                    .is_some_and(|cost| cost.contains("reprovision")),
                "{:?}",
                confirm.detail
            );
            assert_eq!(
                controller
                    .get_wifi_broadcast_detail(&crew.id)
                    .await
                    .expect("detail")
                    .hidden,
                Some(!hidden_after),
                "nothing is written before the answer"
            );

            press(&mut app, 'y');
            finish_write(&mut app).await;
            assert_eq!(
                controller
                    .get_wifi_broadcast_detail(&crew.id)
                    .await
                    .expect("detail")
                    .hidden,
                Some(hidden_after)
            );
            let toast = app.notices.latest().expect("a toast");
            assert_eq!(toast.level, NotificationLevel::Success);
            assert!(toast.message.contains(says), "{}", toast.message);
        }
        controller.disconnect().await;
    }

    #[tokio::test]
    async fn a_second_write_to_an_ssid_waits_for_the_first() {
        let (mut app, controller) = ssids_app().await;
        let ssid = controller.wifi_broadcasts_snapshot()[0].clone();
        let write = |hidden| Action::WifiSsidWrite {
            id: ssid.id.clone(),
            change: hide(hidden),
            notice: Notice::new("Changing\u{2026}", "Changed"),
        };

        app.process_action(&write(true)).expect("first write");
        app.process_action(&write(false)).expect("second write");
        let refused = app.notices.latest().expect("the second is refused");
        assert_eq!(refused.level, NotificationLevel::Warning);
        assert!(
            refused.message.contains("still being applied"),
            "{}",
            refused.message
        );
        // Reading for a toggle waits too: its answer would be stale.
        app.process_action(&Action::RequestSsidVisibilityToggle(ssid.id.clone()))
            .expect("H during a write");

        finish_write(&mut app).await;
        assert!(
            app.pending_confirm.is_none(),
            "no toggle was read meanwhile"
        );
        assert_eq!(
            controller
                .get_wifi_broadcast_detail(&ssid.id)
                .await
                .expect("detail")
                .hidden,
            Some(true),
            "only the first write ran"
        );

        app.process_action(&write(false)).expect("a later write");
        finish_write(&mut app).await;
        assert_eq!(
            controller
                .get_wifi_broadcast_detail(&ssid.id)
                .await
                .expect("detail")
                .hidden,
            Some(false)
        );
        controller.disconnect().await;
    }

    #[tokio::test]
    async fn a_controller_switch_forgets_the_writes_of_the_old_one() {
        let (mut app, controller) = ssids_app().await;
        let ssid = controller.wifi_broadcasts_snapshot()[0].clone();
        app.process_action(&Action::WifiSsidWrite {
            id: ssid.id.clone(),
            change: hide(true),
            notice: Notice::new("Changing\u{2026}", "Changed"),
        })
        .expect("write");
        assert!(app.wifi.writes.contains(&ssid.id));

        app.process_action(&Action::ControllerChanged)
            .expect("switch");
        assert!(app.wifi.writes.is_empty());
        finish_write(&mut app).await;
        controller.disconnect().await;
    }

    /// A controller that answers every write with 200 and goes on
    /// reporting the SSID as it was.
    #[tokio::test]
    async fn a_write_the_controller_did_not_apply_is_reported_as_a_failure() {
        use wiremock::matchers::{body_partial_json, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        const SITE: &str = "550e8400-e29b-41d4-a716-446655440000";
        const SSID: &str = "7a1d0c7e-52c7-4b7e-9d0a-2f3b9d6a4c11";
        let server = MockServer::start().await;
        let base = "/v1/connector/consoles/console-1/proxy/network/integration/v1";
        let unchanged = serde_json::json!({
            "type": "STANDARD", "id": SSID, "name": "Studio", "enabled": true,
            "metadata": {"origin": "USER_DEFINED"}, "hideName": false,
            "securityConfiguration": {"type": "OPEN"},
            "broadcastingFrequenciesGHz": [2.4, 5]
        });
        Mock::given(method("GET"))
            .and(path(format!("{base}/sites")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "offset": 0, "limit": 50, "count": 1, "totalCount": 1,
                "data": [{ "id": SITE, "internalReference": "default", "name": "Default" }],
            })))
            .mount(&server)
            .await;
        let record = format!("{base}/sites/{SITE}/wifi/broadcasts/{SSID}");
        Mock::given(method("GET"))
            .and(path(&record))
            .respond_with(ResponseTemplate::new(200).set_body_json(unchanged.clone()))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path(&record))
            .and(body_partial_json(serde_json::json!({"hideName": true})))
            .respond_with(ResponseTemplate::new(200).set_body_json(unchanged))
            .expect(1)
            .mount(&server)
            .await;

        let controller = Controller::new(unifly_api::ControllerConfig {
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
        app.active_screen = ScreenId::Wifi;

        app.process_action(&Action::WifiSsidWrite {
            id: EntityId::from(SSID),
            change: hide(true),
            notice: Notice::new("Hiding Studio\u{2026}", "Studio is hidden"),
        })
        .expect("write");
        finish_write(&mut app).await;

        let toast = app.notices.latest().expect("a toast");
        assert_eq!(toast.level, NotificationLevel::Error, "{}", toast.message);
        assert!(
            toast.message.contains("still reports its name announced"),
            "{}",
            toast.message
        );
        server.verify().await;
    }

    #[test]
    fn what_did_not_take_is_named_and_a_change_that_took_is_not() {
        let detail: WifiBroadcast = serde_json::from_value(serde_json::json!({
            "id": uuid::Uuid::nil().to_string(), "name": "Studio", "enabled": true,
            "broadcast_type": "Standard", "security": "Open", "network_id": null,
            "frequencies_ghz": [], "hidden": false, "hotspot_enabled": false, "origin": null
        }))
        .expect("SSID fixture should deserialize");
        let update = |json: serde_json::Value| -> UpdateWifiBroadcastRequest {
            serde_json::from_value(json).expect("update")
        };
        for (asked, still) in [
            (serde_json::json!({"enabled": false}), Some("it on")),
            (serde_json::json!({"enabled": true}), None),
            (
                serde_json::json!({"hide_ssid": true}),
                Some("its name announced"),
            ),
            (serde_json::json!({"hide_ssid": false}), None),
            (serde_json::json!({"name": "Loft"}), Some("its old name")),
            (serde_json::json!({"name": "Studio"}), None),
            (serde_json::json!({"passphrase": "not compared"}), None),
        ] {
            assert_eq!(
                unapplied(&update(asked.clone()), &detail).as_deref(),
                still,
                "{asked}"
            );
        }
    }
}
