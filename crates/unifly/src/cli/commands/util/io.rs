use std::io::{IsTerminal, Read};
use std::path::Path;

use crate::cli::error::CliError;

pub fn confirm(message: &str, yes_flag: bool) -> Result<bool, CliError> {
    if yes_flag {
        return Ok(true);
    }

    if !std::io::stdin().is_terminal() {
        return Err(CliError::NonInteractiveRequiresYes {
            action: message.into(),
        });
    }

    let confirmed = dialoguer::Confirm::new()
        .with_prompt(message)
        .default(false)
        .interact()
        .map_err(|error| CliError::Io(std::io::Error::other(error)))?;
    Ok(confirmed)
}

pub fn read_json_file(path: &Path) -> Result<serde_json::Value, CliError> {
    let file = std::fs::File::open(path)?;
    let mut reader = json_comments::StripComments::new(std::io::BufReader::new(file));
    let mut text = String::new();
    reader.read_to_string(&mut text)?;
    let cleaned = strip_trailing_commas(&text);
    serde_json::from_str(&cleaned).map_err(|error| CliError::Validation {
        field: "from-file".into(),
        reason: format!("invalid JSON: {error}"),
    })
}

/// Drop trailing commas before `]` / `}` so JSONC files (with comments
/// already stripped upstream) parse cleanly under `serde_json`.
/// String contents are preserved — a comma inside a quoted string
/// followed by `]` is left alone.
///
/// Operates on raw bytes because every meaningful sigil (`"`, `,`, `\\`,
/// `]`, `}`, ASCII whitespace) is single-byte. Multi-byte UTF-8 sequences
/// have the high bit set on every byte, so they never match those sigils
/// and pass through to the output verbatim. The output buffer is a
/// `Vec<u8>` (not a `String`) so we never re-encode bytes individually --
/// the previous implementation used `char::from(byte)` which mangled
/// non-ASCII labels (`"Café"`, emoji, etc.) into mojibake.
fn strip_trailing_commas(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut in_string = false;
    let mut escape = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            out.push(c);
            if escape {
                escape = false;
            } else if c == b'\\' {
                escape = true;
            } else if c == b'"' {
                in_string = false;
            }
            i += 1;
        } else if c == b'"' {
            in_string = true;
            out.push(b'"');
            i += 1;
        } else if c == b',' {
            // Look ahead through whitespace for `]` or `}`.
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && (bytes[j] == b']' || bytes[j] == b'}') {
                // Skip the comma; whitespace and the closer follow.
                i += 1;
            } else {
                out.push(b',');
                i += 1;
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    // Input was valid UTF-8 and we only suppress full ASCII commas, so
    // the resulting bytes are also valid UTF-8.
    String::from_utf8(out).expect("strip_trailing_commas preserves UTF-8 boundaries")
}

#[cfg(test)]
mod tests {
    use super::strip_trailing_commas;

    #[test]
    fn strips_trailing_comma_before_closing_bracket() {
        let s = r#"{"ports": [1, 2, 3,]}"#;
        let want = r#"{"ports": [1, 2, 3]}"#;
        assert_eq!(strip_trailing_commas(s), want);
    }

    #[test]
    fn strips_trailing_comma_before_closing_brace() {
        let s = "{\"a\": 1, \"b\": 2,}";
        assert_eq!(strip_trailing_commas(s), "{\"a\": 1, \"b\": 2}");
    }

    #[test]
    fn strips_through_whitespace_and_newlines() {
        let s = "{\n  \"a\": 1,\n}";
        assert_eq!(strip_trailing_commas(s), "{\n  \"a\": 1\n}");
    }

    #[test]
    fn preserves_commas_inside_strings() {
        // Comma followed by `]` inside a string MUST be preserved.
        let s = r#"{"label": "a,]"}"#;
        assert_eq!(strip_trailing_commas(s), r#"{"label": "a,]"}"#);
    }

    #[test]
    fn preserves_escaped_quotes_inside_strings() {
        let s = r#"{"label": "she said \"hi,\""}"#;
        assert_eq!(strip_trailing_commas(s), r#"{"label": "she said \"hi,\""}"#);
    }

    #[test]
    fn leaves_non_trailing_commas_alone() {
        let s = "[1, 2, 3]";
        assert_eq!(strip_trailing_commas(s), "[1, 2, 3]");
    }

    /// Regression: the previous `char::from(byte)` implementation
    /// rebuilt the output by re-encoding each byte as a single Unicode
    /// scalar, which corrupted multi-byte UTF-8 sequences in port
    /// labels (`"Café"`) or any non-ASCII string content.
    #[test]
    fn preserves_non_ascii_utf8_strings() {
        // Café = 0x43 0x61 0x66 0xC3 0xA9 -- the trailing 0xC3 0xA9
        // pair is é. With trailing commas around it for good measure.
        let s = "{\"label\": \"Café\", \"ports\": [1, 2,]}";
        let want = "{\"label\": \"Café\", \"ports\": [1, 2]}";
        assert_eq!(strip_trailing_commas(s), want);
    }

    #[test]
    fn preserves_emoji_in_strings() {
        let s = "{\"name\": \"ap-living-room 🛋️\",}";
        let want = "{\"name\": \"ap-living-room 🛋️\"}";
        assert_eq!(strip_trailing_commas(s), want);
    }
}
