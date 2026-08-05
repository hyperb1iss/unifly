//! Traffic matching list command handlers.

use std::sync::Arc;

use tabled::Tabled;
use unifly_api::{Command as CoreCommand, Controller, EntityId, TrafficMatchingList};

use crate::cli::args::{GlobalOpts, TrafficListType, TrafficListsArgs, TrafficListsCommand};
use crate::cli::error::CliError;
use crate::cli::output;

use super::util;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct TrafficListRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Type")]
    list_type: String,
    #[tabled(rename = "Items")]
    item_count: String,
}

fn traffic_list_row(t: &Arc<TrafficMatchingList>, p: &output::Painter) -> TrafficListRow {
    TrafficListRow {
        id: p.id(&t.id.to_string()),
        name: p.name(&t.name),
        list_type: p.muted(&t.list_type),
        item_count: p.number(&t.items.len().to_string()),
    }
}

fn detail(t: &Arc<TrafficMatchingList>) -> String {
    let mut lines = vec![
        format!("ID:    {}", t.id),
        format!("Name:  {}", t.name),
        format!("Type:  {}", t.list_type),
        format!("Items: {}", t.items.len()),
    ];
    if let Some(ref origin) = t.origin {
        lines.push(format!("Origin: {origin:?}"));
    }
    if !t.items.is_empty() {
        lines.push(String::new());
        lines.push("Entries:".into());
        for item in &t.items {
            lines.push(format!("  - {item}"));
        }
    }
    lines.join("\n")
}

// ── Handler ─────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
pub async fn handle(
    controller: &Controller,
    args: TrafficListsArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    util::ensure_integration_access(controller, "traffic-lists").await?;

    let p = output::Painter::new(global);

    match args.command {
        TrafficListsCommand::List(list) => {
            let all = controller.traffic_matching_lists_snapshot();
            let snap = util::apply_list_args(all.iter().cloned(), &list, |t, filter| {
                util::matches_json_filter(t, filter)
            });
            let out = output::render_list(
                &global.output,
                &snap,
                |t| traffic_list_row(t, &p),
                |t| t.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        TrafficListsCommand::Get { id } => {
            let snap = controller.traffic_matching_lists_snapshot();
            let found = snap.iter().find(|t| t.id.to_string() == id);
            match found {
                Some(t) => {
                    let out =
                        output::render_single(&global.output, t, detail, |t| t.id.to_string());
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "traffic matching list".into(),
                        identifier: id,
                        list_command: "traffic-lists list".into(),
                    });
                }
            }
            Ok(())
        }

        TrafficListsCommand::Create {
            from_file,
            name,
            list_type,
            items,
        } => {
            let req = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                unifly_api::command::CreateTrafficMatchingListRequest {
                    name: name.unwrap_or_default(),
                    list_type: list_type.map_or_else(
                        || "IPV4".into(),
                        |t| match t {
                            TrafficListType::Ports => "PORT".into(),
                            TrafficListType::Ipv4 => "IPV4".into(),
                            TrafficListType::Ipv6 => "IPV6".into(),
                        },
                    ),
                    entries: items.unwrap_or_default(),
                    raw_items: None,
                    description: None,
                }
            };
            let result = controller
                .execute(CoreCommand::CreateTrafficMatchingList(req))
                .await?;
            if let unifly_api::CommandResult::TrafficMatchingList(list) = result {
                let list = Arc::new(list);
                let out =
                    output::render_single(&global.output, &list, detail, |t| t.id.to_string());
                output::print_output(&out, global.quiet);
            } else {
                output::warn_unrenderable_create(global.quiet);
            }
            if !global.quiet {
                eprintln!("Traffic matching list created");
            }
            Ok(())
        }

        TrafficListsCommand::Update { id, from_file } => {
            if from_file.is_none() {
                return Err(CliError::Validation {
                    field: "update".into(),
                    reason: "traffic list updates currently require --from-file".into(),
                });
            }
            let update = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                unifly_api::command::UpdateTrafficMatchingListRequest::default()
            };
            let eid = EntityId::from(id);
            controller
                .execute(CoreCommand::UpdateTrafficMatchingList { id: eid, update })
                .await?;
            if !global.quiet {
                eprintln!("Traffic matching list updated");
            }
            Ok(())
        }

        TrafficListsCommand::Delete { id } => {
            let eid = EntityId::from(id.clone());
            if !util::confirm(&format!("Delete traffic matching list {id}?"), global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::DeleteTrafficMatchingList { id: eid })
                .await?;
            if !global.quiet {
                eprintln!("Traffic matching list deleted");
            }
            Ok(())
        }
    }
}
