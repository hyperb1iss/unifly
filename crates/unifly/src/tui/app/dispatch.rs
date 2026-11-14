use std::process::{Command, Stdio};
use std::time::Duration;

use color_eyre::eyre::Result;

use super::{App, ConnectionStatus};
use crate::tui::action::{Action, Notification};
use crate::tui::screen::ScreenId;

impl App {
    /// Process a single action — update app state and propagate to components.
    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    pub(super) fn process_action(&mut self, action: &Action) -> Result<()> {
        let invalidates = action_invalidates(action);
        if self.handle_command_action(action)? || self.handle_networks_firewall_action(action)? {
            if invalidates {
                self.needs_redraw = true;
            }
            return Ok(());
        }

        match action {
            Action::Quit => {
                self.running = false;
            }
            Action::Resize(w, h) => {
                self.terminal_size = (*w, *h);
            }
            Action::SwitchScreen(target) => self.switch_screen(*target)?,
            Action::PressKey(key) => {
                if let Some(next) = self.handle_key_event(*key)? {
                    self.action_tx.send(next)?;
                }
            }
            Action::GoBack => {
                // Going back does not arm another "back", or Esc would
                // bounce between the same two screens.
                if let Some(prev) = self.previous_screen.take() {
                    self.switch_screen(prev)?;
                    self.previous_screen = None;
                }
            }
            Action::OpenPrompt(request) => {
                let input =
                    crate::tui::widgets::line_input::LineInput::new(request.initial.clone());
                self.prompt = Some((request.clone(), input));
            }
            Action::Repaint => {
                self.repaint = true;
            }
            Action::Refresh => self.refresh_now(),
            Action::ShowDevice(_) => self.show_entity(ScreenId::Devices, action)?,
            Action::ShowClient(_) => self.show_entity(ScreenId::Clients, action)?,
            Action::ToggleHelp => {
                self.help_visible = !self.help_visible;
            }
            Action::ToggleNotices => {
                self.notices_visible = !self.notices_visible;
            }
            Action::ToggleAbout => {
                self.about_visible = !self.about_visible;
            }
            Action::OpenSearch => self.open_search(),
            Action::SearchSubmit => {
                // The screen keeps filtering by the query; only the bar
                // closes, and reopening it picks the query back up.
                self.search_active = false;
            }
            Action::CloseSearch => {
                self.search_active = false;
                self.search_input.clear();
                self.forward_to_screen(self.active_screen, action)?;
            }
            Action::Connecting => {
                self.connection_status = ConnectionStatus::Connecting;
            }
            Action::Connected => {
                self.connection_status = ConnectionStatus::Connected;
                self.connection_error = None;
            }
            Action::Disconnected(reason) => {
                self.connection_status = ConnectionStatus::Disconnected;
                // The status bar draws the reason, so it is sanitized too.
                let reason = &self.shown_text(reason);
                // Auto-reconnect resends this action after every failed
                // attempt; only toast when the failure reason is news.
                if self.connection_error.as_ref() != Some(reason) {
                    self.show_notification(Notification::error(format!(
                        "connection failed: {reason}"
                    )));
                }
                self.connection_error = Some(reason.clone());
            }
            Action::Reconnecting { attempt } => {
                self.connection_status = ConnectionStatus::Reconnecting { attempt: *attempt };
            }
            Action::RetryConnect => {
                self.retry_connect();
            }
            Action::Invalidate => {
                self.needs_redraw = true;
            }
            Action::ChartPeak => {
                if self.effects_enabled {
                    self.effects.start_chart_pulse();
                }
                self.needs_redraw = true;
            }
            Action::Render => {}
            Action::Tick => {
                self.forward_to_all_screens(action)?;

                if self.notices.expire() {
                    self.needs_redraw = true;
                }

                if self.active_screen == ScreenId::Stats
                    && self.connection_status == ConnectionStatus::Connected
                    && let Some(last) = self.last_stats_fetch
                    && last.elapsed() > Duration::from_mins(1)
                {
                    let _ = self.action_tx.send(Action::RequestStats(self.stats_period));
                }
            }
            Action::DevicesUpdated(_)
            | Action::ClientsUpdated(_)
            | Action::NetworksUpdated(_)
            | Action::FirewallPoliciesUpdated(_)
            | Action::FirewallZonesUpdated(_)
            | Action::AclRulesUpdated(_)
            | Action::NatPoliciesUpdated(_)
            | Action::WifiBroadcastsUpdated(_)
            | Action::EventReceived(_)
            | Action::HealthUpdated(_)
            | Action::SiteUpdated(_)
            | Action::WifiNeighborsUpdated(_)
            | Action::WifiChannelsUpdated(_)
            | Action::WifiClientDetailLoaded { .. }
            | Action::WifiClientDetailFailed { .. }
            | Action::WifiRoamHistoryLoaded { .. }
            | Action::WifiRoamHistoryFailed { .. }
            | Action::StatsUpdated(_)
            | Action::ControllerChanged
            | Action::NetworkEditResult(_) => {
                self.forward_to_all_screens(action)?;
            }
            Action::AccessUpdated(access) => {
                self.access = *access;
                self.forward_to_all_screens(action)?;
            }
            Action::RequestWifiNeighbors(within_secs) => {
                self.fetch_wifi_neighbors(*within_secs);
            }
            Action::RequestWifiChannels => {
                self.fetch_wifi_channels();
            }
            Action::RequestWifiClientDetail(ip) => {
                self.fetch_wifi_client_detail(ip);
            }
            Action::RequestWifiRoamHistory { mac, limit } => {
                self.fetch_wifi_roam_history(mac, *limit);
            }
            Action::RequestStats(period) => {
                self.stats_period = *period;
                self.last_stats_fetch = Some(std::time::Instant::now());
                self.fetch_stats(*period);
            }
            Action::OnboardingComplete { config, .. } => {
                self.handle_onboarding_complete(config)?;
            }
            Action::OnboardingTestResult(_) => {
                self.forward_to_screen(ScreenId::Setup, action)?;
            }
            Action::OpenSettings => {
                self.open_settings()?;
            }
            Action::CloseSettings => {
                self.close_settings();
            }
            Action::SettingsTestResult(_) => {
                self.forward_to_screen(ScreenId::Settings, action)?;
            }
            Action::SettingsApply { config, .. } => {
                self.apply_settings(config)?;
            }
            Action::Notify(notification) => {
                self.show_notification(notification.clone());
            }
            Action::DismissNotification => {
                self.notices.dismiss_sticky();
            }
            Action::DismissToast(id) => {
                self.notices.dismiss(*id);
            }
            Action::OpenDonate => {
                open_url("https://github.com/sponsors/hyperb1iss");
            }
            Action::SetShowDonate(show) => {
                self.show_donate = *show;
            }
            other => {
                self.forward_to_screen(self.active_screen, other)?;
            }
        }

        if invalidates {
            self.needs_redraw = true;
        }

        Ok(())
    }
}

impl App {
    /// Fetch the controller's data now. While disconnected this retries
    /// the connection instead.
    fn refresh_now(&mut self) {
        if self.connection_status == ConnectionStatus::Disconnected {
            self.retry_connect();
            return;
        }
        let Some(controller) = self.controller.clone() else {
            return;
        };
        self.spawn_op("Refreshing\u{2026}", async move {
            match controller.full_refresh().await {
                Ok(()) => Notification::success("Data refreshed"),
                Err(error) => Notification::error(format!("Refresh failed: {error}")),
            }
        });
        if self.active_screen == ScreenId::Stats {
            let _ = self.action_tx.send(Action::RequestStats(self.stats_period));
        }
    }

    /// Switch to `screen` and hand it the entity to open.
    fn show_entity(&mut self, screen: ScreenId, action: &Action) -> Result<()> {
        self.switch_screen(screen)?;
        self.forward_to_screen(screen, action)
    }
}

fn action_invalidates(action: &Action) -> bool {
    !matches!(
        action,
        Action::Render
            | Action::Tick
            | Action::RequestWifiNeighbors(_)
            | Action::RequestWifiChannels
            | Action::RequestWifiClientDetail(_)
            | Action::RequestWifiRoamHistory { .. }
            | Action::RequestStats(_)
    )
}

/// Open a URL in the user's default browser.
///
/// The opener runs detached from the terminal. The TUI owns the screen, so
/// anything the opener or the browser it starts prints would land on top of
/// it, and an inherited stdin could swallow keystrokes.
pub(super) fn open_url(url: &str) {
    let mut command = opener_command(url);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match command.spawn() {
        Ok(mut child) => {
            // Reap the opener so it does not linger as a zombie, and keep a
            // trace of a failure since its stderr is gone.
            let _ = std::thread::Builder::new()
                .name("unifly-open-url".into())
                .spawn(move || match child.wait() {
                    Ok(status) if !status.success() => {
                        tracing::debug!("browser opener exited with {status}");
                    }
                    Ok(_) => {}
                    Err(error) => tracing::debug!("could not wait for the browser opener: {error}"),
                });
        }
        Err(error) => tracing::debug!("could not open {url}: {error}"),
    }
}

fn opener_command(url: &str) -> Command {
    #[cfg(target_os = "macos")]
    let command = {
        let mut command = Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(target_os = "windows")]
    let command = {
        let mut command = Command::new("cmd");
        command.args(["/c", "start", url]);
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    command
}

#[cfg(test)]
mod tests {
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[test]
    fn opener_hands_the_url_to_xdg_open() {
        let command = super::opener_command("https://github.com/sponsors/hyperb1iss");

        assert_eq!(command.get_program(), "xdg-open");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["https://github.com/sponsors/hyperb1iss"]
        );
    }
}
