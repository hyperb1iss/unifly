use std::sync::Arc;

use tabled::Tabled;
use unifly_api::model::FirewallPolicy;
use unifly_api::{
    Command as CoreCommand, Controller, CreateFirewallPolicyRequest, EntityId,
    UpdateFirewallPolicyRequest,
};

use crate::cli::args::{FirewallPoliciesCommand, GlobalOpts, OutputFormat};
use crate::cli::error::CliError;
use crate::cli::output;

use super::shared::{build_filter_spec, map_fw_action, parse_reorder_zone_pair};
use super::util;

#[derive(Tabled)]
struct PolicyRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Action")]
    action: String,
    #[tabled(rename = "Enabled")]
    enabled: String,
    #[tabled(rename = "Source")]
    source: String,
    #[tabled(rename = "Destination")]
    destination: String,
}

fn policy_row(policy: &Arc<FirewallPolicy>, painter: &output::Painter) -> PolicyRow {
    let source = policy
        .source_summary
        .as_deref()
        .or_else(|| policy.source.zone_id.as_ref().map(|_| "zone-only"))
        .unwrap_or("-");
    let destination = policy
        .destination_summary
        .as_deref()
        .or_else(|| policy.destination.zone_id.as_ref().map(|_| "zone-only"))
        .unwrap_or("-");

    PolicyRow {
        id: painter.id(&policy.id.to_string()),
        name: painter.name(&policy.name),
        action: painter.action(&format!("{:?}", policy.action)),
        enabled: painter.enabled(policy.enabled),
        source: painter.muted(source),
        destination: painter.muted(destination),
    }
}

fn policy_detail(policy: &Arc<FirewallPolicy>) -> String {
    let mut lines = vec![
        format!("ID:          {}", policy.id),
        format!("Name:        {}", policy.name),
        format!(
            "Description: {}",
            policy.description.as_deref().unwrap_or("-")
        ),
        format!("Enabled:     {}", policy.enabled),
        format!("Action:      {:?}", policy.action),
        format!("IP Version:  {:?}", policy.ip_version),
        format!(
            "Src Zone:    {}",
            policy
                .source
                .zone_id
                .as_ref()
                .map_or_else(|| "-".into(), ToString::to_string)
        ),
    ];
    if let Some(filter) = &policy.source.filter {
        lines.push(format!("Src Filter:  {}", filter.summary()));
    }
    lines.push(format!(
        "Dst Zone:    {}",
        policy
            .destination
            .zone_id
            .as_ref()
            .map_or_else(|| "-".into(), ToString::to_string)
    ));
    if let Some(filter) = &policy.destination.filter {
        lines.push(format!("Dst Filter:  {}", filter.summary()));
    }
    lines.push(format!(
        "States:      {}",
        if policy.connection_states.is_empty() {
            "any".into()
        } else {
            policy.connection_states.join(", ")
        }
    ));
    lines.push(format!("Logging:     {}", policy.logging_enabled));
    lines.join("\n")
}

#[allow(clippy::too_many_lines)]
pub(super) async fn handle(
    controller: &Controller,
    cmd: FirewallPoliciesCommand,
    global: &GlobalOpts,
    painter: &output::Painter,
) -> Result<(), CliError> {
    match cmd {
        FirewallPoliciesCommand::List(list) => {
            let all = controller.firewall_policies_snapshot();
            let snapshot = util::apply_list_args(all.iter().cloned(), &list, |policy, filter| {
                util::matches_json_filter(policy, filter)
            });
            let out = output::render_list(
                &global.output,
                &snapshot,
                |policy| policy_row(policy, painter),
                |policy| policy.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        FirewallPoliciesCommand::Get { id } => {
            let snapshot = controller.firewall_policies_snapshot();
            let found = snapshot.iter().find(|policy| policy.id.to_string() == id);
            match found {
                Some(policy) => {
                    let out =
                        output::render_single(&global.output, policy, policy_detail, |policy| {
                            policy.id.to_string()
                        });
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "firewall policy".into(),
                        identifier: id,
                        list_command: "firewall policies list".into(),
                    });
                }
            }
            Ok(())
        }

        FirewallPoliciesCommand::Create {
            from_file,
            name,
            action,
            source_zone,
            dest_zone,
            enabled,
            description,
            logging,
            allow_return_traffic,
            src_network,
            src_ip,
            src_port,
            dst_network,
            dst_ip,
            dst_port,
            states,
            ip_version,
            after_system,
        } => {
            handle_create(
                controller,
                global,
                from_file,
                name,
                action,
                source_zone,
                dest_zone,
                enabled,
                description,
                logging,
                allow_return_traffic,
                src_network,
                src_ip,
                src_port,
                dst_network,
                dst_ip,
                dst_port,
                states,
                ip_version,
                after_system,
            )
            .await
        }

        FirewallPoliciesCommand::Update {
            id,
            allow_return_traffic,
            from_file,
            src_network,
            src_ip,
            src_port,
            dst_network,
            dst_ip,
            dst_port,
            states,
            ip_version,
        } => {
            handle_update(
                controller,
                global,
                id,
                allow_return_traffic,
                from_file,
                src_network,
                src_ip,
                src_port,
                dst_network,
                dst_ip,
                dst_port,
                states,
                ip_version,
            )
            .await
        }

        FirewallPoliciesCommand::Patch {
            id,
            enabled,
            logging,
        } => {
            if enabled.is_none() && logging.is_none() {
                return Err(CliError::Validation {
                    field: "patch".into(),
                    reason: "at least one of --enabled or --logging is required".into(),
                });
            }

            controller
                .execute(CoreCommand::PatchFirewallPolicy {
                    id: EntityId::from(id),
                    enabled,
                    logging,
                })
                .await?;
            if !global.quiet {
                let mut parts = Vec::new();
                if let Some(enabled) = enabled {
                    parts.push(if enabled { "enabled" } else { "disabled" });
                }
                if let Some(logging) = logging {
                    parts.push(if logging {
                        "logging enabled"
                    } else {
                        "logging disabled"
                    });
                }
                eprintln!("Firewall policy {}", parts.join(", "));
            }
            Ok(())
        }

        FirewallPoliciesCommand::Delete { id } => {
            if !util::confirm(&format!("Delete firewall policy {id}?"), global.yes)? {
                return Ok(());
            }

            controller
                .execute(CoreCommand::DeleteFirewallPolicy {
                    id: EntityId::from(id.clone()),
                })
                .await?;
            if !global.quiet {
                eprintln!("Firewall policy deleted");
            }
            Ok(())
        }

        FirewallPoliciesCommand::Reorder {
            source_zone,
            dest_zone,
            get,
            set,
            after_system,
        } => {
            let zone_pair =
                parse_reorder_zone_pair(Some(source_zone.as_str()), Some(dest_zone.as_str()))?;

            if let Some(ids) = set {
                let new_ids = ids
                    .into_iter()
                    .map(|s| match EntityId::from(s.clone()) {
                        id @ EntityId::Uuid(_) => Ok(id),
                        EntityId::Legacy(_) => Err(CliError::Validation {
                            field: "set".into(),
                            reason: format!("\"{s}\" is not a valid policy UUID"),
                        }),
                    })
                    .collect::<Result<Vec<EntityId>, _>>()?;
                let ordering = controller
                    .get_firewall_policy_ordering(&zone_pair.0, &zone_pair.1)
                    .await?;
                let preserved: Vec<EntityId> = if after_system {
                    ordering
                        .before_system_defined
                        .into_iter()
                        .map(EntityId::Uuid)
                        .collect()
                } else {
                    ordering
                        .after_system_defined
                        .into_iter()
                        .map(EntityId::Uuid)
                        .collect()
                };

                // Reject overlap: a policy can't be in both halves at once.
                // Without this check the controller PUT would have the same
                // UUID in both lists, which the controller may silently
                // dedupe or reject ambiguously.
                let preserved_set: std::collections::HashSet<&EntityId> =
                    preserved.iter().collect();
                if let Some(overlap) = new_ids.iter().find(|id| preserved_set.contains(id)) {
                    let side = if after_system { "before-system" } else { "after-system" };
                    return Err(CliError::Validation {
                        field: "set".into(),
                        reason: format!(
                            "policy \"{overlap}\" is already in the {side} ordering for this zone-pair"
                        ),
                    });
                }

                let (before_system_ids, after_system_ids) = if after_system {
                    (preserved, new_ids)
                } else {
                    (new_ids, preserved)
                };
                controller
                    .execute(CoreCommand::ReorderFirewallPolicies {
                        zone_pair,
                        before_system_ids,
                        after_system_ids,
                    })
                    .await?;
                if !global.quiet {
                    eprintln!("Firewall policy order updated");
                }
            } else {
                let _ = get;
                let ordering = controller
                    .get_firewall_policy_ordering(&zone_pair.0, &zone_pair.1)
                    .await?;
                let out = match &global.output {
                    OutputFormat::Table | OutputFormat::Plain => {
                        let before = ordering
                            .before_system_defined
                            .iter()
                            .map(|id| format!("  - {id}"))
                            .collect::<Vec<_>>()
                            .join("\n");
                        let after = ordering
                            .after_system_defined
                            .iter()
                            .map(|id| format!("  - {id}"))
                            .collect::<Vec<_>>()
                            .join("\n");
                        format!(
                            "Before System Defined:\n{}\n\nAfter System Defined:\n{}",
                            if before.is_empty() {
                                "  (none)"
                            } else {
                                &before
                            },
                            if after.is_empty() { "  (none)" } else { &after }
                        )
                    }
                    OutputFormat::Json => {
                        serde_json::to_string_pretty(&ordering).unwrap_or_default()
                    }
                    OutputFormat::JsonCompact => {
                        serde_json::to_string(&ordering).unwrap_or_default()
                    }
                    OutputFormat::Yaml => serde_yaml_ng::to_string(&ordering).unwrap_or_default(),
                };
                output::print_output(&out, global.quiet);
            }

            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
async fn handle_create(
    controller: &Controller,
    global: &GlobalOpts,
    from_file: Option<std::path::PathBuf>,
    name: Option<String>,
    action: Option<crate::cli::args::FirewallAction>,
    source_zone: Option<String>,
    dest_zone: Option<String>,
    enabled: bool,
    description: Option<String>,
    logging: bool,
    allow_return_traffic: bool,
    src_network: Option<Vec<String>>,
    src_ip: Option<Vec<String>>,
    src_port: Option<Vec<String>>,
    dst_network: Option<Vec<String>>,
    dst_ip: Option<Vec<String>>,
    dst_port: Option<Vec<String>>,
    states: Option<Vec<String>>,
    ip_version: Option<String>,
    after_system: bool,
) -> Result<(), CliError> {
    let req = if let Some(path) = from_file.as_ref() {
        let mut req: CreateFirewallPolicyRequest =
            serde_json::from_value(util::read_json_file(path)?)?;
        req.resolve_filters()
            .map_err(|reason| CliError::Validation {
                field: "filter".into(),
                reason,
            })?;
        req
    } else {
        CreateFirewallPolicyRequest {
            name: name.unwrap_or_default(),
            action: action
                .as_ref()
                .map_or(unifly_api::model::FirewallAction::Block, map_fw_action),
            source_zone_id: EntityId::from(source_zone.unwrap_or_default()),
            destination_zone_id: EntityId::from(dest_zone.unwrap_or_default()),
            enabled,
            logging_enabled: logging,
            allow_return_traffic,
            description,
            ip_version,
            connection_states: states,
            source_filter: build_filter_spec("src", src_network, src_ip, src_port)?,
            destination_filter: build_filter_spec("dst", dst_network, dst_ip, dst_port)?,
            src_network: None,
            src_ip: None,
            src_port: None,
            dst_network: None,
            dst_ip: None,
            dst_port: None,
        }
    };

    let source_zone_id = req.source_zone_id.clone();
    let destination_zone_id = req.destination_zone_id.clone();

    let result = controller
        .execute(CoreCommand::CreateFirewallPolicy(req))
        .await?;

    let created_id = match &result {
        unifly_api::CommandResult::CreatedId(id) => Some(id.to_string()),
        _ => None,
    };

    let reorder_err = if after_system {
        move_policy_after_system(controller, result, source_zone_id, destination_zone_id)
            .await
            .err()
    } else {
        None
    };

    if !global.quiet {
        match (after_system, &reorder_err, &created_id) {
            (true, Some(err), Some(id)) => {
                eprintln!(
                    "Firewall policy created (id {id}); reorder after system-defined rules failed: {err}"
                );
                eprintln!(
                    "Use `unifly firewall policies reorder --source-zone <ID> --dest-zone <ID> --set ... --after-system` to retry."
                );
            }
            (true, Some(err), None) => {
                eprintln!(
                    "Firewall policy created; reorder after system-defined rules failed: {err}"
                );
            }
            (true, None, _) => eprintln!("Firewall policy created (after system-defined rules)"),
            (false, _, _) => eprintln!("Firewall policy created"),
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_update(
    controller: &Controller,
    global: &GlobalOpts,
    id: String,
    allow_return_traffic: Option<bool>,
    from_file: Option<std::path::PathBuf>,
    src_network: Option<Vec<String>>,
    src_ip: Option<Vec<String>>,
    src_port: Option<Vec<String>>,
    dst_network: Option<Vec<String>>,
    dst_ip: Option<Vec<String>>,
    dst_port: Option<Vec<String>>,
    states: Option<Vec<String>>,
    ip_version: Option<String>,
) -> Result<(), CliError> {
    if from_file.is_none()
        && allow_return_traffic.is_none()
        && src_network.is_none()
        && src_ip.is_none()
        && src_port.is_none()
        && dst_network.is_none()
        && dst_ip.is_none()
        && dst_port.is_none()
        && states.is_none()
        && ip_version.is_none()
    {
        return Err(CliError::Validation {
            field: "update".into(),
            reason: "at least one update flag or --from-file is required".into(),
        });
    }

    let update = if let Some(path) = from_file.as_ref() {
        let mut update: UpdateFirewallPolicyRequest =
            serde_json::from_value(util::read_json_file(path)?)?;
        update
            .resolve_filters()
            .map_err(|reason| CliError::Validation {
                field: "filter".into(),
                reason,
            })?;
        update
    } else {
        UpdateFirewallPolicyRequest {
            allow_return_traffic,
            source_filter: build_filter_spec("src", src_network, src_ip, src_port)?,
            destination_filter: build_filter_spec("dst", dst_network, dst_ip, dst_port)?,
            connection_states: states,
            ip_version,
            ..UpdateFirewallPolicyRequest::default()
        }
    };

    controller
        .execute(CoreCommand::UpdateFirewallPolicy {
            id: EntityId::from(id),
            update,
        })
        .await?;
    if !global.quiet {
        eprintln!("Firewall policy updated");
    }
    Ok(())
}

/// Move a newly created firewall policy from "Before System Defined" to
/// "After System Defined" in the zone-pair ordering.
async fn move_policy_after_system(
    controller: &Controller,
    result: unifly_api::CommandResult,
    source_zone_id: EntityId,
    destination_zone_id: EntityId,
) -> Result<(), CliError> {
    if let unifly_api::CommandResult::CreatedId(created_id) = result {
        let mut ordering = controller
            .get_firewall_policy_ordering(&source_zone_id, &destination_zone_id)
            .await?;
        if let EntityId::Uuid(uuid) = &created_id {
            ordering.before_system_defined.retain(|id| id != uuid);
            ordering.after_system_defined.push(*uuid);
        }
        controller
            .execute(CoreCommand::ReorderFirewallPolicies {
                zone_pair: (source_zone_id, destination_zone_id),
                before_system_ids: ordering
                    .before_system_defined
                    .into_iter()
                    .map(EntityId::Uuid)
                    .collect(),
                after_system_ids: ordering
                    .after_system_defined
                    .into_iter()
                    .map(EntityId::Uuid)
                    .collect(),
            })
            .await?;
    }
    Ok(())
}
