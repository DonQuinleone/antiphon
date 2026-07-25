use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::app::App;
use super::compose::ComposeState;
use super::prefill::DraftFields;

pub(super) const FIELD_COUNT: usize = 5;
const LAST_FIELD: usize = FIELD_COUNT - 1;
const FROM_FIELD: usize = LAST_FIELD;
const LABELS: [&str; FIELD_COUNT] =
    ["To:", "Cc:", "Bcc:", "Subject:", "From:"];
const LABEL_COLS: usize = 9;
const CURSOR: char = '\u{258c}';
const HINT: &str = "tab/shift-tab move \u{b7} enter on From (or \
                    ctrl-e) opens the editor \u{b7} esc backs out";

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

/// What one key did to the fields, for the event loop to act
/// on; identity cycling is handled by the compose state.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum HeadersOutcome {
    Edited,
    CycleFrom(i32),
    OpenEditor,
    Cancel,
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

    pub fn feed(&mut self, key: KeyEvent) -> HeadersOutcome {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return self.control_key(key.code);
        }
        match key.code {
            KeyCode::Esc => HeadersOutcome::Cancel,
            KeyCode::Tab => self.step_focus(1),
            KeyCode::BackTab => self.step_focus(-1),
            KeyCode::Enter => self.enter(),
            other => self.field_key(other),
        }
    }

    fn control_key(&mut self, code: KeyCode) -> HeadersOutcome {
        match code {
            KeyCode::Char('e') => HeadersOutcome::OpenEditor,
            _ => HeadersOutcome::Edited,
        }
    }

    fn enter(&mut self) -> HeadersOutcome {
        if self.focus == LAST_FIELD {
            return HeadersOutcome::OpenEditor;
        }
        self.step_focus(1)
    }

    fn step_focus(&mut self, step: i32) -> HeadersOutcome {
        let count = FIELD_COUNT as i32;
        let next = (self.focus as i32 + step).rem_euclid(count);
        self.focus = next as usize;
        self.cursor = self.field().chars().count();
        HeadersOutcome::Edited
    }

    fn field_key(&mut self, code: KeyCode) -> HeadersOutcome {
        if self.focus == FROM_FIELD {
            return from_outcome(code);
        }
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
        HeadersOutcome::Edited
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

fn from_outcome(code: KeyCode) -> HeadersOutcome {
    match code {
        KeyCode::Right | KeyCode::Char(' ') => {
            HeadersOutcome::CycleFrom(1)
        }
        KeyCode::Left => HeadersOutcome::CycleFrom(-1),
        _ => HeadersOutcome::Edited,
    }
}

fn byte_index(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

pub(super) fn draw_headers(frame: &mut Frame, app: &App, area: Rect) {
    let Some(state) = &app.compose else {
        return;
    };
    let mut lines = field_lines(app.theme, state, true);
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        HINT,
        Style::new().fg(app.theme.text_muted),
    )));
    frame.render_widget(Paragraph::new(lines), area);
}

/// The five header rows, shared by the fields stage (with
/// focus and cursor) and the summary above the body editor.
pub(super) fn field_lines(
    theme: &antiphon_ui::Theme,
    state: &ComposeState,
    focused: bool,
) -> Vec<Line<'static>> {
    let fields = &state.fields;
    let values = [
        fields.to.clone(),
        fields.cc.clone(),
        fields.bcc.clone(),
        fields.subject.clone(),
        state.sender_line(),
    ];
    LABELS
        .iter()
        .zip(values)
        .enumerate()
        .map(|(index, (label, value))| {
            let active = focused && index == fields.focus;
            field_line(theme, label, value, active, fields.cursor)
        })
        .collect()
}

fn field_line(
    theme: &antiphon_ui::Theme,
    label: &'static str,
    value: String,
    active: bool,
    cursor: usize,
) -> Line<'static> {
    let mut label_style = Style::new().fg(theme.accent);
    if active {
        label_style = label_style
            .fg(theme.accent_strong)
            .add_modifier(Modifier::BOLD);
    }
    let value = if active {
        with_cursor(&value, cursor)
    } else {
        value
    };
    Line::from(vec![
        Span::styled(format!("{label:<LABEL_COLS$}"), label_style),
        Span::styled(value, Style::new().fg(theme.text_primary)),
    ])
}

fn with_cursor(value: &str, cursor: usize) -> String {
    let at = byte_index(value, cursor);
    let mut out = value.to_string();
    out.insert(at, CURSOR);
    out
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::super::compose::test_state;
    use super::super::testkit::app_with_messages;
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn typed(fields: &mut HeaderFields, text: &str) {
        for ch in text.chars() {
            fields.feed(key(KeyCode::Char(ch)));
        }
    }

    #[test]
    fn keys_drive_the_field_machine_per_table() {
        use HeadersOutcome::*;
        use KeyCode::*;

        let cases: &[(usize, KeyCode, HeadersOutcome, usize)] = &[
            (0, Tab, Edited, 1),
            (1, Tab, Edited, 2),
            (4, Tab, Edited, 0),
            (0, BackTab, Edited, 4),
            (3, BackTab, Edited, 2),
            (0, Enter, Edited, 1),
            (3, Enter, Edited, 4),
            (4, Enter, OpenEditor, 4),
            (2, Esc, Cancel, 2),
            (4, Right, CycleFrom(1), 4),
            (4, Left, CycleFrom(-1), 4),
            (4, Char(' '), CycleFrom(1), 4),
            (4, Char('x'), Edited, 4),
        ];
        for (focus, code, expected, after) in cases {
            let mut fields = HeaderFields {
                focus: *focus,
                ..HeaderFields::default()
            };
            let outcome = fields.feed(key(*code));
            assert_eq!(outcome, *expected, "{focus} {code:?}");
            assert_eq!(fields.focus, *after, "{focus} {code:?}");
        }
    }

    #[test]
    fn ctrl_e_opens_the_editor_from_any_field() {
        for focus in 0..FIELD_COUNT {
            let mut fields = HeaderFields {
                focus,
                ..HeaderFields::default()
            };
            let outcome = fields.feed(KeyEvent::new(
                KeyCode::Char('e'),
                KeyModifiers::CONTROL,
            ));
            assert_eq!(outcome, HeadersOutcome::OpenEditor, "{focus}");
        }
    }

    #[test]
    fn line_editing_inserts_and_deletes_at_the_cursor() {
        let mut fields = HeaderFields::default();
        typed(&mut fields, "ab@example.com");
        assert_eq!(fields.to, "ab@example.com");
        for _ in 0.."example.com".len() {
            fields.feed(key(KeyCode::Left));
        }
        fields.feed(key(KeyCode::Backspace));
        typed(&mut fields, "+list@");
        fields.feed(key(KeyCode::End));
        assert_eq!(fields.to, "ab+list@example.com");
        fields.feed(key(KeyCode::Home));
        fields.feed(key(KeyCode::Delete));
        assert_eq!(fields.to, "b+list@example.com");
        fields.feed(key(KeyCode::Backspace));
        assert_eq!(fields.to, "b+list@example.com");
    }

    #[test]
    fn switching_fields_parks_the_cursor_at_the_end() {
        let mut fields = HeaderFields::default();
        typed(&mut fields, "to@example.com");
        fields.feed(key(KeyCode::Tab));
        assert_eq!(fields.cursor, 0);
        typed(&mut fields, "cc@example.com");
        fields.feed(key(KeyCode::BackTab));
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
        fields.feed(key(KeyCode::Left));
        fields.feed(key(KeyCode::Backspace));
        assert_eq!(fields.subject, "gr\u{fc}e");
    }

    #[test]
    fn the_fields_render_with_focus_and_cursor() {
        let mut app = app_with_messages(1);
        let mut state = test_state();
        state.fields.to = "alba@example.com".to_string();
        state.fields.cursor = state.fields.to.chars().count();
        app.compose = Some(state);

        let backend = TestBackend::new(60, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                draw_headers(frame, &app, frame.area());
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let row = |y: u16| -> String {
            (0..buffer.area.width)
                .map(|x| {
                    buffer.cell((x, y)).unwrap().symbol().to_string()
                })
                .collect()
        };
        assert!(
            row(0).starts_with("To:      alba@example.com\u{258c}"),
            "{:?}",
            row(0)
        );
        assert!(row(1).starts_with("Cc:"), "{:?}", row(1));
        assert!(row(2).starts_with("Bcc:"), "{:?}", row(2));
        assert!(row(3).starts_with("Subject:"), "{:?}", row(3));
        assert!(
            row(4).starts_with("From:    Tester <tester@example.com>"),
            "{:?}",
            row(4)
        );
        assert!(row(6).contains("tab/shift-tab"), "{:?}", row(6));
    }
}
