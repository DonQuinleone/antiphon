use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) use super::headers_draw::{
    draw_completion, draw_headers, field_lines,
};
use super::prefill::DraftFields;

pub(super) const FIELD_COUNT: usize = 5;
const LAST_FIELD: usize = FIELD_COUNT - 1;
const FROM_FIELD: usize = LAST_FIELD;
const RECIPIENT_FIELDS: usize = 3;
const CURSOR: char = '\u{258c}';

/// The structured header fields above the body editor: To,
/// Cc, Bcc and Subject take free text; From cycles through
/// the configured identities.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct HeaderFields {
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub focus: usize,
    pub cursor: usize,
}

impl HeaderFields {
    pub fn from_draft(fields: &DraftFields) -> HeaderFields {
        HeaderFields {
            to: fields.to.clone(),
            cc: fields.cc.clone(),
            bcc: fields.bcc.clone(),
            subject: fields.subject.clone(),
            focus: 0,
            cursor: fields.to.chars().count(),
        }
    }

    /// Literal line editing for the focused field; From is not
    /// a text field, so there Left/Right/Space ask to cycle the
    /// identity (the returned step) and every other key is
    /// inert. Focus, submit and cancel are resolved as compose
    /// actions before a key ever reaches here.
    pub fn edit(&mut self, key: KeyEvent) -> Option<i32> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return None;
        }
        if self.focus == FROM_FIELD {
            return from_cycle(key.code);
        }
        self.field_key(key.code);
        None
    }

    pub fn step_focus(&mut self, step: i32) {
        let count = FIELD_COUNT as i32;
        let next = (self.focus as i32 + step).rem_euclid(count);
        self.focus = next as usize;
        self.cursor = self.field().chars().count();
    }

    pub fn at_last_field(&self) -> bool {
        self.focus == LAST_FIELD
    }

    fn field_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char(ch) => self.insert(ch),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1)
            }
            KeyCode::Right => {
                self.cursor =
                    (self.cursor + 1).min(self.field().chars().count())
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.field().chars().count(),
            _ => {}
        }
    }

    fn insert(&mut self, ch: char) {
        let cursor = self.cursor;
        let field = self.field_mut();
        let at = byte_index(field, cursor);
        field.insert(at, ch);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor -= 1;
        self.delete();
    }

    fn delete(&mut self) {
        let cursor = self.cursor;
        let field = self.field_mut();
        if cursor >= field.chars().count() {
            return;
        }
        let at = byte_index(field, cursor);
        field.remove(at);
    }

    pub fn recipient_focused(&self) -> bool {
        self.focus < RECIPIENT_FIELDS
    }

    /// Swaps the focused field's text wholesale, cursor at the
    /// end; completion acceptance is the caller.
    pub fn replace_field(&mut self, text: String) {
        self.cursor = text.chars().count();
        *self.field_mut() = text;
    }

    pub fn field(&self) -> &str {
        match self.focus {
            0 => &self.to,
            1 => &self.cc,
            2 => &self.bcc,
            3 => &self.subject,
            _ => "",
        }
    }

    fn field_mut(&mut self) -> &mut String {
        match self.focus {
            0 => &mut self.to,
            1 => &mut self.cc,
            2 => &mut self.bcc,
            _ => &mut self.subject,
        }
    }
}

fn from_cycle(code: KeyCode) -> Option<i32> {
    match code {
        KeyCode::Right | KeyCode::Char(' ') => Some(1),
        KeyCode::Left => Some(-1),
        _ => None,
    }
}

pub(super) fn byte_index(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

pub(super) fn with_cursor(value: &str, cursor: usize) -> String {
    let at = byte_index(value, cursor);
    let mut out = value.to_string();
    out.insert(at, CURSOR);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn typed(fields: &mut HeaderFields, text: &str) {
        for ch in text.chars() {
            fields.edit(key(KeyCode::Char(ch)));
        }
    }

    #[test]
    fn the_from_field_cycles_on_arrows_and_space() {
        use KeyCode::*;

        let cases: &[(KeyCode, Option<i32>)] = &[
            (Right, Some(1)),
            (Char(' '), Some(1)),
            (Left, Some(-1)),
            (Char('x'), None),
        ];
        for (code, expected) in cases {
            let mut fields = HeaderFields {
                focus: FROM_FIELD,
                ..HeaderFields::default()
            };
            assert_eq!(fields.edit(key(*code)), *expected, "{code:?}");
        }
    }

    #[test]
    fn step_focus_wraps_and_tracks_the_last_field() {
        let mut fields = HeaderFields::default();
        fields.step_focus(-1);
        assert_eq!(fields.focus, LAST_FIELD);
        assert!(fields.at_last_field());
        fields.step_focus(1);
        assert_eq!(fields.focus, 0);
        assert!(!fields.at_last_field());
    }

    #[test]
    fn line_editing_inserts_and_deletes_at_the_cursor() {
        let mut fields = HeaderFields::default();
        typed(&mut fields, "ab@example.com");
        assert_eq!(fields.to, "ab@example.com");
        for _ in 0.."example.com".len() {
            fields.edit(key(KeyCode::Left));
        }
        fields.edit(key(KeyCode::Backspace));
        typed(&mut fields, "+list@");
        fields.edit(key(KeyCode::End));
        assert_eq!(fields.to, "ab+list@example.com");
        fields.edit(key(KeyCode::Home));
        fields.edit(key(KeyCode::Delete));
        assert_eq!(fields.to, "b+list@example.com");
        fields.edit(key(KeyCode::Backspace));
        assert_eq!(fields.to, "b+list@example.com");
    }

    #[test]
    fn switching_fields_parks_the_cursor_at_the_end() {
        let mut fields = HeaderFields::default();
        typed(&mut fields, "to@example.com");
        fields.step_focus(1);
        assert_eq!(fields.cursor, 0);
        typed(&mut fields, "cc@example.com");
        fields.step_focus(-1);
        assert_eq!(fields.cursor, "to@example.com".len());
        assert_eq!(fields.cc, "cc@example.com");
    }

    #[test]
    fn multibyte_field_text_edits_by_characters() {
        let mut fields = HeaderFields {
            focus: 3,
            ..HeaderFields::default()
        };
        typed(&mut fields, "gr\u{fc}\u{df}e");
        fields.edit(key(KeyCode::Left));
        fields.edit(key(KeyCode::Backspace));
        assert_eq!(fields.subject, "gr\u{fc}e");
    }
}
