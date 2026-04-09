//! NAT policy command handlers.

use std::sync::Arc;

use tabled::Tabled;
use unifly_api::model::NatPolicy;
use unifly_api::{
    Command as CoreCommand, Controller, CreateNatPolicyRequest, EntityId, UpdateNatPolicyRequest,
};

use crate::cli::args::{GlobalOpts, NatArgs, NatCommand, NatPoliciesCommand};
use crate::cli::error::CliError;
use crate::cli::output;

use super::util;

#[derive(Tabled)]
struct NatRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Type")]
    nat_type: String,
    #[tabled(rename = "Enabled")]
    enabled: String,
    #[tabled(rename = "Protocol")]
    protocol: String,
    #[tabled(rename = "Destination")]
    destination: String,
    #[tabled(rename = "Translation")]
    translation: String,
}

fn nat_row(policy: &Arc<NatPolicy>, painter: &output::Painter) -> NatRow {
    let destination = match (&policy.dst_address, &policy.dst_port) {
        (Some(addr), Some(port)) => format!("{addr}:{port}"),
        (Some(addr), None) => addr.clone(),
        (None, Some(port)) => format!("*:{port}"),
        (None, None) => "-".into(),
    };
    let translation = match (&policy.translated_address, &policy.translated_port) {
        (Some(addr), Some(port)) => format!("{addr}:{port}"),
        (Some(addr), None) => addr.clone(),
        (None, Some(port)) => format!("*:{port}"),
        (None, None) => "-".into(),
    };

    NatRow {
        id: painter.id(&policy.id.to_string()),
        name: painter.name(&policy.name),
        nat_type: format!("{:?}", policy.nat_type),
        enabled: painter.enabled(policy.enabled),
        protocol: policy.protocol.as_deref().unwrap_or("-").into(),
        destination: painter.muted(&destination),
        translation: painter.muted(&translation),
    }
}

fn nat_detail(policy: &Arc<NatPolicy>) -> String {
    let mut lines = vec![
        format!("ID:          {}", policy.id),
        format!("Name:        {}", policy.name),
        format!(
            "Description: {}",
            policy.description.as_deref().unwrap_or("-")
        ),
        format!("Enabled:     {}", policy.enabled),
        format!("Type:        {:?}", policy.nat_type),
        format!(
            "Interface:   {}",
            policy
                .interface_id
                .as_ref()
                .map_or("-".into(), ToString::to_string)
        ),
        format!("Protocol:    {}", policy.protocol.as_deref().unwrap_or("-")),
    ];

    if policy.src_address.is_some() || policy.src_port.is_some() {
        lines.push(format!(
            "Source:      {}{}",
            policy.src_address.as_deref().unwrap_or("*"),
            policy
                .src_port
                .as_ref()
                .map_or(String::new(), |p| format!(":{p}"))
        ));
    }
    if policy.dst_address.is_some() || policy.dst_port.is_some() {
        lines.push(format!(
            "Destination: {}{}",
            policy.dst_address.as_deref().unwrap_or("*"),
            policy
                .dst_port
                .as_ref()
                .map_or(String::new(), |p| format!(":{p}"))
        ));
    }
    if policy.translated_address.is_some() || policy.translated_port.is_some() {
        lines.push(format!(
            "Translated:  {}{}",
            policy.translated_address.as_deref().unwrap_or("*"),
            policy
                .translated_port
                .as_ref()
                .map_or(String::new(), |p| format!(":{p}"))
        ));
    }

    lines.join("\n")
}

pub async fn handle(
    controller: &Controller,
    args: NatArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    util::ensure_session_access(controller, "nat").await?;
    let painter = output::Painter::new(global);

    match args.command {
        NatCommand::Policies(args) => {
            handle_policies(controller, args.command, global, &painter).await
        }
    }
}

async fn handle_policies(
    controller: &Controller,
    cmd: NatPoliciesCommand,
    global: &GlobalOpts,
    painter: &output::Painter,
) -> Result<(), CliError> {
    match cmd {
        NatPoliciesCommand::List(list) => handle_list(controller, &list, global, painter),
        NatPoliciesCommand::Get { id } => handle_get(controller, &id, global),
        NatPoliciesCommand::Create {
            from_file,
            name,
            nat_type,
            interface_id,
            protocol,
            src_address,
            src_port,
            dst_address,
            dst_port,
            translated_address,
            translated_port,
            enabled,
            description,
        } => {
            handle_create(
                controller,
                from_file,
                name,
                nat_type,
                interface_id,
                protocol,
                src_address,
                src_port,
                dst_address,
                dst_port,
                translated_address,
                translated_port,
                enabled,
                description,
                global,
            )
            .await
        }
        NatPoliciesCommand::Update {
            id,
            name,
            nat_type,
            interface_id,
            protocol,
            src_address,
            src_port,
            dst_address,
            dst_port,
            translated_address,
            translated_port,
            enabled,
            description,
            from_file,
        } => {
            handle_update(
                controller,
                id,
                from_file,
                name,
                nat_type,
                interface_id,
                protocol,
                src_address,
                src_port,
                dst_address,
                dst_port,
                translated_address,
                translated_port,
                enabled,
                description,
                global,
            )
            .await
        }
        NatPoliciesCommand::Delete { id } => handle_delete(controller, &id, global).await,
    }
}

#[allow(clippy::unnecessary_wraps)]
fn handle_list(
    controller: &Controller,
    list: &crate::cli::args::ListArgs,
    global: &GlobalOpts,
    painter: &output::Painter,
) -> Result<(), CliError> {
    let all = controller.nat_policies_snapshot();
    let snapshot = util::apply_list_args(all.iter().cloned(), list, |policy, filter| {
        util::matches_json_filter(policy, filter)
    });
    let out = output::render_list(
        &global.output,
        &snapshot,
        |policy| nat_row(policy, painter),
        |policy| policy.id.to_string(),
    );
    output::print_output(&out, global.quiet);
    Ok(())
}

fn handle_get(controller: &Controller, id: &str, global: &GlobalOpts) -> Result<(), CliError> {
    let snapshot = controller.nat_policies_snapshot();
    let found = snapshot.iter().find(|p| p.id.to_string() == id);
    match found {
        Some(policy) => {
            let out =
                output::render_single(&global.output, policy, nat_detail, |p| p.id.to_string());
            output::print_output(&out, global.quiet);
        }
        None => {
            return Err(CliError::NotFound {
                resource_type: "NAT policy".into(),
                identifier: id.to_string(),
                list_command: "nat policies list".into(),
            });
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_create(
    controller: &Controller,
    from_file: Option<std::path::PathBuf>,
    name: Option<String>,
    nat_type: Option<String>,
    interface_id: Option<String>,
    protocol: Option<String>,
    src_address: Option<String>,
    src_port: Option<String>,
    dst_address: Option<String>,
    dst_port: Option<String>,
    translated_address: Option<String>,
    translated_port: Option<String>,
    enabled: bool,
    description: Option<String>,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    let req: CreateNatPolicyRequest = if let Some(path) = from_file.as_ref() {
        serde_json::from_value(util::read_json_file(path)?)?
    } else {
        CreateNatPolicyRequest {
            name: name.unwrap_or_default(),
            nat_type: nat_type.unwrap_or_default(),
            description,
            enabled,
            interface_id: interface_id.map(EntityId::from),
            protocol,
            src_address,
            src_port,
            dst_address,
            dst_port,
            translated_address,
            translated_port,
        }
    };

    controller
        .execute(CoreCommand::CreateNatPolicy(req))
        .await?;
    if !global.quiet {
        eprintln!("NAT policy created");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_update(
    controller: &Controller,
    id: String,
    from_file: Option<std::path::PathBuf>,
    name: Option<String>,
    nat_type: Option<String>,
    interface_id: Option<String>,
    protocol: Option<String>,
    src_address: Option<String>,
    src_port: Option<String>,
    dst_address: Option<String>,
    dst_port: Option<String>,
    translated_address: Option<String>,
    translated_port: Option<String>,
    enabled: Option<bool>,
    description: Option<String>,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    let update: UpdateNatPolicyRequest = if let Some(path) = from_file.as_ref() {
        serde_json::from_value(util::read_json_file(path)?)?
    } else {
        if name.is_none()
            && nat_type.is_none()
            && interface_id.is_none()
            && protocol.is_none()
            && src_address.is_none()
            && src_port.is_none()
            && dst_address.is_none()
            && dst_port.is_none()
            && translated_address.is_none()
            && translated_port.is_none()
            && enabled.is_none()
            && description.is_none()
        {
            return Err(CliError::Validation {
                field: "update".into(),
                reason: "at least one update flag or --from-file is required".into(),
            });
        }

        UpdateNatPolicyRequest {
            name,
            nat_type,
            description,
            enabled,
            interface_id: interface_id.map(EntityId::from),
            protocol,
            src_address,
            src_port,
            dst_address,
            dst_port,
            translated_address,
            translated_port,
        }
    };

    controller
        .execute(CoreCommand::UpdateNatPolicy {
            id: EntityId::from(id),
            update,
        })
        .await?;
    if !global.quiet {
        eprintln!("NAT policy updated");
    }
    Ok(())
}

async fn handle_delete(
    controller: &Controller,
    id: &str,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    if !util::confirm(&format!("Delete NAT policy {id}?"), global.yes)? {
        return Ok(());
    }

    controller
        .execute(CoreCommand::DeleteNatPolicy {
            id: EntityId::from(id.to_string()),
        })
        .await?;
    if !global.quiet {
        eprintln!("NAT policy deleted");
    }
    Ok(())
}
