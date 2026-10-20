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
/// serde_yaml_ng emits YAML 1.2. Readers that still resolve plain scalars
/// by YAML 1.1 rules, PyYAML among them, would read some strings it leaves
/// bare as other types, so every string value, key, and sequence item that
/// a YAML 1.1 resolver would not read back as the same string comes out
/// single-quoted:
///
/// - booleans, including `y`/`n`, `yes`/`no`, and `on`/`off` in each case
/// - nulls: `~`, `null` in each case, and the empty string
/// - integers in any base, with underscores, or in base 60 (`1_000`,
///   `0b101`, `017`, `0x1F`, `190:20:30`)
/// - floats with underscores, in base 60, or spelled `.inf` and `.nan`
/// - timestamps such as `2024-01-01` or `2001-12-14 21:59:43.10 -5`
/// - the merge key `<<` and the value key `=`
/// - anything else that contains a colon
///
/// Real numbers, booleans, and nulls stay bare. Multi-line strings stay
/// block scalars, which no resolver types, and strings that a single-quoted
/// scalar cannot carry verbatim (other control characters, U+2028, U+2029,
/// U+FEFF, U+FFFE, U+FFFF) keep the emitter's own quoting. When the emitted
/// text cannot be read back, as with a flattened map that repeats a key,
/// the output is the plain emission without this quoting.
///
/// The emitter picks scalar styles itself, so quoted strings go in as
/// unique plain placeholders and are swapped for their quoted form
/// afterwards: the choice is made on values, never by re-parsing lines.
pub(crate) fn render_yaml<T: serde::Serialize + ?Sized>(data: &T) -> String {
    let emitted = serde_yaml_ng::to_string(data).expect("serialization should not fail");
    quote_ambiguous_strings(&emitted).unwrap_or(emitted)
}

const PLACEHOLDER_END: char = '_';
const PLACEHOLDER_PAD: char = 'p';

/// Longest key the emitter writes as an implicit `key: value`. A placeholder
/// for a longer quoted key is padded past this so the emitter picks the
/// explicit `? key` form, which has no length limit.
const MAX_IMPLICIT_KEY_LEN: usize = 128;

/// Plain scalars a YAML 1.1 reader resolves to a boolean or null.
const YAML_1_1_BOOL_AND_NULL_WORDS: &[&str] = &[
    "y", "Y", "yes", "Yes", "YES", "n", "N", "no", "No", "NO", "true", "True", "TRUE", "false",
    "False", "FALSE", "on", "On", "ON", "off", "Off", "OFF", "~", "null", "Null", "NULL", "",
];

/// PyYAML's YAML 1.1 implicit resolvers for int, float, timestamp, merge,
/// and value, each anchored to the whole scalar.
static YAML_1_1_TYPED_PATTERNS: std::sync::LazyLock<Result<regex::RegexSet, regex::Error>> =
    std::sync::LazyLock::new(|| {
        regex::RegexSet::new([
            r"^(?:[-+]?0b[0-1_]+|[-+]?0[0-7_]+|[-+]?(?:0|[1-9][0-9_]*)|[-+]?0x[0-9a-fA-F_]+|[-+]?[1-9][0-9_]*(?::[0-5]?[0-9])+)$",
            r"^(?:[-+]?[0-9][0-9_]*\.[0-9_]*(?:[eE][-+][0-9]+)?|\.[0-9][0-9_]*(?:[eE][-+][0-9]+)?|[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*|[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN))$",
            r"^(?:[0-9]{4}-[0-9]{2}-[0-9]{2}|[0-9]{4}-[0-9]{1,2}-[0-9]{1,2}(?:[Tt]|[ \t]+)[0-9]{1,2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]*)?(?:[ \t]*(?:Z|[-+][0-9]{1,2}(?::[0-9]{2})?))?)$",
            r"^<<$",
            r"^=$",
        ])
    });

/// A string swapped out of the value tree, with the padding its placeholder
/// carries.
struct Stashed {
    text: String,
    pad: usize,
}

/// Re-emit `emitted` with ambiguous strings quoted, working from the text
/// the emitter wrote: `to_value` would widen an f32 such as 2.4 into
/// 2.4000000953674316, while the emitter writes it short. Returns `None`
/// when the text cannot be read back, as with a flattened map that repeats
/// a typed field's key, and the caller keeps the plain emission.
fn quote_ambiguous_strings(emitted: &str) -> Option<String> {
    let mut value: serde_yaml_ng::Value = serde_yaml_ng::from_str(emitted).ok()?;
    let marker = placeholder_marker(&value);
    let mut stashed = Vec::new();
    stash_ambiguous_strings(&mut value, &marker, &mut stashed, false);
    let yaml = serde_yaml_ng::to_string(&value).ok()?;
    Some(restore_quoted(&yaml, &marker, &stashed))
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

/// Whether a single-quoted scalar carries `text` verbatim: no line breaks
/// to fold and no characters a quoted scalar may not hold. Tabs are fine.
fn single_quotable(text: &str) -> bool {
    text.chars().all(|c| {
        c == '\t'
            || !(c.is_control()
                || matches!(
                    c,
                    '\u{2028}' | '\u{2029}' | '\u{FEFF}' | '\u{FFFE}' | '\u{FFFF}'
                ))
    })
}

/// Whether a YAML 1.1 resolver would read plain `text` as anything but this
/// string, or the colon rule asks for quotes anyway.
fn wants_single_quotes(text: &str) -> bool {
    single_quotable(text)
        && (YAML_1_1_BOOL_AND_NULL_WORDS.contains(&text)
            || text.contains(':')
            // A set that failed to build quotes everything it would have
            // checked rather than let a retyped string through.
            || YAML_1_1_TYPED_PATTERNS
                .as_ref()
                .map_or(true, |patterns| patterns.is_match(text)))
}

fn stash_ambiguous_strings(
    value: &mut serde_yaml_ng::Value,
    marker: &str,
    stashed: &mut Vec<Stashed>,
    is_key: bool,
) {
    use serde_yaml_ng::Value;
    match value {
        Value::String(text) if wants_single_quotes(text) => {
            let mut placeholder = format!("{marker}{}{PLACEHOLDER_END}", stashed.len());
            let quoted_len = text.len() + 2 + text.matches('\'').count();
            let pad = if is_key && quoted_len > MAX_IMPLICIT_KEY_LEN {
                (MAX_IMPLICIT_KEY_LEN + 1).saturating_sub(placeholder.len())
            } else {
                0
            };
            placeholder.extend(std::iter::repeat_n(PLACEHOLDER_PAD, pad));
            let text = std::mem::replace(text, placeholder);
            stashed.push(Stashed { text, pad });
        }
        Value::Sequence(items) => {
            for item in items {
                stash_ambiguous_strings(item, marker, stashed, false);
            }
        }
        Value::Mapping(map) => {
            let entries = std::mem::take(map);
            for (mut key, mut item) in entries {
                stash_ambiguous_strings(&mut key, marker, stashed, true);
                stash_ambiguous_strings(&mut item, marker, stashed, false);
                map.insert(key, item);
            }
        }
        Value::Tagged(tagged) => stash_ambiguous_strings(&mut tagged.value, marker, stashed, false),
        Value::String(_) | Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn restore_quoted(yaml: &str, marker: &str, stashed: &[Stashed]) -> String {
    let mut out = String::with_capacity(yaml.len());
    let mut rest = yaml;
    while let Some(start) = rest.find(marker) {
        out.push_str(&rest[..start]);
        let after = &rest[start + marker.len()..];
        let (digits, tail) = after.split_at(after.bytes().take_while(u8::is_ascii_digit).count());
        let entry = digits.parse::<usize>().ok().and_then(|i| stashed.get(i));
        let tail = tail.strip_prefix(PLACEHOLDER_END).and_then(|tail| {
            let pad = entry.map_or(0, |entry| entry.pad);
            let padding = tail.get(..pad)?;
            padding
                .chars()
                .all(|c| c == PLACEHOLDER_PAD)
                .then(|| &tail[pad..])
        });
        if let (Some(entry), Some(tail)) = (entry, tail) {
            out.push('\'');
            out.push_str(&entry.text.replace('\'', "''"));
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
    fn yaml_quotes_yaml_1_1_boolean_and_null_words() {
        let yaml = round_trip(&json!({
            "poe": "off",
            "modes": ["on", "Yes", "n", "NULL", "~", "auto"],
            "Off": "kept",
            "enabled": false,
            "uplink": null,
            "label": "",
        }));
        for expected in [
            "poe: 'off'",
            "- 'on'",
            "- 'Yes'",
            "- 'n'",
            "- 'NULL'",
            "- '~'",
            "- auto",
            "'Off': kept",
            "enabled: false",
            "uplink: null",
            "label: ''",
        ] {
            assert!(yaml.contains(expected), "missing {expected:?} in:\n{yaml}");
        }
    }

    #[test]
    fn yaml_falls_back_to_plain_emission_when_it_cannot_reread_itself() {
        // A flattened map can repeat a typed field's key; the emitter writes
        // that, but the parser rejects it, so quoting must step aside.
        #[derive(serde::Serialize)]
        struct Repeats {
            id: u8,
            #[serde(flatten)]
            extra: serde_json::Map<String, serde_json::Value>,
        }
        let data = Repeats {
            id: 1,
            extra: serde_json::from_value(json!({ "id": 2, "when": "12:30" })).expect("object"),
        };
        let plain = serde_yaml_ng::to_string(&data).expect("emits");
        assert_eq!(render_yaml(&data), plain);
    }

    #[test]
    fn yaml_quotes_every_string_a_yaml_1_1_resolver_would_retype() {
        let retyped = [
            "1_000",
            "0b101",
            "017",
            "0x1F",
            "-0x_1f",
            "+12",
            "190:20:30",
            "1_000.5",
            ".5",
            "1.",
            "-1.5e+3",
            "190:20:30.15",
            "+.inf",
            ".NaN",
            "2024-01-01",
            "2001-12-14t21:59:43.10-05:00",
            "2001-12-14 21:59:43.10 -5",
            "2001-12-14\t21:59:43",
            "=",
            "<<",
            "yes",
            "Off",
            "y",
            "N",
            "~",
            "Null",
        ];
        let value = json!({ "values": retyped, "keys": { "1_000": 1, "2024-01-01": 2, "<<": 3 } });
        let yaml = round_trip(&value);
        for text in retyped {
            assert!(
                yaml.contains(&format!("- '{text}'")),
                "{text:?} bare in:\n{yaml}"
            );
        }
        for key in ["'1_000': 1", "'2024-01-01': 2", "'<<': 3"] {
            assert!(yaml.contains(key), "{key:?} missing in:\n{yaml}");
        }
    }

    #[test]
    fn yaml_leaves_strings_yaml_1_1_reads_as_strings_bare() {
        let plain = [
            "auto", "1a", "1.2.3", "2024-1-1", "==", "<<<", "_1", "0b", "x_1",
        ];
        let yaml = round_trip(&json!(plain));
        for text in plain {
            assert!(
                yaml.contains(&format!("- {text}\n")),
                "{text:?} quoted in:\n{yaml}"
            );
        }
    }

    #[test]
    fn yaml_keeps_real_scalars_bare() {
        let yaml = round_trip(&json!({ "num": 1000, "f": 1.5, "b": true, "z": null, "neg": -3 }));
        assert_eq!(yaml, "b: true\nf: 1.5\nneg: -3\nnum: 1000\nz: null\n");
    }

    #[test]
    fn yaml_quotes_long_keys_in_explicit_form() {
        let key = format!("1{}", ":30".repeat(60));
        let yaml = round_trip(&json!({ key.clone(): "v", "after": "x:y" }));
        assert!(yaml.contains(&format!("? '{key}'")), "{yaml}");
    }

    #[test]
    fn yaml_1_1_patterns_compile() {
        assert!(super::YAML_1_1_TYPED_PATTERNS.is_ok());
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
