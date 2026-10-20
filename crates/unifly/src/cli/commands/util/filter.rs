use crate::cli::args::{FilterOp, ListArgs, ListFilter};

/// Apply a list command's `--filter`, `--offset`, and `--limit`/`--all` to
/// `items`. Unless `quiet`, a hint on stderr says when the limit cut the
/// list short.
pub fn apply_list_args<T: serde::Serialize>(
    items: impl IntoIterator<Item = T>,
    list: &ListArgs,
    quiet: bool,
) -> Vec<T> {
    let offset = usize::try_from(list.offset).unwrap_or(usize::MAX);
    let limit = usize::try_from(list.limit).unwrap_or(usize::MAX);

    let filtered: Vec<T> = items
        .into_iter()
        .filter(|item| {
            list.filter
                .as_ref()
                .is_none_or(|filter| filter.matches(item))
        })
        .skip(offset)
        .collect();

    if list.all {
        filtered
    } else if filtered.len() > limit {
        if !quiet {
            eprintln!(
                "Showing {limit} of {} results. Use --all (-a) to see everything.",
                filtered.len()
            );
        }
        filtered.into_iter().take(limit).collect()
    } else {
        filtered
    }
}

impl ListFilter {
    /// Whether `item`, serialized to JSON, satisfies the expression.
    pub fn matches<T: serde::Serialize>(&self, item: &T) -> bool {
        let Some(expr) = &self.expr else {
            return true;
        };
        let Ok(value) = serde_json::to_value(item) else {
            return false;
        };

        let mut matches = Vec::new();
        collect_path_values(&value, &expr.path, &mut matches);
        matches
            .into_iter()
            .any(|candidate| matches_filter_value(candidate, &expr.op, &expr.values))
    }
}

fn collect_path_values<'a>(
    value: &'a serde_json::Value,
    path: &[String],
    out: &mut Vec<&'a serde_json::Value>,
) {
    if path.is_empty() {
        out.push(value);
        return;
    }

    match value {
        serde_json::Value::Object(map) => {
            if let Some(next) = map.get(&path[0]) {
                collect_path_values(next, &path[1..], out);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_path_values(item, path, out);
            }
        }
        _ => {}
    }
}

fn scalar_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        serde_json::Value::Null => Some("null".into()),
        _ => None,
    }
}

fn matches_filter_value(value: &serde_json::Value, op: &FilterOp, candidates: &[String]) -> bool {
    match value {
        serde_json::Value::Array(items) => items
            .iter()
            .any(|item| matches_filter_value(item, op, candidates)),
        serde_json::Value::Object(_) => false,
        scalar => {
            let Some(text) = scalar_text(scalar) else {
                return false;
            };
            let normalized = text.to_ascii_lowercase();

            match op {
                FilterOp::Eq => candidates
                    .iter()
                    .any(|candidate| normalized == candidate.to_ascii_lowercase()),
                FilterOp::Contains => candidates
                    .iter()
                    .any(|candidate| normalized.contains(&candidate.to_ascii_lowercase())),
                FilterOp::In => candidates
                    .iter()
                    .any(|candidate| normalized == candidate.to_ascii_lowercase()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ListFilter, apply_list_args};
    use crate::cli::args::ListArgs;

    fn filter(expr: &str) -> ListFilter {
        ListFilter::parse(expr).expect("valid filter")
    }

    #[test]
    fn apply_list_args_respects_offset_limit() {
        let args = ListArgs {
            limit: 2,
            offset: 1,
            all: false,
            filter: None,
        };
        let rows = vec![1, 2, 3, 4];
        let sliced = apply_list_args(rows, &args, true);
        assert_eq!(sliced, vec![2, 3]);
    }

    #[test]
    fn apply_list_args_supports_local_expression_filters() {
        let args = ListArgs {
            limit: 25,
            offset: 0,
            all: false,
            filter: Some(filter("name.eq('beta')")),
        };
        let rows = vec![
            serde_json::json!({"name":"alpha"}),
            serde_json::json!({"name":"beta"}),
        ];
        let filtered = apply_list_args(rows, &args, true);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0]["name"], "beta");
    }

    #[test]
    fn list_filter_supports_contains_and_in() {
        let item = serde_json::json!({
            "name": "Alpha Beta",
            "state": "online",
            "tags": ["primary", "uplink"],
            "nested": {
                "ssid": "NovaNet"
            }
        });

        assert!(filter("name.contains('beta')").matches(&item));
        assert!(filter("state.in('offline', 'ONLINE')").matches(&item));
        assert!(filter("nested.ssid.eq('novanet')").matches(&item));
        assert!(!filter("name.eq('gamma')").matches(&item));
    }

    #[test]
    fn list_filter_rejects_malformed_expressions() {
        for expr in [
            "Alpha Beta",
            "name.starts_with('alpha')",
            "name.eq('a','b')",
            "name.in()",
            "name.eq('unterminated)",
            ".eq('x')",
        ] {
            assert!(ListFilter::parse(expr).is_err(), "{expr} should not parse");
        }
    }

    #[test]
    fn empty_list_filter_matches_everything() {
        let empty = filter("  ");
        assert!(empty.matches(&serde_json::json!({"name": "anything"})));
    }
}
