//! Update known configuration fields without discarding user-authored TOML.

use std::path::Path;

use toml_edit::{DocumentMut, Item, TableLike};

use super::{Config, ConfigError};

pub(super) fn render(path: &Path, config: &Config) -> Result<String, ConfigError> {
    let serialized = toml::to_string_pretty(config)?;
    let original = match std::fs::read_to_string(path) {
        Ok(original) => original,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(serialized),
        Err(error) => return Err(save_error(path, error.to_string())),
    };
    // Loading the existing typed config also rejects invalid known fields before
    // any write. Unknown fields are retained in the editable document below.
    let before = super::parse_config(&original, path)?;
    let before = toml::Table::try_from(before)?;
    let after = toml::Table::try_from(config)?;
    let mut document = original
        .parse::<DocumentMut>()
        .map_err(|_| save_error(path, "existing configuration is not valid TOML".into()))?;
    let fresh = serialized
        .parse::<DocumentMut>()
        .map_err(|_| save_error(path, "serialized configuration is not valid TOML".into()))?;
    patch(document.as_table_mut(), &before, &after, fresh.as_table());
    Ok(document.to_string())
}

fn save_error(path: &Path, reason: String) -> ConfigError {
    ConfigError::Save {
        path: path.to_owned(),
        reason,
    }
}

fn patch(
    target: &mut dyn TableLike,
    before: &toml::Table,
    after: &toml::Table,
    fresh: &dyn TableLike,
) {
    for key in before.keys().filter(|key| !after.contains_key(*key)) {
        target.remove(key);
    }
    for (key, value) in after {
        if before.get(key) == Some(value) {
            continue;
        }
        let Some(replacement) = fresh.get(key) else {
            continue;
        };
        if let (
            Some(toml::Value::Table(old)),
            toml::Value::Table(new),
            Some(existing),
            Some(next),
        ) = (
            before.get(key),
            value,
            target.get_mut(key).and_then(Item::as_table_like_mut),
            replacement.as_table_like(),
        ) {
            patch(existing, old, new, next);
        } else {
            let mut replacement = replacement.clone();
            if let (Some(old), Some(new)) = (
                target.get(key).and_then(Item::as_value),
                replacement.as_value_mut(),
            ) {
                *new.decor_mut() = old.decor().clone();
            }
            if let Some(existing) = target.get_mut(key) {
                *existing = replacement;
            } else {
                target.insert(key, replacement);
            }
        }
    }
}

#[cfg(test)]
mod tests;
