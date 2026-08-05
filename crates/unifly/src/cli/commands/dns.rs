//! DNS policy command handlers.

use std::sync::Arc;

use tabled::Tabled;
use unifly_api::model::{DnsPolicy, DnsPolicyType};
use unifly_api::{
    Command as CoreCommand, Controller, CreateDnsPolicyRequest, EntityId, UpdateDnsPolicyRequest,
};

use crate::cli::args::{DnsArgs, DnsCommand, DnsRecordType, GlobalOpts};
use crate::cli::error::CliError;
use crate::cli::output;

use super::util;

fn map_dns_type(rt: &DnsRecordType) -> DnsPolicyType {
    match rt {
        DnsRecordType::A => DnsPolicyType::ARecord,
        DnsRecordType::Aaaa => DnsPolicyType::AaaaRecord,
        DnsRecordType::Cname => DnsPolicyType::CnameRecord,
        DnsRecordType::Mx => DnsPolicyType::MxRecord,
        DnsRecordType::Txt => DnsPolicyType::TxtRecord,
        DnsRecordType::Srv => DnsPolicyType::SrvRecord,
        DnsRecordType::Forward => DnsPolicyType::ForwardDomain,
    }
}

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct DnsRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Type")]
    record_type: String,
    #[tabled(rename = "Domain")]
    domain: String,
    #[tabled(rename = "Value")]
    value: String,
    #[tabled(rename = "TTL")]
    ttl: String,
}

fn dns_row(d: &Arc<DnsPolicy>, p: &output::Painter) -> DnsRow {
    DnsRow {
        id: p.id(&d.id.to_string()),
        record_type: p.muted(&d.policy_type.to_string()),
        domain: p.name(&d.domain),
        value: p.ip(&d.value),
        ttl: p.number(&d.ttl_seconds.map(|t| t.to_string()).unwrap_or_default()),
    }
}

fn detail(d: &Arc<DnsPolicy>) -> String {
    [
        format!("ID:     {}", d.id),
        format!("Type:   {}", d.policy_type),
        format!("Domain: {}", d.domain),
        format!("Value:  {}", d.value),
        format!(
            "TTL:    {}",
            d.ttl_seconds
                .map_or_else(|| "-".into(), |t: u32| t.to_string())
        ),
    ]
    .join("\n")
}

// ── Handler ─────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
pub async fn handle(
    controller: &Controller,
    args: DnsArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    util::ensure_integration_access(controller, "dns").await?;

    let p = output::Painter::new(global);

    match args.command {
        DnsCommand::List(list) => {
            let all = controller.dns_policies_snapshot();
            let snap = util::apply_list_args(all.iter().cloned(), &list, |d, filter| {
                util::matches_json_filter(d, filter)
            });
            let out = output::render_list(
                &global.output,
                &snap,
                |d| dns_row(d, &p),
                |d| d.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        DnsCommand::Get { id } => {
            let snap = controller.dns_policies_snapshot();
            let found = snap.iter().find(|d| d.id.to_string() == id);
            match found {
                Some(d) => {
                    let out =
                        output::render_single(&global.output, d, detail, |d| d.id.to_string());
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "DNS policy".into(),
                        identifier: id,
                        list_command: "dns list".into(),
                    });
                }
            }
            Ok(())
        }

        DnsCommand::Create {
            from_file,
            record_type,
            domain,
            value,
            ttl,
            priority,
        } => {
            let req = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                CreateDnsPolicyRequest {
                    name: domain.clone().unwrap_or_default(),
                    policy_type: record_type
                        .as_ref()
                        .map_or(DnsPolicyType::ARecord, map_dns_type),
                    enabled: true,
                    domain: None,
                    domains: domain.map(|d| vec![d]),
                    upstream: None,
                    value,
                    ttl_seconds: Some(ttl),
                    priority,
                    ipv4_address: None,
                    ipv6_address: None,
                    target_domain: None,
                    mail_server_domain: None,
                    text: None,
                    ip_address: None,
                    server_domain: None,
                    service: None,
                    protocol: None,
                    port: None,
                    weight: None,
                }
            };

            let result = controller
                .execute(CoreCommand::CreateDnsPolicy(req))
                .await?;
            if let unifly_api::CommandResult::DnsPolicy(policy) = result {
                let policy = Arc::new(policy);
                let out =
                    output::render_single(&global.output, &policy, detail, |d| d.id.to_string());
                output::print_output(&out, global.quiet);
            } else {
                output::warn_unrenderable_create(global.quiet);
            }
            if !global.quiet {
                eprintln!("DNS policy created");
            }
            Ok(())
        }

        DnsCommand::Update { id, from_file } => {
            if from_file.is_none() {
                return Err(CliError::Validation {
                    field: "update".into(),
                    reason: "DNS updates currently require --from-file".into(),
                });
            }
            let update = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                UpdateDnsPolicyRequest::default()
            };
            let eid = EntityId::from(id);
            controller
                .execute(CoreCommand::UpdateDnsPolicy { id: eid, update })
                .await?;
            if !global.quiet {
                eprintln!("DNS policy updated");
            }
            Ok(())
        }

        DnsCommand::Delete { id } => {
            let eid = EntityId::from(id.clone());
            if !util::confirm(&format!("Delete DNS policy {id}?"), global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::DeleteDnsPolicy { id: eid })
                .await?;
            if !global.quiet {
                eprintln!("DNS policy deleted");
            }
            Ok(())
        }
    }
}
