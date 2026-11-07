use tabled::Tabled;
use unifly_api::{
    Controller,
    integration_types::{LagConfiguration, LagResponse, McLagDomainResponse, SwitchStackResponse},
};

use crate::cli::{
    args::{GlobalOpts, SwitchingCommand},
    commands::util,
    error::CliError,
    output,
};

#[derive(Tabled)]
struct LagRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Type")]
    kind: &'static str,
    #[tabled(rename = "Members")]
    members: usize,
}

#[derive(Tabled)]
struct DomainRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Peers")]
    peers: usize,
    #[tabled(rename = "LAGs")]
    lags: usize,
}

#[derive(Tabled)]
struct StackRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Units")]
    units: usize,
    #[tabled(rename = "LAGs")]
    lags: usize,
}

fn lag_row(lag: &LagResponse) -> LagRow {
    let (kind, members) = match &lag.configuration {
        LagConfiguration::Local { members } => ("LOCAL", members.len()),
        LagConfiguration::MultiChassis { members, .. } => ("MULTI_CHASSIS", members.len()),
        LagConfiguration::SwitchStack { members, .. } => ("SWITCH_STACK", members.len()),
    };
    LagRow {
        id: lag.id.to_string(),
        kind,
        members,
    }
}

fn domain_row(domain: &McLagDomainResponse) -> DomainRow {
    DomainRow {
        id: domain.id.to_string(),
        name: domain.name.clone(),
        peers: domain.peers.len(),
        lags: domain.lags.len(),
    }
}

fn stack_row(stack: &SwitchStackResponse) -> StackRow {
    StackRow {
        id: stack.id.to_string(),
        name: stack.name.clone(),
        units: stack.units.len(),
        lags: stack.lags.len(),
    }
}

pub(super) async fn lags(
    controller: &Controller,
    command: SwitchingCommand,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    util::ensure_integration_access(controller, "switching inventory").await?;
    let out = match command {
        SwitchingCommand::List(list) => {
            let mut records = controller.list_lags().await?;
            records.sort_by_key(|record| record.id);
            let records = util::apply_list_args(records, &list, global.quiet);
            output::render_list(&global.output, &records, lag_row, |record| {
                record.id.to_string()
            })
        }
        SwitchingCommand::Get { id } => {
            let id = id.parse().map_err(|_| CliError::Validation {
                field: "id".into(),
                reason: "expected an Integration API UUID".into(),
            })?;
            let record = controller.get_lag(&id).await?;
            output::render_single(
                &global.output,
                &record,
                output::render_json_pretty,
                |record| record.id.to_string(),
            )
        }
    };
    output::print_output(&out, global.quiet);
    Ok(())
}

pub(super) async fn mc_lag_domains(
    controller: &Controller,
    command: SwitchingCommand,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    util::ensure_integration_access(controller, "switching inventory").await?;
    let out = match command {
        SwitchingCommand::List(list) => {
            let mut records = controller.list_mc_lag_domains().await?;
            records.sort_by_key(|record| record.id);
            let records = util::apply_list_args(records, &list, global.quiet);
            output::render_list(&global.output, &records, domain_row, |record| {
                record.id.to_string()
            })
        }
        SwitchingCommand::Get { id } => {
            let id = id.parse().map_err(|_| CliError::Validation {
                field: "id".into(),
                reason: "expected an Integration API UUID".into(),
            })?;
            let record = controller.get_mc_lag_domain(&id).await?;
            output::render_single(
                &global.output,
                &record,
                output::render_json_pretty,
                |record| record.id.to_string(),
            )
        }
    };
    output::print_output(&out, global.quiet);
    Ok(())
}

pub(super) async fn switch_stacks(
    controller: &Controller,
    command: SwitchingCommand,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    util::ensure_integration_access(controller, "switching inventory").await?;
    let out = match command {
        SwitchingCommand::List(list) => {
            let mut records = controller.list_switch_stacks().await?;
            records.sort_by_key(|record| record.id);
            let records = util::apply_list_args(records, &list, global.quiet);
            output::render_list(&global.output, &records, stack_row, |record| {
                record.id.to_string()
            })
        }
        SwitchingCommand::Get { id } => {
            let id = id.parse().map_err(|_| CliError::Validation {
                field: "id".into(),
                reason: "expected an Integration API UUID".into(),
            })?;
            let record = controller.get_switch_stack(&id).await?;
            output::render_single(
                &global.output,
                &record,
                output::render_json_pretty,
                |record| record.id.to_string(),
            )
        }
    };
    output::print_output(&out, global.quiet);
    Ok(())
}
