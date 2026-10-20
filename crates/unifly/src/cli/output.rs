//! Output formatting: table, JSON, YAML, plain.
//!
//! Renders data in the format selected by `--output`. Table uses `tabled`,
//! structured formats use serde, plain emits one identifier per line.
//! Color output uses opaline's SilkCircuit theme via owo-colors.

use std::io::{self, IsTerminal, Write};

use opaline::adapters::owo_colors::OwoThemeExt;
use owo_colors::OwoColorize;
use tabled::{Table, Tabled, settings::Style};

use crate::cli::args::{ColorMode, OutputFormat};

// ── Color helpers (SilkCircuit palette via opaline) ──────────────────

/// Determine whether color output should be enabled.
pub fn should_color(mode: &ColorMode) -> bool {
    match mode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => io::stdout().is_terminal() && std::env::var("NO_COLOR").is_err(),
    }
}

/// Load the CLI color theme.
///
/// Resolution: `--theme` flag / `UNIFLY_THEME` env → config `defaults.theme`
/// → `silkcircuit-neon` fallback. Matches the TUI resolution order so both
/// surfaces stay in sync.
pub fn load_theme(name: Option<&str>) -> opaline::Theme {
    let config_theme = name
        .is_none()
        .then(|| {
            crate::config::load_config()
                .ok()
                .and_then(|c| c.defaults.theme)
        })
        .flatten();
    builtin_theme(name.or(config_theme.as_deref()))
}

/// Resolve a theme by name without consulting the config file.
fn builtin_theme(name: Option<&str>) -> opaline::Theme {
    opaline::load_by_name(name.unwrap_or("silkcircuit-neon")).unwrap_or_else(|| {
        opaline::load_by_name("silkcircuit-neon").expect("builtin theme must exist")
    })
}

/// Apply theme coloring to a value based on its semantic role.
pub fn themed(theme: &opaline::Theme, text: &str, token: &str) -> String {
    format!("{}", text.style(theme.owo_fg(token)))
}

// ── Semantic color helpers ───────────────────────────────────────────
//
// Consistent color vocabulary across all CLI commands. Each helper
// returns the input unchanged if color is disabled.

/// Colorizer that holds theme + color state to avoid reloading per field.
pub struct Painter {
    theme: opaline::Theme,
    enabled: bool,
}

impl Painter {
    /// Create a painter from global options.
    pub fn new(global: &super::args::GlobalOpts) -> Self {
        let enabled = should_color(&global.color)
            && matches!(global.output, super::args::OutputFormat::Table);
        // The demo fleet never opens the config file, which holds profiles
        // and possibly plaintext secrets: its theme comes only from
        // --theme / UNIFLY_THEME or the builtin default.
        let theme = if global.demo_fleet {
            builtin_theme(global.theme.as_deref())
        } else {
            load_theme(global.theme.as_deref())
        };
        Self { theme, enabled }
    }

    /// Create a disabled (no-color) painter. Colors are never drawn, so it
    /// does not read the config for a theme.
    pub fn plain() -> Self {
        Self {
            theme: builtin_theme(None),
            enabled: false,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn paint(&self, text: &str, token: &str) -> String {
        if self.enabled {
            themed(&self.theme, text, token)
        } else {
            text.to_string()
        }
    }

    // ── Semantic methods ────────────────────────────────────

    /// Names, labels, identifiers (neon cyan)
    pub fn name(&self, text: &str) -> String {
        self.paint(text, "accent.secondary")
    }

    /// IP addresses, subnets (coral)
    pub fn ip(&self, text: &str) -> String {
        self.paint(text, "code.number")
    }

    /// MAC addresses, hardware identifiers (dim)
    pub fn mac(&self, text: &str) -> String {
        self.paint(text, "text.dim")
    }

    /// Types, categories, models (muted)
    pub fn muted(&self, text: &str) -> String {
        self.paint(text, "text.muted")
    }

    /// UUIDs, IDs (dim)
    pub fn id(&self, text: &str) -> String {
        self.paint(text, "text.dim")
    }

    /// Numeric values, counts, ports (coral)
    pub fn number(&self, text: &str) -> String {
        self.paint(text, "code.number")
    }

    /// Success states, "yes", "enabled", "online" (green)
    pub fn success(&self, text: &str) -> String {
        self.paint(text, "success")
    }

    /// Error states, "no", "blocked", "offline" (red)
    pub fn error(&self, text: &str) -> String {
        self.paint(text, "error")
    }

    /// Warning states (yellow)
    pub fn warning(&self, text: &str) -> String {
        self.paint(text, "warning")
    }

    /// Boolean field: green for true, red for false
    pub fn enabled(&self, val: bool) -> String {
        if val {
            self.success("yes")
        } else {
            self.error("no")
        }
    }

    /// Action: allow=green, block=red, reject=yellow
    pub fn action(&self, text: &str) -> String {
        match text.to_lowercase().as_str() {
            "allow" => self.success(text),
            "block" | "drop" => self.error(text),
            "reject" => self.warning(text),
            _ => self.muted(text),
        }
    }

    /// Device state: online=green, offline=red, other=yellow
    pub fn state(&self, text: &str) -> String {
        match text.to_lowercase().as_str() {
            "online" | "connected" => self.success(text),
            "offline" | "disconnected" => self.error(text),
            _ => self.warning(text),
        }
    }

    /// Health status: ok=green, warning=yellow, error=red
    pub fn health(&self, text: &str) -> String {
        match text.to_lowercase().as_str() {
            "ok" | "healthy" => self.success(text),
            "warning" => self.warning(text),
            _ => self.error(text),
        }
    }

    /// Keyword emphasis (electric purple, bold)
    pub fn keyword(&self, text: &str) -> String {
        if self.enabled {
            format!("{}", text.style(self.theme.owo_style("keyword")))
        } else {
            text.to_string()
        }
    }
}

// ── Render dispatchers ───────────────────────────────────────────────

/// Render a list of serde-serializable + tabled items in the chosen format.
///
/// - `table`: uses the `Tabled` derive to build a pretty table
/// - `json` / `json-compact`: serializes the original data via serde
/// - `yaml`: serializes via serde_yaml_ng
/// - `plain`: calls `id_fn` on each item to emit one identifier per line
pub fn render_list<T, R>(
    format: &OutputFormat,
    data: &[T],
    to_row: impl Fn(&T) -> R,
    id_fn: impl Fn(&T) -> String,
) -> String
where
    T: serde::Serialize,
    R: Tabled,
{
    match format {
        OutputFormat::Table => {
            let rows: Vec<R> = data.iter().map(to_row).collect();
            render_table(&rows)
        }
        OutputFormat::Json => render_json(data, false),
        OutputFormat::JsonCompact => render_json(data, true),
        OutputFormat::Yaml => render_yaml(data),
        OutputFormat::Plain => data.iter().map(&id_fn).collect::<Vec<_>>().join("\n"),
    }
}

/// Render a single serde-serializable item in the chosen format.
///
/// Table rendering uses a custom `detail_fn` that returns a pre-formatted string,
/// since single-item detail views don't use `Tabled` derive.
pub fn render_single<T>(
    format: &OutputFormat,
    data: &T,
    detail_fn: impl Fn(&T) -> String,
    id_fn: impl Fn(&T) -> String,
) -> String
where
    T: serde::Serialize,
{
    match format {
        OutputFormat::Table => detail_fn(data),
        OutputFormat::Json => render_json(data, false),
        OutputFormat::JsonCompact => render_json(data, true),
        OutputFormat::Yaml => render_yaml(data),
        OutputFormat::Plain => id_fn(data),
    }
}

/// Warn when a create succeeded on the controller but its response could
/// not be converted for rendering, so stdout stays empty. The mutation is
/// real; failing the command here would invite duplicate-create retries.
pub fn warn_unrenderable_create(quiet: bool) {
    if !quiet {
        eprintln!(
            "Warning: created, but the controller response could not be rendered; use the matching list command to fetch the new entity"
        );
    }
}

/// Print the rendered output to stdout, respecting quiet mode.
pub fn print_output(output: &str, quiet: bool) {
    if quiet || output.is_empty() {
        return;
    }
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{output}");
}

// ── Format-specific renderers ────────────────────────────────────────

fn render_table<R: Tabled>(rows: &[R]) -> String {
    Table::new(rows).with(Style::rounded()).to_string()
}

/// Pretty-printed JSON.
pub(crate) fn render_json_pretty<T: serde::Serialize + ?Sized>(data: &T) -> String {
    serde_json::to_string_pretty(data).expect("serialization should not fail")
}

/// Compact single-line JSON.
pub(crate) fn render_json_compact<T: serde::Serialize + ?Sized>(data: &T) -> String {
    serde_json::to_string(data).expect("serialization should not fail")
}

fn render_json<T: serde::Serialize + ?Sized>(data: &T, compact: bool) -> String {
    if compact {
        render_json_compact(data)
    } else {
        render_json_pretty(data)
    }
}

/// YAML output.
///
/// serde_yaml_ng emits YAML 1.2, where a bare `12:34:56` is a string, but
/// YAML 1.1 readers such as PyYAML read colon-separated digits as a base-60
/// number. Single-line strings that contain a colon therefore come out
/// single-quoted. The emitter picks scalar styles itself, so those strings
/// go in as unique plain placeholders and are swapped for their quoted form
/// afterwards: the choice is made on values, never by re-parsing lines.
pub(crate) fn render_yaml<T: serde::Serialize + ?Sized>(data: &T) -> String {
    yaml_with_quoted_colons(data).expect("serialization should not fail")
}

const PLACEHOLDER_END: char = '_';

/// Longest key worth quoting. The emitter writes longer keys in the explicit
/// `? key` form, which a single-quoted replacement must not turn back into
/// an implicit key past YAML's length limit.
const MAX_QUOTED_KEY_LEN: usize = 128;

fn yaml_with_quoted_colons<T: serde::Serialize + ?Sized>(
    data: &T,
) -> Result<String, serde_yaml_ng::Error> {
    // Emit first and read the text back: `to_value` would widen an f32 such
    // as 2.4 into 2.4000000953674316, while the emitter writes it short.
    let mut value: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&serde_yaml_ng::to_string(data)?)?;
    let marker = placeholder_marker(&value);
    let mut originals = Vec::new();
    stash_colon_strings(&mut value, &marker, &mut originals);
    let yaml = serde_yaml_ng::to_string(&value)?;
    Ok(restore_quoted(&yaml, &marker, &originals))
}

/// A placeholder prefix that no key or string in `value` contains, so every
/// occurrence in the emitted text is one this module inserted.
fn placeholder_marker(value: &serde_yaml_ng::Value) -> String {
    let mut attempt = 0_u32;
    loop {
        let marker = format!("uniflyq{attempt}x");
        if !contains_text(value, &marker) {
            return marker;
        }
        attempt += 1;
    }
}

fn contains_text(value: &serde_yaml_ng::Value, needle: &str) -> bool {
    use serde_yaml_ng::Value;
    match value {
        Value::String(text) => text.contains(needle),
        Value::Sequence(items) => items.iter().any(|item| contains_text(item, needle)),
        Value::Mapping(map) => map
            .iter()
            .any(|(key, item)| contains_text(key, needle) || contains_text(item, needle)),
        Value::Tagged(tagged) => contains_text(&tagged.value, needle),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

/// Whether `text` should be emitted single-quoted. Multi-line strings stay
/// with the emitter, which writes them as block scalars that no reader
/// types, and so do strings a single-quoted scalar cannot carry verbatim.
fn wants_single_quotes(text: &str) -> bool {
    text.contains(':')
        && text.chars().all(|c| {
            !c.is_control()
                && !matches!(
                    c,
                    '\u{2028}' | '\u{2029}' | '\u{FEFF}' | '\u{FFFE}' | '\u{FFFF}'
                )
        })
}

fn stash_colon_strings(
    value: &mut serde_yaml_ng::Value,
    marker: &str,
    originals: &mut Vec<String>,
) {
    use serde_yaml_ng::Value;
    match value {
        Value::String(text) if wants_single_quotes(text) => {
            let placeholder = format!("{marker}{}{PLACEHOLDER_END}", originals.len());
            originals.push(std::mem::replace(text, placeholder));
        }
        Value::Sequence(items) => {
            for item in items {
                stash_colon_strings(item, marker, originals);
            }
        }
        Value::Mapping(map) => {
            let entries = std::mem::take(map);
            for (mut key, mut item) in entries {
                if key
                    .as_str()
                    .is_none_or(|text| text.len() <= MAX_QUOTED_KEY_LEN)
                {
                    stash_colon_strings(&mut key, marker, originals);
                }
                stash_colon_strings(&mut item, marker, originals);
                map.insert(key, item);
            }
        }
        Value::Tagged(tagged) => stash_colon_strings(&mut tagged.value, marker, originals),
        Value::String(_) | Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn restore_quoted(yaml: &str, marker: &str, originals: &[String]) -> String {
    let mut out = String::with_capacity(yaml.len());
    let mut rest = yaml;
    while let Some(start) = rest.find(marker) {
        out.push_str(&rest[..start]);
        let after = &rest[start + marker.len()..];
        let (digits, tail) = after.split_at(after.bytes().take_while(u8::is_ascii_digit).count());
        let original = digits.parse::<usize>().ok().and_then(|i| originals.get(i));
        if let (Some(original), Some(tail)) = (original, tail.strip_prefix(PLACEHOLDER_END)) {
            out.push('\'');
            out.push_str(&original.replace('\'', "''"));
            out.push('\'');
            rest = tail;
        } else {
            out.push_str(marker);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::render_yaml;

    fn round_trip(value: &serde_json::Value) -> String {
        let yaml = render_yaml(value);
        let parsed: serde_json::Value = serde_yaml_ng::from_str(&yaml)
            .unwrap_or_else(|err| panic!("invalid YAML ({err}):\n{yaml}"));
        assert_eq!(&parsed, value, "YAML did not round-trip:\n{yaml}");
        yaml
    }

    #[test]
    fn yaml_escapes_apostrophes_in_quoted_colon_values() {
        round_trip(&json!({ "name": "Bob's AP:5G", "nested": [{ "ssid": "o'k:3" }] }));
    }

    #[test]
    fn yaml_leaves_block_scalar_content_alone() {
        round_trip(&json!({ "note": "line one\nkey: x:y\n  indented: a:b\nlast" }));
    }

    #[test]
    fn yaml_handles_keys_the_emitter_quotes() {
        round_trip(&json!({ "quoted key: x": "v:1", "12:30": "noon:ish" }));
    }

    #[test]
    fn yaml_quotes_colon_scalars_for_yaml_1_1_readers() {
        let yaml = round_trip(&json!({
            "mac": "12:34:56:01:02:03",
            "times": ["12:30", "plain"],
            "url": "http://example.com:8080/x",
        }));
        assert!(yaml.contains("mac: '12:34:56:01:02:03'"), "{yaml}");
        assert!(yaml.contains("- '12:30'"), "{yaml}");
        assert!(yaml.contains("- plain"), "{yaml}");
        assert!(yaml.contains("url: 'http://example.com:8080/x'"), "{yaml}");
    }

    #[test]
    fn yaml_placeholders_never_collide_with_data() {
        round_trip(&json!({
            "a": "uniflyq0x0_",
            "b": "uniflyq0x0_:",
            "c": "time 10:00",
            "d": ["uniflyq1x", "x:y"],
        }));
    }

    #[test]
    fn yaml_keeps_single_precision_floats_short() {
        #[derive(serde::Serialize)]
        struct Radio {
            frequency_ghz: f32,
            bands: Vec<f32>,
        }
        let yaml = render_yaml(&Radio {
            frequency_ghz: 2.4,
            bands: vec![5.0, 6.0],
        });
        assert_eq!(yaml, "frequency_ghz: 2.4\nbands:\n- 5.0\n- 6.0\n");
    }

    #[test]
    fn yaml_handles_keys_too_long_to_be_implicit() {
        let key = format!("{}:x", "k".repeat(1021));
        round_trip(&json!({ key: "v:1" }));
    }

    #[test]
    fn yaml_keeps_struct_field_order() {
        #[derive(serde::Serialize)]
        struct Row {
            zeta: &'static str,
            alpha: &'static str,
        }
        let yaml = render_yaml(&Row {
            zeta: "a:b",
            alpha: "c",
        });
        assert_eq!(yaml, "zeta: 'a:b'\nalpha: c\n");
    }
}
