use std::sync::Arc;

use tabled::Tabled;
use unifly_api::model::{FirewallGroup, FirewallGroupType};
use unifly_api::{
    Command as CoreCommand, Controller, CreateFirewallGroupRequest, EntityId,
    UpdateFirewallGroupRequest,
};

use crate::cli::args::{FirewallGroupTypeArg, FirewallGroupsCommand, GlobalOpts};
use crate::cli::error::CliError;
use crate::cli::output;

use super::util;

#[derive(Tabled)]
struct GroupRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Type")]
    group_type: String,
    #[tabled(rename = "Members")]
    members: String,
}

fn group_row(group: &Arc<FirewallGroup>, painter: &output::Painter) -> GroupRow {
    let members_display = if group.group_members.len() <= 5 {
        group.group_members.join(", ")
    } else {
        format!(
            "{}, {} +{} more",
            group.group_members[0],
            group.group_members[1],
            group.group_members.len() - 2
        )
    };

    GroupRow {
        id: painter.id(&group.id.to_string()),
        name: painter.name(&group.name),
        group_type: painter.muted(&group.group_type.to_string()),
        members: painter.muted(&members_display),
    }
}

fn group_detail(group: &Arc<FirewallGroup>) -> String {
    let members = group
        .group_members
        .iter()
        .map(|m| format!("  - {m}"))
        .collect::<Vec<_>>()
        .join("\n");

    let mut lines = vec![
        format!("ID:          {}", group.id),
        format!(
            "External ID: {}",
            group.external_id.as_deref().unwrap_or("-")
        ),
        format!("Name:        {}", group.name),
        format!("Type:        {}", group.group_type),
        format!("Members:\n{members}"),
    ];

    if members.is_empty() {
        lines.pop();
        lines.push("Members:     (none)".into());
    }

    lines.join("\n")
}

fn map_group_type(arg: &FirewallGroupTypeArg) -> FirewallGroupType {
    match arg {
        FirewallGroupTypeArg::PortGroup => FirewallGroupType::PortGroup,
        FirewallGroupTypeArg::AddressGroup => FirewallGroupType::AddressGroup,
        FirewallGroupTypeArg::Ipv6AddressGroup => FirewallGroupType::Ipv6AddressGroup,
    }
}

pub(super) async fn handle(
    controller: &Controller,
    cmd: FirewallGroupsCommand,
    global: &GlobalOpts,
    painter: &output::Painter,
) -> Result<(), CliError> {
    match cmd {
        FirewallGroupsCommand::List { list, r#type } => {
            let all = controller.firewall_groups_snapshot();
            let type_filter = r#type.as_ref().map(map_group_type);
            let snapshot = util::apply_list_args(
                all.iter()
                    .filter(|g| type_filter.is_none_or(|t| g.group_type == t))
                    .cloned(),
                &list,
                util::matches_json_filter,
            );
            let out = output::render_list(
                &global.output,
                &snapshot,
                |group| group_row(group, painter),
                |group| group.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        FirewallGroupsCommand::Get { id } => {
            let snapshot = controller.firewall_groups_snapshot();
            let found = snapshot
                .iter()
                .find(|g| g.id.to_string() == id || g.external_id.as_deref() == Some(&id));
            match found {
                Some(group) => {
                    let out = output::render_single(&global.output, group, group_detail, |group| {
                        group.id.to_string()
                    });
                    output::print_output(&out, global.quiet);
                    Ok(())
                }
                None => Err(CliError::NotFound {
                    resource_type: "firewall group".into(),
                    identifier: id,
                    list_command: "firewall groups list".into(),
                }),
            }
        }

        FirewallGroupsCommand::Create {
            name,
            r#type,
            members,
            from_file,
        } => handle_create(controller, global, name, r#type, members, from_file).await,

        FirewallGroupsCommand::Update {
            id,
            name,
            members,
            from_file,
        } => handle_update(controller, global, id, name, members, from_file).await,

        FirewallGroupsCommand::Delete { id } => {
            if !util::confirm(&format!("Delete firewall group {id}?"), global.yes)? {
                return Ok(());
            }

            controller
                .execute(CoreCommand::DeleteFirewallGroup {
                    id: EntityId::from(id),
                })
                .await?;
            if !global.quiet {
                eprintln!("Firewall group deleted");
            }
            Ok(())
        }
    }
}

async fn handle_create(
    controller: &Controller,
    global: &GlobalOpts,
    name: Option<String>,
    r#type: FirewallGroupTypeArg,
    members: Option<Vec<String>>,
    from_file: Option<std::path::PathBuf>,
) -> Result<(), CliError> {
    let req = if let Some(ref path) = from_file {
        serde_json::from_value(util::read_json_file(path)?)?
    } else {
        CreateFirewallGroupRequest {
            name: name.unwrap_or_default(),
            group_type: map_group_type(&r#type),
            group_members: members.unwrap_or_default(),
        }
    };
    controller
        .execute(CoreCommand::CreateFirewallGroup(req))
        .await?;
    if !global.quiet {
        eprintln!("Firewall group created");
    }
    Ok(())
}

async fn handle_update(
    controller: &Controller,
    global: &GlobalOpts,
    id: String,
    name: Option<String>,
    members: Option<Vec<String>>,
    from_file: Option<std::path::PathBuf>,
) -> Result<(), CliError> {
    if from_file.is_none() && name.is_none() && members.is_none() {
        return Err(CliError::Validation {
            field: "update".into(),
            reason: "at least one of --name, --members, or --from-file is required".into(),
        });
    }

    let update = if let Some(ref path) = from_file {
        serde_json::from_value(util::read_json_file(path)?)?
    } else {
        UpdateFirewallGroupRequest {
            name,
            group_members: members,
        }
    };
    controller
        .execute(CoreCommand::UpdateFirewallGroup {
            id: EntityId::from(id),
            update,
        })
        .await?;
    if !global.quiet {
        eprintln!("Firewall group updated");
    }
    Ok(())
}
