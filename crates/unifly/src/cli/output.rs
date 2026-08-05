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

    let resolved = name
        .or(config_theme.as_deref())
        .unwrap_or("silkcircuit-neon");

    opaline::load_by_name(resolved).unwrap_or_else(|| {
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
        Self {
            theme: load_theme(global.theme.as_deref()),
            enabled,
        }
    }

    /// Create a disabled (no-color) painter.
    pub fn plain() -> Self {
        Self {
            theme: load_theme(None),
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
/// After serialisation, quotes any unquoted scalar values that contain
/// colons so the output is safe for YAML 1.1 parsers (which interpret
/// colon-separated digits as sexagesimal numbers).
pub(crate) fn render_yaml<T: serde::Serialize + ?Sized>(data: &T) -> String {
    let raw = serde_yaml_ng::to_string(data).expect("serialization should not fail");
    quote_yaml_colons(&raw)
}

/// Find unquoted YAML values that contain `:` and wrap them in single quotes.
fn quote_yaml_colons(yaml: &str) -> String {
    let mut out = String::with_capacity(yaml.len());
    for line in yaml.lines() {
        // Only touch "key: value" lines where the value is a bare
        // (unquoted) string containing at least one colon.
        if let Some(colon_pos) = line.find(": ") {
            let value = &line[colon_pos + 2..];
            let needs_quoting =
                value.contains(':') && !value.starts_with('\'') && !value.starts_with('"');
            if needs_quoting {
                out.push_str(&line[..colon_pos + 2]);
                out.push('\'');
                out.push_str(value);
                out.push('\'');
            } else {
                out.push_str(line);
            }
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}
