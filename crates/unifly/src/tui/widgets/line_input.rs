//! A one-line text editor: the text, a cursor, and the keys that move and
//! edit it. Every typed field in the TUI (search, forms, the setup wizard)
//! edits through this, so they all take the same keys.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Style;
use ratatui::text::Span;

/// What a key did to a [`LineInput`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// The key is not an editing key; the caller may use it.
    Ignored,
    /// The cursor moved, or the key was an editing key with nothing to do.
    Moved,
    /// The text changed.
    Changed,
}

impl Edit {
    /// Whether the editor took the key, changed or not.
    pub fn consumed(self) -> bool {
        self != Self::Ignored
    }
}

/// One line of editable text with a cursor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineInput {
    text: String,
    /// Cursor position as a count of characters before it.
    cursor: usize,
}

impl LineInput {
    /// An editor holding `text`, with the cursor after it.
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.chars().count();
        Self { text, cursor }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Replace the text and put the cursor after it.
    pub fn set(&mut self, text: impl Into<String>) {
        *self = Self::new(text);
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// Byte offset of the character at `index`, or the end of the text.
    fn byte_at(&self, index: usize) -> usize {
        self.text
            .char_indices()
            .nth(index)
            .map_or(self.text.len(), |(byte, _)| byte)
    }

    /// Insert one character at the cursor.
    pub fn insert(&mut self, ch: char) {
        let at = self.byte_at(self.cursor);
        self.text.insert(at, ch);
        self.cursor += 1;
    }

    /// Insert pasted text at the cursor. Control characters are dropped, so
    /// a newline in a paste can never submit the field, and only characters
    /// `accept` allows go in.
    pub fn paste(&mut self, text: &str, accept: impl Fn(char) -> bool) -> Edit {
        let before = self.text.len();
        for ch in text.chars().filter(|ch| !ch.is_control() && accept(*ch)) {
            self.insert(ch);
        }
        if self.text.len() == before {
            Edit::Moved
        } else {
            Edit::Changed
        }
    }

    fn delete_range(&mut self, from: usize, to: usize) -> Edit {
        if from >= to {
            return Edit::Moved;
        }
        let (start, end) = (self.byte_at(from), self.byte_at(to));
        self.text.replace_range(start..end, "");
        self.cursor = from;
        Edit::Changed
    }

    /// Where the word before the cursor starts.
    fn word_start(&self) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let mut at = self.cursor.min(chars.len());
        while at > 0 && chars[at - 1].is_whitespace() {
            at -= 1;
        }
        while at > 0 && !chars[at - 1].is_whitespace() {
            at -= 1;
        }
        at
    }

    /// Apply an editing key, accepting any printable character.
    pub fn handle_key(&mut self, key: KeyEvent) -> Edit {
        self.handle_key_accepting(key, |_| true)
    }

    /// Apply an editing key. A typed character goes in only when `accept`
    /// allows it; a refused one is still consumed, so it cannot fall
    /// through to a shortcut.
    pub fn handle_key_accepting(&mut self, key: KeyEvent, accept: impl Fn(char) -> bool) -> Edit {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        // AltGr reports as Ctrl+Alt and types an ordinary character.
        let chord = ctrl != alt;
        let len = self.len();

        match key.code {
            KeyCode::Char(ch) if !chord => {
                if !ch.is_control() && accept(ch) {
                    self.insert(ch);
                    Edit::Changed
                } else {
                    Edit::Moved
                }
            }
            KeyCode::Char('a') if ctrl => self.move_to(0),
            KeyCode::Char('e') if ctrl => self.move_to(len),
            KeyCode::Char('b') if ctrl => self.move_to(self.cursor.saturating_sub(1)),
            KeyCode::Char('f') if ctrl => self.move_to(self.cursor + 1),
            KeyCode::Char('u') if ctrl => self.delete_range(0, self.cursor),
            KeyCode::Char('k') if ctrl => self.delete_range(self.cursor, len),
            // Terminals that send ^H for Backspace report it as Ctrl+h.
            KeyCode::Char('h') if ctrl => {
                self.delete_range(self.cursor.saturating_sub(1), self.cursor)
            }
            KeyCode::Char('w') if ctrl => self.delete_range(self.word_start(), self.cursor),
            KeyCode::Backspace if ctrl || alt => self.delete_range(self.word_start(), self.cursor),
            KeyCode::Backspace => self.delete_range(self.cursor.saturating_sub(1), self.cursor),
            KeyCode::Delete => self.delete_range(self.cursor, (self.cursor + 1).min(len)),
            KeyCode::Left => self.move_to(self.cursor.saturating_sub(1)),
            KeyCode::Right => self.move_to(self.cursor + 1),
            KeyCode::Home => self.move_to(0),
            KeyCode::End => self.move_to(len),
            _ => Edit::Ignored,
        }
    }

    fn move_to(&mut self, cursor: usize) -> Edit {
        self.cursor = cursor.min(self.len());
        Edit::Moved
    }

    /// The spans that draw the field in `width` columns: the stretch of text
    /// around the cursor that fits, with a block cursor when `focused`.
    /// `mask` draws every character as that one, for secrets.
    pub fn spans(
        &self,
        width: usize,
        focused: bool,
        mask: Option<char>,
        style: Style,
    ) -> Vec<Span<'static>> {
        let chars: Vec<char> = match mask {
            Some(dot) => self.text.chars().map(|_| dot).collect(),
            None => self.text.chars().collect(),
        };
        if !focused {
            let shown: String = fit_from(&chars, 0, width);
            return vec![Span::styled(shown, style)];
        }

        let cursor = self.cursor.min(chars.len());
        // One column is the cursor's when it sits past the last character.
        let budget = width.max(1);
        let start = window_start(&chars, cursor, budget);
        let before: String = chars[start..cursor].iter().collect();
        let under = chars.get(cursor).copied();
        let used = text_width(&before) + under.map_or(1, char_width);
        let after: String = match under {
            Some(_) => fit_from(&chars, cursor + 1, budget.saturating_sub(used)),
            None => String::new(),
        };

        let cursor_style = Style::default()
            .bg(style.fg.unwrap_or_default())
            .fg(style.bg.unwrap_or(ratatui::style::Color::Black));
        vec![
            Span::styled(before, style),
            match under {
                Some(ch) => Span::styled(ch.to_string(), cursor_style),
                None => Span::styled("\u{2588}", style),
            },
            Span::styled(after, style),
        ]
    }
}

/// Columns one character takes when drawn.
fn char_width(ch: char) -> usize {
    let mut buffer = [0u8; 4];
    Span::raw(&*ch.encode_utf8(&mut buffer)).width()
}

fn text_width(text: &str) -> usize {
    Span::raw(text).width()
}

/// The characters from `start` that fit in `width` columns.
fn fit_from(chars: &[char], start: usize, width: usize) -> String {
    let mut used = 0;
    chars
        .iter()
        .skip(start)
        .take_while(|ch| {
            used += char_width(**ch);
            used <= width
        })
        .collect()
}

/// The first character to draw so the cursor, at `cursor`, stays inside
/// `width` columns: the text scrolls left only as far as it must.
fn window_start(chars: &[char], cursor: usize, width: usize) -> usize {
    let cursor_width = chars.get(cursor).copied().map_or(1, char_width);
    let mut used = cursor_width;
    let mut start = cursor;
    while start > 0 {
        let next = char_width(chars[start - 1]);
        if used + next > width {
            break;
        }
        used += next;
        start -= 1;
    }
    start
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL)
    }

    fn typed(text: &str) -> LineInput {
        let mut input = LineInput::default();
        for ch in text.chars() {
            input.handle_key(key(KeyCode::Char(ch)));
        }
        input
    }

    fn drawn(input: &LineInput, width: usize) -> String {
        input
            .spans(width, true, None, Style::default())
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn typing_inserts_at_the_cursor() {
        let mut input = typed("ac");
        input.handle_key(key(KeyCode::Left));
        assert_eq!(input.handle_key(key(KeyCode::Char('b'))), Edit::Changed);
        assert_eq!(input.text(), "abc");

        input.handle_key(key(KeyCode::Home));
        input.handle_key(key(KeyCode::Char('>')));
        input.handle_key(key(KeyCode::End));
        input.handle_key(key(KeyCode::Char('<')));
        assert_eq!(input.text(), ">abc<");
    }

    #[test]
    fn backspace_and_delete_remove_around_the_cursor() {
        let mut input = typed("héllo");
        input.handle_key(key(KeyCode::Home));
        input.handle_key(key(KeyCode::Right));
        assert_eq!(input.handle_key(key(KeyCode::Delete)), Edit::Changed);
        assert_eq!(input.text(), "hllo");
        assert_eq!(input.handle_key(key(KeyCode::Backspace)), Edit::Changed);
        assert_eq!(input.text(), "llo");
        assert_eq!(
            input.handle_key(key(KeyCode::Backspace)),
            Edit::Moved,
            "nothing before the cursor"
        );
    }

    #[test]
    fn control_chords_edit_instead_of_typing_their_letter() {
        let mut input = typed("alpha beta gamma");
        assert_eq!(input.handle_key(ctrl('w')), Edit::Changed);
        assert_eq!(input.text(), "alpha beta ");
        input.handle_key(ctrl('a'));
        input.handle_key(key(KeyCode::Right));
        assert_eq!(input.handle_key(ctrl('k')), Edit::Changed);
        assert_eq!(input.text(), "a");
        input.set("clear me");
        assert_eq!(input.handle_key(ctrl('u')), Edit::Changed);
        assert_eq!(input.text(), "");

        // A terminal that sends ^H for Backspace must not type an `h`.
        let mut input = typed("ab");
        input.handle_key(ctrl('h'));
        assert_eq!(input.text(), "a");

        // Unbound chords are left to the caller and type nothing.
        let mut input = typed("ab");
        assert_eq!(input.handle_key(ctrl('c')), Edit::Ignored);
        assert_eq!(input.handle_key(ctrl('r')), Edit::Ignored);
        assert_eq!(
            input.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT)),
            Edit::Ignored
        );
        assert_eq!(input.text(), "ab");
    }

    #[test]
    fn altgr_characters_type_normally() {
        let mut input = LineInput::default();
        let altgr = KeyModifiers::CONTROL | KeyModifiers::ALT;
        input.handle_key(KeyEvent::new(KeyCode::Char('@'), altgr));
        assert_eq!(input.text(), "@");
    }

    #[test]
    fn a_refused_character_is_consumed_but_not_typed() {
        let mut input = LineInput::default();
        let digits = |ch: char| ch.is_ascii_digit();
        assert_eq!(
            input.handle_key_accepting(key(KeyCode::Char('q')), digits),
            Edit::Moved
        );
        assert_eq!(
            input.handle_key_accepting(key(KeyCode::Char('7')), digits),
            Edit::Changed
        );
        assert_eq!(input.text(), "7");
    }

    #[test]
    fn a_paste_drops_newlines_and_refused_characters() {
        let mut input = typed("key=");
        assert_eq!(input.paste("abc\r\n\tdef", |_| true), Edit::Changed);
        assert_eq!(input.text(), "key=abcdef");

        let mut input = LineInput::default();
        input.paste("10.0.0.x1\n", |ch| ch.is_ascii_digit() || ch == '.');
        assert_eq!(input.text(), "10.0.0.1");
    }

    #[test]
    fn a_long_value_scrolls_to_keep_the_cursor_in_view() {
        let mut input = typed("abcdefghij");
        assert_eq!(drawn(&input, 6), "fghij\u{2588}");

        input.handle_key(key(KeyCode::Home));
        assert_eq!(drawn(&input, 6), "abcdef");

        // Wide characters take two columns each.
        let input = typed("日本語日本語");
        assert_eq!(drawn(&input, 5), "本語\u{2588}");
    }

    #[test]
    fn an_unfocused_field_shows_its_start_and_a_mask_hides_it() {
        let input = typed("hunter2");
        let plain = input.spans(4, false, None, Style::default());
        assert_eq!(plain[0].content, "hunt");
        let masked = input.spans(20, false, Some('\u{25CF}'), Style::default());
        assert_eq!(masked[0].content.chars().count(), 7);
        assert!(!masked[0].content.contains('h'));
    }
}
