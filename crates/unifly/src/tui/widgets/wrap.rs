//! Word wrapping for text the TUI sizes a box around: the same lines are
//! counted and drawn, so the box always fits them.

use ratatui::text::Span;

fn width_of(text: &str) -> usize {
    Span::raw(text).width()
}

/// `text` broken into lines of at most `width` columns, at spaces where it
/// can. A word longer than a line is split.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let mut word = word;
            loop {
                let gap = usize::from(!line.is_empty());
                if width_of(&line) + gap + width_of(word) <= width {
                    if gap == 1 {
                        line.push(' ');
                    }
                    line.push_str(word);
                    break;
                }
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                    continue;
                }
                // The word alone overflows an empty line: take what fits.
                let mut used = 0;
                let split = word
                    .char_indices()
                    .find(|(_, ch)| {
                        used += width_of(ch.encode_utf8(&mut [0; 4]));
                        used > width
                    })
                    .map_or(word.len(), |(at, _)| at)
                    .max(word.chars().next().map_or(1, char::len_utf8));
                lines.push(word[..split].to_owned());
                word = &word[split..];
                if word.is_empty() {
                    break;
                }
            }
        }
        lines.push(line);
    }
    while lines.len() > 1 && lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// [`wrap`] cut to `max_lines`, with an ellipsis closing a cut line.
pub fn wrap_clamped(text: &str, width: usize, max_lines: usize) -> Vec<String> {
    let mut lines = wrap(text, width);
    if lines.len() > max_lines.max(1) {
        lines.truncate(max_lines.max(1));
        if let Some(last) = lines.last_mut() {
            while width_of(last) + 1 > width.max(1) && last.pop().is_some() {}
            last.push('\u{2026}');
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breaks_at_spaces_and_keeps_every_word() {
        let lines = wrap(
            "Operation not supported: UpdateNetwork (requires Integration API)",
            24,
        );
        assert_eq!(
            lines,
            [
                "Operation not supported:",
                "UpdateNetwork (requires",
                "Integration API)"
            ]
        );
        assert!(lines.iter().all(|line| width_of(line) <= 24));
    }

    #[test]
    fn splits_a_word_longer_than_a_line() {
        let lines = wrap("id 0123456789abcdef0123 end", 8);
        assert_eq!(lines, ["id", "01234567", "89abcdef", "0123 end"]);
    }

    #[test]
    fn short_text_and_empty_text_are_one_line() {
        assert_eq!(wrap("ok", 20), ["ok"]);
        assert_eq!(wrap("", 20), [""]);
        assert_eq!(wrap("a\nb", 20), ["a", "b"]);
    }

    #[test]
    fn a_clamped_wrap_marks_what_it_cut() {
        let lines = wrap_clamped("one two three four five six", 9, 2);
        assert_eq!(lines, ["one two", "three\u{2026}"]);
        assert_eq!(wrap_clamped("short", 9, 2), ["short"]);
    }

    #[test]
    fn wide_characters_count_two_columns() {
        let lines = wrap("日本語 日本語", 6);
        assert_eq!(lines, ["日本語", "日本語"]);
    }
}
