//! Event command handlers.

use std::io::{self, Write};
use std::sync::Arc;

use chrono::Utc;
use tabled::Tabled;
use unifly_api::{Controller, Event};

use crate::cli::args::{EventsArgs, EventsCommand, GlobalOpts, OutputFormat};
use crate::cli::error::CliError;
use crate::cli::output;

use super::util;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct EventRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Time")]
    time: String,
    #[tabled(rename = "Category")]
    category: String,
    #[tabled(rename = "Message")]
    message: String,
}

fn event_row(e: &Arc<Event>, p: &output::Painter) -> EventRow {
    EventRow {
        id: p.id(&e.id.as_ref().map(ToString::to_string).unwrap_or_default()),
        time: p.muted(&e.timestamp.format("%Y-%m-%d %H:%M:%S").to_string()),
        category: p.muted(&format!("{:?}", e.category)),
        message: e.message.clone(),
    }
}

// ── Handler ─────────────────────────────────────────────────────────

#[allow(clippy::future_not_send)]
pub async fn handle(
    controller: &Controller,
    args: EventsArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    let p = output::Painter::new(global);

    match args.command {
        EventsCommand::List { limit, within } => {
            util::ensure_session_access(controller, "events list").await?;
            let snap = util::order::events(&controller.events_snapshot());
            let cutoff = Utc::now() - chrono::TimeDelta::hours(i64::from(within));
            let filtered: Vec<_> = snap
                .into_iter()
                .filter(|e| e.timestamp >= cutoff)
                .take(usize::try_from(limit).unwrap_or(usize::MAX))
                .collect();
            let out = output::render_list(
                &global.output,
                &filtered,
                |e| event_row(e, &p),
                |e| e.id.as_ref().map(ToString::to_string).unwrap_or_default(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        EventsCommand::Watch { types } => {
            ensure_live_event_access(controller, "events watch").await?;
            watch_events(controller, &global.output, types.as_deref()).await
        }
    }
}

async fn ensure_live_event_access(
    controller: &Controller,
    operation: &str,
) -> Result<(), CliError> {
    if controller.has_live_event_access().await {
        return Ok(());
    }

    Err(CliError::Unsupported {
        operation: operation.into(),
        required: "session authentication (use password or hybrid mode)".into(),
    })
}

/// Stream live events from the controller's WebSocket broadcast channel.
///
/// Prints each event as it arrives; Ctrl+C terminates cleanly.
#[allow(clippy::future_not_send)]
async fn watch_events(
    controller: &Controller,
    format: &OutputFormat,
    type_filter: Option<&[String]>,
) -> Result<(), CliError> {
    let mut rx = controller.events();
    let mut stdout = io::stdout().lock();

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        // Apply type filter if specified
                        if let Some(types) = type_filter {
                            let cat = format!("{:?}", event.category);
                            if !types.iter().any(|t| cat.eq_ignore_ascii_case(t)) {
                                continue;
                            }
                        }

                        let line = watch_line(&event, format);

                        if writeln!(stdout, "{line}").is_err() {
                            break; // Broken pipe
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        eprintln!("warning: skipped {n} events (too slow)");
                    }
                }
            }
        }
    }

    Ok(())
}

/// One streamed event in the selected format, without a trailing newline.
fn watch_line(event: &Event, format: &OutputFormat) -> String {
    match format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            serde_json::to_string(event).unwrap_or_else(|_| format!("{event:?}"))
        }
        // Each event is its own document, so the stream stays valid YAML.
        OutputFormat::Yaml => {
            let yaml = output::render_yaml(event);
            // Drop only the final newline: earlier ones can belong to a
            // block scalar that keeps its trailing line breaks.
            format!("---\n{}", yaml.strip_suffix('\n').unwrap_or(&yaml))
        }
        _ => {
            let time = event.timestamp.format("%H:%M:%S");
            let cat = format!("{:?}", event.category);
            format!("{time}  [{cat}]  {}", event.message)
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use unifly_api::Event;

    use super::watch_line;
    use crate::cli::args::OutputFormat;

    fn event(message: &str) -> Event {
        serde_json::from_value(json!({
            "id": null,
            "timestamp": "2026-03-18T15:30:00Z",
            "category": "Client",
            "severity": "Info",
            "event_type": "EVT_WU_Connected",
            "message": message,
            "device_mac": "12:34:56:01:02:03",
            "client_mac": null,
            "site_id": null,
            "raw_key": null,
        }))
        .expect("event fixture")
    }

    #[test]
    fn yaml_watch_stream_is_one_document_per_event() {
        let mut last = event("trailing block");
        last.raw_key = Some("kept\n\n".into());
        let events = [event("first: joined"), event("second's turn"), last];
        let mut stream = String::new();
        for event in &events {
            stream.push_str(&watch_line(event, &OutputFormat::Yaml));
            stream.push('\n');
        }

        let documents: Vec<serde_json::Value> = serde_yaml_ng::Deserializer::from_str(&stream)
            .map(|document| {
                serde::Deserialize::deserialize(document)
                    .unwrap_or_else(|err| panic!("invalid YAML stream ({err}):\n{stream}"))
            })
            .collect();
        let expected: Vec<serde_json::Value> = events
            .iter()
            .map(|event| serde_json::to_value(event).expect("event serializes"))
            .collect();
        assert_eq!(documents, expected, "{stream}");
    }
}
