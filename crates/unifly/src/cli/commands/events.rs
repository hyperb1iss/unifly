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
            let snap = controller.events_snapshot();
            let cutoff = Utc::now() - chrono::TimeDelta::hours(i64::from(within));
            let filtered: Vec<_> = snap
                .iter()
                .filter(|e| e.timestamp >= cutoff)
                .take(usize::try_from(limit).unwrap_or(usize::MAX))
                .cloned()
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

                        let line = match format {
                            OutputFormat::Json | OutputFormat::JsonCompact => {
                                serde_json::to_string(&*event)
                                    .unwrap_or_else(|_| format!("{event:?}"))
                            }
                            OutputFormat::Yaml => {
                                serde_yaml_ng::to_string(&*event)
                                    .unwrap_or_else(|_| format!("{event:?}"))
                            }
                            _ => {
                                let time = event.timestamp.format("%H:%M:%S");
                                let cat = format!("{:?}", event.category);
                                format!("{time}  [{cat}]  {}", event.message)
                            }
                        };

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
