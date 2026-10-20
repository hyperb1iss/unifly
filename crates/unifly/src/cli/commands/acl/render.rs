use std::sync::Arc;

use tabled::Tabled;
use unifly_api::model::AclRule;

use crate::cli::args::OutputFormat;
use crate::cli::output;

#[derive(Tabled)]
pub(super) struct AclRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Type")]
    rule_type: String,
    #[tabled(rename = "Action")]
    action: String,
    #[tabled(rename = "Enabled")]
    enabled: String,
}

pub(super) fn acl_row(rule: &Arc<AclRule>, painter: &output::Painter) -> AclRow {
    AclRow {
        id: painter.id(&rule.id.to_string()),
        name: painter.name(&rule.name),
        rule_type: painter.muted(&format!("{:?}", rule.rule_type)),
        action: painter.action(&format!("{:?}", rule.action)),
        enabled: painter.enabled(rule.enabled),
    }
}

pub(super) fn detail(rule: &Arc<AclRule>) -> String {
    [
        format!("ID:      {}", rule.id),
        format!("Name:    {}", rule.name),
        format!("Enabled: {}", rule.enabled),
        format!("Type:    {:?}", rule.rule_type),
        format!("Action:  {:?}", rule.action),
        format!("Source:  {}", rule.source_summary.as_deref().unwrap_or("-")),
        format!(
            "Dest:    {}",
            rule.destination_summary.as_deref().unwrap_or("-")
        ),
    ]
    .join("\n")
}

pub(super) fn render_reorder_ids(output: &OutputFormat, ids: &[String]) -> String {
    match output {
        OutputFormat::Json => serde_json::to_string_pretty(ids).unwrap_or_default(),
        OutputFormat::JsonCompact => serde_json::to_string(ids).unwrap_or_default(),
        OutputFormat::Yaml => output::render_yaml(ids),
        OutputFormat::Table | OutputFormat::Plain => ids.join("\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::render_reorder_ids;
    use crate::cli::args::OutputFormat;

    #[test]
    fn render_reorder_ids_keeps_json_compact_compact() {
        let rendered = render_reorder_ids(&OutputFormat::JsonCompact, &["a".into(), "b".into()]);
        assert_eq!(rendered, "[\"a\",\"b\"]");
    }

    #[test]
    fn render_reorder_ids_yaml_is_a_sequence() {
        let rendered = render_reorder_ids(&OutputFormat::Yaml, &["a".into(), "b:c".into()]);
        let parsed: Vec<String> = serde_yaml_ng::from_str(&rendered)
            .unwrap_or_else(|err| panic!("not a YAML sequence ({err}):\n{rendered}"));
        assert_eq!(parsed, ["a", "b:c"]);
    }

    #[test]
    fn render_reorder_ids_pretty_prints_json() {
        let rendered = render_reorder_ids(&OutputFormat::Json, &["a".into()]);
        assert!(rendered.contains('\n'));
    }
}
