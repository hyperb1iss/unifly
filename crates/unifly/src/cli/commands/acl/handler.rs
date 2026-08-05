use unifly_api::{Command as CoreCommand, Controller, EntityId};

use crate::cli::args::{AclArgs, AclCommand, GlobalOpts};
use crate::cli::commands::util;
use crate::cli::error::CliError;
use crate::cli::output;

use super::render::{acl_row, detail, render_reorder_ids};
use super::request::{build_acl_create_request, build_acl_update_request};

#[allow(clippy::too_many_lines)]
pub(super) async fn handle(
    controller: &Controller,
    args: AclArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    util::ensure_integration_access(controller, "acl").await?;

    let painter = output::Painter::new(global);

    match args.command {
        AclCommand::List(list) => {
            let all = controller.acl_rules_snapshot();
            let snapshot = util::apply_list_args(all.iter().cloned(), &list, |rule, filter| {
                util::matches_json_filter(rule, filter)
            });
            let out = output::render_list(
                &global.output,
                &snapshot,
                |rule| acl_row(rule, &painter),
                |rule| rule.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        AclCommand::Get { id } => {
            let snapshot = controller.acl_rules_snapshot();
            let found = snapshot.iter().find(|rule| rule.id.to_string() == id);
            match found {
                Some(rule) => {
                    let out = output::render_single(&global.output, rule, detail, |rule| {
                        rule.id.to_string()
                    });
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "ACL rule".into(),
                        identifier: id,
                        list_command: "acl list".into(),
                    });
                }
            }
            Ok(())
        }

        AclCommand::Create {
            from_file,
            name,
            rule_type,
            action,
            source_zone,
            dest_zone,
            protocol,
            source_port,
            destination_port,
        } => {
            let req = if let Some(path) = from_file.as_ref() {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                build_acl_create_request(
                    name,
                    rule_type,
                    action,
                    source_zone,
                    dest_zone,
                    protocol,
                    source_port,
                    destination_port,
                )?
            };
            let result = controller.execute(CoreCommand::CreateAclRule(req)).await?;
            if let unifly_api::CommandResult::AclRule(rule) = result {
                let rule = std::sync::Arc::new(rule);
                let out = crate::cli::output::render_single(
                    &global.output,
                    &rule,
                    super::render::detail,
                    |r| r.id.to_string(),
                );
                crate::cli::output::print_output(&out, global.quiet);
            }
            if !global.quiet {
                eprintln!("ACL rule created");
            }
            Ok(())
        }

        AclCommand::Update {
            id,
            name,
            rule_type,
            action,
            source_zone,
            dest_zone,
            protocol,
            source_port,
            destination_port,
            enabled,
            from_file,
        } => {
            let update = if let Some(path) = from_file.as_ref() {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                build_acl_update_request(
                    name,
                    rule_type,
                    action.as_ref(),
                    source_zone,
                    dest_zone,
                    protocol,
                    source_port,
                    destination_port,
                    enabled,
                )?
            };
            controller
                .execute(CoreCommand::UpdateAclRule {
                    id: EntityId::from(id),
                    update,
                })
                .await?;
            if !global.quiet {
                eprintln!("ACL rule updated");
            }
            Ok(())
        }

        AclCommand::Delete { id } => {
            if !util::confirm(&format!("Delete ACL rule {id}?"), global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::DeleteAclRule {
                    id: EntityId::from(id),
                })
                .await?;
            if !global.quiet {
                eprintln!("ACL rule deleted");
            }
            Ok(())
        }

        AclCommand::Reorder { get, set } => {
            if let Some(ids) = set {
                let ordered_ids: Vec<EntityId> = ids.into_iter().map(EntityId::from).collect();
                controller
                    .execute(CoreCommand::ReorderAclRules { ordered_ids })
                    .await?;
                if !global.quiet {
                    eprintln!("ACL rule order updated");
                }
            } else {
                let _ = get;
                let ordering = controller.get_acl_rule_ordering().await?;
                let ids = ordering
                    .ordered_acl_rule_ids
                    .into_iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>();
                let out = render_reorder_ids(&global.output, &ids);
                output::print_output(&out, global.quiet);
            }
            Ok(())
        }
    }
}
