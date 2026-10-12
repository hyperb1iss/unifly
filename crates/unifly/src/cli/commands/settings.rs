use tabled::Tabled;
use unifly_api::Controller;

use crate::cli::args::{GlobalOpts, OutputFormat, SettingsArgs, SettingsCommand, SettingsSetArgs};
use crate::cli::error::CliError;
use crate::cli::output;

#[derive(Tabled)]
struct SettingSectionRow {
    #[tabled(rename = "Key")]
    key: String,
    #[tabled(rename = "Fields")]
    field_count: String,
    #[tabled(rename = "Enabled")]
    enabled: String,
    #[tabled(rename = "Notable")]
    notable: String,
}

fn section_row(section: &serde_json::Value, p: &output::Painter) -> SettingSectionRow {
    let obj = section.as_object();
    let key = obj
        .and_then(|o| o.get("key"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("?");

    let field_count = obj.map_or(0, |o| {
        o.keys()
            .filter(|k| !matches!(k.as_str(), "_id" | "key" | "site_id"))
            .count()
    });

    let enabled = obj
        .and_then(|o| o.get("enabled"))
        .and_then(serde_json::Value::as_bool);

    let notable = build_notable_summary(obj);

    SettingSectionRow {
        key: p.name(key),
        field_count: p.number(&field_count.to_string()),
        enabled: match enabled {
            Some(v) => p.enabled(v),
            None => p.muted("-"),
        },
        notable: p.muted(&notable),
    }
}

fn build_notable_summary(obj: Option<&serde_json::Map<String, serde_json::Value>>) -> String {
    let Some(obj) = obj else {
        return String::new();
    };
    let mut notable = Vec::new();
    for (k, v) in obj {
        if matches!(k.as_str(), "_id" | "key" | "site_id" | "enabled") {
            continue;
        }
        if is_secret_field(k) || v.is_object() || v.is_array() {
            continue;
        }
        let display = match v {
            serde_json::Value::String(s) if s.len() > 24 => {
                format!("{}...", s.get(..24).unwrap_or(s))
            }
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        notable.push(format!("{k}={display}"));
        if notable.len() >= 2 {
            break;
        }
    }
    notable.join(", ")
}

/// Whether a settings field holds a credential or internal secret. The
/// controller marks these with an `x_` prefix (`x_password`,
/// `x_shared_secret`, ...).
fn is_secret_field(key: &str) -> bool {
    key.starts_with("x_")
}

/// Remove every secret field, at any depth, from a settings payload.
///
/// Fields are dropped rather than masked: a `***` placeholder fed back from
/// an edited export would overwrite the real credential, while an absent
/// one is restored by [`guard_secrets`] when the section is written back.
/// Returns how many fields were removed.
fn strip_secrets(value: &mut serde_json::Value) -> usize {
    match value {
        serde_json::Value::Object(map) => {
            let before = map.len();
            map.retain(|key, _| !is_secret_field(key));
            let removed = before - map.len();
            removed + map.values_mut().map(strip_secrets).sum::<usize>()
        }
        serde_json::Value::Array(items) => items.iter_mut().map(strip_secrets).sum(),
        _ => 0,
    }
}

/// Replace every secret value, at any depth, with `***` for display.
fn mask_secrets(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, field) in map.iter_mut() {
                if is_secret_field(key) {
                    *field = serde_json::Value::String("***".to_owned());
                } else {
                    mask_secrets(field);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                mask_secrets(item);
            }
        }
        _ => {}
    }
}

/// Copy secret fields from `current` into `incoming` wherever `incoming`
/// omits them at the same object key path. `set --data` replaces nested
/// objects wholesale, so without this an object taken from a stripped
/// export would erase the credentials nested inside it. Arrays are not
/// matched by position: an element's index is not its identity.
fn carry_secrets(current: &serde_json::Value, incoming: &mut serde_json::Value) {
    let (serde_json::Value::Object(current), serde_json::Value::Object(incoming)) =
        (current, incoming)
    else {
        return;
    };
    for (key, value) in current {
        if let Some(next) = incoming.get_mut(key) {
            carry_secrets(value, next);
        } else if is_secret_field(key) {
            incoming.insert(key.clone(), value.clone());
        }
    }
}

/// Gather every secret field under `value` as (path, key, value).
fn collect_secrets<'a>(
    value: &'a serde_json::Value,
    path: &str,
    found: &mut Vec<(String, &'a str, &'a serde_json::Value)>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, field) in map {
                let child = format!("{path}.{key}");
                if is_secret_field(key) {
                    found.push((child, key.as_str(), field));
                } else {
                    collect_secrets(field, &child, found);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_secrets(item, &format!("{path}[{index}]"), found);
            }
        }
        _ => {}
    }
}

/// Collect the paths of secret fields present in `current` that `merged`
/// no longer has, such as one inside an array element the update replaced.
fn dropped_secrets(
    current: &serde_json::Value,
    merged: Option<&serde_json::Value>,
    path: &str,
    dropped: &mut Vec<String>,
) {
    match current {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                let next = merged.and_then(|m| m.get(key));
                if is_secret_field(key) {
                    if next.is_none() {
                        dropped.push(child);
                    }
                } else {
                    dropped_secrets(value, next, &child, dropped);
                }
            }
        }
        // Positions are not identities: entries can move, and a neighbor can
        // be removed. A credential inside an array counts as kept when the
        // same field still holds the same value anywhere in the update.
        serde_json::Value::Array(_) => {
            let mut kept = Vec::new();
            if let Some(merged) = merged {
                collect_secrets(merged, path, &mut kept);
            }
            let mut live = Vec::new();
            collect_secrets(current, path, &mut live);
            for (live_path, key, value) in live {
                if !kept.iter().any(|(_, k, v)| *k == key && *v == value) {
                    dropped.push(live_path);
                }
            }
        }
        _ => {}
    }
}

/// Fill in secrets the update left out, then refuse it if any live
/// credential would still be erased. A credential is only removed when the
/// update names it explicitly (for example by setting it to `""`).
fn guard_secrets(
    current: &serde_json::Value,
    merged: &mut serde_json::Value,
) -> Result<(), CliError> {
    carry_secrets(current, merged);
    let mut dropped = Vec::new();
    dropped_secrets(current, Some(merged), "", &mut dropped);
    if dropped.is_empty() {
        return Ok(());
    }
    Err(CliError::Validation {
        field: "data".into(),
        reason: format!(
            "this update would erase credential field(s) {}; include them \
             (export with --with-secrets), set them explicitly, or pass \
             --allow-secret-removal if removing them is intended",
            dropped.join(", ")
        ),
    })
}

fn section_detail(section: &serde_json::Value, mask_sensitive: bool) -> String {
    let Some(obj) = section.as_object() else {
        return section.to_string();
    };

    let max_key_len = obj
        .keys()
        .filter(|k| !matches!(k.as_str(), "_id" | "site_id"))
        .map(String::len)
        .max()
        .unwrap_or(0);

    let mut lines = Vec::new();
    for (k, v) in obj {
        if matches!(k.as_str(), "_id" | "site_id") {
            continue;
        }
        let display = if mask_sensitive && is_secret_field(k) {
            "***".to_owned()
        } else if v.is_object() || v.is_array() {
            let mut shown = v.clone();
            if mask_sensitive {
                mask_secrets(&mut shown);
            }
            serde_json::to_string_pretty(&shown).unwrap_or_else(|_| shown.to_string())
        } else {
            match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            }
        };
        lines.push(format!("{k:<max_key_len$}  {display}"));
    }
    lines.join("\n")
}

pub async fn handle(
    controller: &Controller,
    args: SettingsArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    let p = output::Painter::new(global);

    match args.command {
        SettingsCommand::List { with_secrets } => {
            let mut sections = controller.get_all_site_settings().await?;
            // The table summary never shows secret values; machine formats
            // serialize whole sections, so they drop secrets unless asked.
            if !with_secrets && !matches!(global.output, OutputFormat::Table) {
                for section in &mut sections {
                    strip_secrets(section);
                }
            }
            let out = output::render_list(
                &global.output,
                &sections,
                |s| section_row(s, &p),
                |s| {
                    s.get("key")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_owned()
                },
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        SettingsCommand::Get { key, with_secrets } => {
            let mut section = controller.get_site_setting(&key).await?;
            let table = matches!(global.output, OutputFormat::Table);
            // The table keeps secret rows visible as `***` so their presence
            // shows; machine formats drop them (see `strip_secrets`).
            if !with_secrets && !table {
                strip_secrets(&mut section);
            }
            let mask = table && !with_secrets;
            let out = output::render_single(
                &global.output,
                &section,
                |s| section_detail(s, mask),
                |s| {
                    s.get("key")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_owned()
                },
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        SettingsCommand::Set(set_args) => handle_set(controller, set_args, global).await,

        SettingsCommand::Export { with_secrets } => {
            let mut sections = controller.get_all_site_settings().await?;
            if !with_secrets {
                let removed: usize = sections.iter_mut().map(strip_secrets).sum();
                if removed > 0 && !global.quiet {
                    eprintln!(
                        "Left out {removed} credential field(s) (x_*); pass --with-secrets to include them"
                    );
                }
            }
            let out = output::render_json_pretty(&sections);
            output::print_output(&out, global.quiet);
            Ok(())
        }
    }
}

async fn handle_set(
    controller: &Controller,
    args: SettingsSetArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    let current = controller.get_site_setting(&args.key).await?;
    let Some(mut obj) = current.as_object().cloned() else {
        return Err(CliError::Validation {
            field: "key".into(),
            reason: format!("setting '{}' is not a JSON object", args.key),
        });
    };

    if let Some(data) = args.data {
        let patch: serde_json::Value = serde_json::from_str(&data)?;
        let Some(patch_obj) = patch.as_object() else {
            return Err(CliError::Validation {
                field: "data".into(),
                reason: "expected a JSON object".into(),
            });
        };
        for (k, v) in patch_obj {
            obj.insert(k.clone(), v.clone());
        }
    } else if let (Some(field), Some(raw_value)) = (args.field, args.value) {
        let parsed = parse_value_literal(&raw_value);
        obj.insert(field, parsed);
    } else {
        return Err(CliError::Validation {
            field: "set".into(),
            reason: "provide either <FIELD> <VALUE> or --data".into(),
        });
    }

    obj.remove("_id");
    obj.remove("site_id");
    obj.remove("key");

    let mut update = serde_json::Value::Object(obj);
    if !args.allow_secret_removal {
        guard_secrets(&current, &mut update)?;
    }

    controller.update_site_setting(&args.key, &update).await?;

    if !global.quiet {
        eprintln!("Setting '{}' updated", args.key);
    }
    Ok(())
}

fn parse_value_literal(raw: &str) -> serde_json::Value {
    match raw {
        "true" => serde_json::Value::Bool(true),
        "false" => serde_json::Value::Bool(false),
        "null" => serde_json::Value::Null,
        _ => {
            if let Ok(n) = raw.parse::<i64>() {
                serde_json::Value::Number(n.into())
            } else if let Ok(f) = raw.parse::<f64>() {
                serde_json::Number::from_f64(f).map_or_else(
                    || serde_json::Value::String(raw.to_owned()),
                    serde_json::Value::Number,
                )
            } else {
                serde_json::Value::String(raw.to_owned())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_secrets_removes_credential_fields_at_any_depth() {
        let mut section = serde_json::json!({
            "key": "radius",
            "enabled": true,
            "x_secret": "hunter2",
            "servers": [{ "ip": "10.0.0.5", "x_shared_secret": "s3cr3t" }],
            "nested": { "x_password": "pw", "port": 1812 }
        });

        assert_eq!(strip_secrets(&mut section), 3);
        assert_eq!(
            section,
            serde_json::json!({
                "key": "radius",
                "enabled": true,
                "servers": [{ "ip": "10.0.0.5" }],
                "nested": { "port": 1812 }
            })
        );
    }

    #[test]
    fn strip_secrets_leaves_secretless_sections_alone() {
        let mut section = serde_json::json!({ "key": "dpi", "enabled": false });
        let original = section.clone();
        assert_eq!(strip_secrets(&mut section), 0);
        assert_eq!(section, original);
    }

    #[test]
    fn parse_value_literal_bool() {
        assert_eq!(parse_value_literal("true"), serde_json::Value::Bool(true));
        assert_eq!(parse_value_literal("false"), serde_json::Value::Bool(false));
    }

    #[test]
    fn parse_value_literal_number() {
        assert_eq!(parse_value_literal("42"), serde_json::json!(42));
        assert_eq!(parse_value_literal("-7"), serde_json::json!(-7));
        assert_eq!(parse_value_literal("2.5"), serde_json::json!(2.5));
    }

    #[test]
    fn parse_value_literal_null() {
        assert_eq!(parse_value_literal("null"), serde_json::Value::Null);
    }

    #[test]
    fn parse_value_literal_string_fallback() {
        assert_eq!(
            parse_value_literal("hello"),
            serde_json::Value::String("hello".into())
        );
        assert_eq!(
            parse_value_literal("0.ubnt.pool.ntp.org"),
            serde_json::Value::String("0.ubnt.pool.ntp.org".into())
        );
    }

    #[test]
    fn notable_summary_skips_meta_and_secrets() {
        let obj: serde_json::Value = serde_json::json!({
            "_id": "abc",
            "key": "mgmt",
            "site_id": "def",
            "enabled": true,
            "x_ssh_password": "secret",
            "led_enabled": true,
            "auto_upgrade": true
        });
        let summary = build_notable_summary(obj.as_object());
        assert!(!summary.contains("_id"));
        assert!(!summary.contains("x_ssh"));
        assert!(!summary.starts_with("enabled="));
        assert!(summary.contains("led_enabled"));
    }

    #[test]
    fn section_detail_masks_sensitive() {
        let section = serde_json::json!({
            "key": "mgmt",
            "x_ssh_password": "secret",
            "led_enabled": true
        });
        let detail = section_detail(&section, true);
        assert!(detail.contains("***"));
        assert!(!detail.contains("secret"));
        assert!(detail.contains("led_enabled"));
    }

    #[test]
    fn notable_truncates_long_strings_safely() {
        let obj: serde_json::Value = serde_json::json!({
            "key": "test",
            "long_field": "this is a string that is definitely longer than twenty four characters"
        });
        let summary = build_notable_summary(obj.as_object());
        assert!(summary.contains("..."));
        assert!(summary.len() < 60);
    }

    #[test]
    fn section_detail_masks_nested_secrets() {
        let section = serde_json::json!({
            "key": "radius",
            "nested": { "x_password": "hunter2", "port": 1812 },
            "servers": [{ "ip": "10.0.0.5", "x_shared_secret": "s3cr3t" }]
        });
        let detail = section_detail(&section, true);
        assert!(!detail.contains("hunter2"));
        assert!(!detail.contains("s3cr3t"));
        assert!(detail.contains("1812"));
        assert!(detail.contains("10.0.0.5"));
    }

    #[test]
    fn stripped_export_replays_without_losing_nested_secrets() {
        let current = serde_json::json!({
            "nested": { "x_password": "hunter2", "port": 1812 },
            "x_top": "keep"
        });
        let mut exported = current.clone();
        strip_secrets(&mut exported);
        exported["nested"]["port"] = serde_json::json!(1813);

        let mut update = current.clone();
        update["nested"] = exported["nested"].clone();
        guard_secrets(&current, &mut update).expect("object secrets carry over");

        assert_eq!(update["nested"]["x_password"], "hunter2");
        assert_eq!(update["nested"]["port"], 1813);
        assert_eq!(update["x_top"], "keep");
    }

    #[test]
    fn update_that_drops_an_array_secret_is_refused() {
        let current = serde_json::json!({
            "servers": [{ "ip": "10.0.0.5", "x_shared_secret": "s3cr3t" }]
        });
        let mut update = serde_json::json!({ "servers": [{ "ip": "10.0.0.6" }] });

        let error = guard_secrets(&current, &mut update).expect_err("array secret would be lost");
        assert!(
            error.to_string().contains("servers[0].x_shared_secret"),
            "{error}"
        );
    }

    #[test]
    fn reordering_array_entries_keeps_their_secrets() {
        let current = serde_json::json!({
            "servers": [{ "ip": "A", "x_shared_secret": "s3cr3t" }, { "ip": "B" }]
        });
        let mut reordered = serde_json::json!({
            "servers": [{ "ip": "B" }, { "ip": "A", "x_shared_secret": "s3cr3t" }]
        });
        guard_secrets(&current, &mut reordered).expect("the secret moved, not vanished");

        let mut trimmed = serde_json::json!({
            "servers": [{ "ip": "A", "x_shared_secret": "s3cr3t" }]
        });
        guard_secrets(&current, &mut trimmed).expect("removing a secretless entry is fine");
    }

    #[test]
    fn explicitly_cleared_secret_is_allowed() {
        let current = serde_json::json!({ "nested": { "x_password": "hunter2" } });
        let mut update = serde_json::json!({ "nested": { "x_password": "" } });
        guard_secrets(&current, &mut update).expect("an explicit value is intentional");
        assert_eq!(update["nested"]["x_password"], "");
    }

    #[test]
    fn section_detail_shows_all_when_unmasked() {
        let section = serde_json::json!({
            "key": "mgmt",
            "x_ssh_password": "secret",
            "led_enabled": true
        });
        let detail = section_detail(&section, false);
        assert!(detail.contains("secret"));
    }
}
