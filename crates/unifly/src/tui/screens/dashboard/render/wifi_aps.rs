use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use unifly_api::{Device, DeviceType};

use crate::tui::action::Action;
use crate::tui::screens::events::render::clip;
use crate::tui::theme;
use crate::tui::widgets::click::{self, Click};

use super::super::DashboardScreen;
use super::super::focus::Panel;
use super::muted_line;

/// Widest an access point's name is drawn.
const NAME_MAX: usize = 16;

/// How a column of channels is written.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ChannelForm {
    /// Band and channel: `2G:1 5G:36`.
    Labelled,
    /// Channels alone, low band first: `1/36`.
    Bare,
}

/// An access point's channels in `form`.
fn channels(ap: &Device, form: ChannelForm) -> String {
    let radios = ap.radios.iter().map(|radio| {
        let band = if radio.frequency_ghz >= 5.9 {
            "6G"
        } else if radio.frequency_ghz >= 4.9 {
            "5G"
        } else {
            "2G"
        };
        let channel = radio
            .channel
            .map_or_else(|| "─".into(), |channel| channel.to_string());
        (band, channel)
    });
    match form {
        ChannelForm::Labelled => radios
            .map(|(band, channel)| format!("{band}:{channel}"))
            .collect::<Vec<_>>()
            .join(" "),
        ChannelForm::Bare => radios
            .map(|(_, channel)| channel)
            .collect::<Vec<_>>()
            .join("/"),
    }
}

/// The fullest form in which every one of `aps` fits `width` whole, so
/// the column reads one way and no value is cut partway. `None` when not
/// even the bare channels fit.
fn channel_form(aps: &[&std::sync::Arc<Device>], width: usize) -> Option<ChannelForm> {
    [ChannelForm::Labelled, ChannelForm::Bare]
        .into_iter()
        .find(|form| aps.iter().all(|ap| channels(ap, *form).len() <= width))
}

impl DashboardScreen {
    /// Mean WiFi experience of the clients on `ap`, when any report one.
    fn experience(&self, ap: &Device) -> Option<u32> {
        let scores: Vec<u32> = self
            .clients
            .iter()
            .filter(|c| {
                c.uplink_device_mac.as_ref() == Some(&ap.mac)
                    || c.wireless
                        .as_ref()
                        .and_then(|wl| wl.bssid.as_ref())
                        .is_some_and(|bssid| *bssid == ap.mac)
            })
            .filter_map(|c| c.wireless.as_ref()?.satisfaction)
            .map(u32::from)
            .collect();
        let count = u32::try_from(scores.len())
            .ok()
            .filter(|count| *count > 0)?;
        Some(scores.iter().sum::<u32>() / count)
    }

    /// WiFi / APs panel - tabular layout with header, aligned columns.
    pub(super) fn render_wifi_aps(&self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        let block = self.panel_block(Panel::Wifi, "WiFi / APs");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let mut aps: Vec<_> = self
            .devices
            .iter()
            .filter(|d| d.device_type == DeviceType::AccessPoint)
            .collect();
        if aps.is_empty() {
            let line = muted_line(self.empty_text("No access points"));
            frame.render_widget(Paragraph::new(line), inner);
            return;
        }
        aps.sort_by_key(|ap| std::cmp::Reverse(ap.client_count.unwrap_or(0)));

        let max_rows = usize::from(inner.height);
        let w = usize::from(inner.width);
        let (cli_col, exp_col) = (4_usize, 5_usize);
        // The name column is as wide as the longest name, so the channel
        // column gets every cell the names do not need.
        let longest = aps
            .iter()
            .map(|ap| Span::raw(ap.name.as_deref().unwrap_or("AP")).width())
            .max()
            .unwrap_or(2);
        let remaining = w.saturating_sub(1 + cli_col + exp_col);
        let name_width = longest.clamp(2, NAME_MAX).min(remaining.max(2));
        let chan_width = remaining.saturating_sub(name_width + 1);
        let shown = &aps[..aps.len().min(max_rows.saturating_sub(1))];
        let form = (aps.iter().any(|ap| !ap.radios.is_empty()))
            .then(|| channel_form(shown, chan_width))
            .flatten();

        let header = Style::default()
            .fg(theme::border_unfocused())
            .add_modifier(Modifier::BOLD);
        let mut hdr = vec![
            Span::styled(format!(" {:<name_width$}", "AP"), header),
            Span::styled(format!("{:>cli_col$}", "Cli"), header),
            Span::styled(format!("{:>exp_col$}", "Exp"), header),
        ];
        if form.is_some() {
            hdr.push(Span::styled(" Chan", header));
        }
        let mut lines = vec![Line::from(hdr)];

        for ap in shown {
            let ap_name = clip(ap.name.as_deref().unwrap_or("AP"), name_width);
            let cli = ap.client_count.unwrap_or(0);
            let mut spans = vec![
                Span::styled(
                    format!(" {ap_name:<name_width$}"),
                    Style::default()
                        .fg(theme::accent_secondary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{cli:>cli_col$}"),
                    Style::default().fg(theme::warning()),
                ),
            ];
            match self.experience(ap) {
                Some(exp) => {
                    let color = if exp >= 80 {
                        theme::success()
                    } else if exp >= 50 {
                        theme::warning()
                    } else {
                        theme::error()
                    };
                    spans.push(Span::styled(
                        format!("{exp:>4}%"),
                        Style::default().fg(color),
                    ));
                }
                None => spans.push(Span::styled(
                    format!("{:>exp_col$}", "─"),
                    Style::default().fg(theme::border_unfocused()),
                )),
            }
            if let Some(form) = form {
                spans.push(Span::styled(
                    format!(" {}", channels(ap, form)),
                    Style::default().fg(theme::border_unfocused()),
                ));
            }

            lines.push(Line::from(spans));
            self.hits.borrow_mut().push(
                click::paragraph_row(inner, lines.len() - 1),
                Click::Action(Action::ShowDevice(ap.id.clone())),
            );
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }
}
