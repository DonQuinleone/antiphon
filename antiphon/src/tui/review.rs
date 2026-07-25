use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::app::App;
use super::compose::ComposeState;
use super::draw::header_line;

const BODY_PREVIEW_LINES: usize = 8;
const PLAIN_PLAN: &str = "[plain]";
const SELECTED_MARK: &str = "\u{25b8} ";
const UNSELECTED_MARK: &str = "  ";
const FOOTER: &str = "y send \u{b7} e body \u{b7} h headers \u{b7} \
                      a attach \u{b7} d remove \u{b7} s sign \u{b7} \
                      x encrypt \u{b7} q save draft \u{b7} ctrl-c \
                      stays";

/// What a review key asks of the event loop; toggles mutate
/// the state and stay put.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ReviewOutcome {
    Stay,
    Send,
    EditBody,
    EditHeaders,
    PromptAttachment,
    SaveDraft,
}

pub(super) fn feed(
    state: &mut ComposeState,
    key: KeyEvent,
) -> ReviewOutcome {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return ReviewOutcome::Stay;
    }
    match key.code {
        KeyCode::Char('y') => ReviewOutcome::Send,
        KeyCode::Char('e') => ReviewOutcome::EditBody,
        KeyCode::Char('h') => ReviewOutcome::EditHeaders,
        KeyCode::Char('a') => ReviewOutcome::PromptAttachment,
        KeyCode::Char('d') => remove_attachment(state),
        KeyCode::Char('s') => toggle_sign(state),
        KeyCode::Char('x') => toggle_encrypt(state),
        KeyCode::Char('q') => ReviewOutcome::SaveDraft,
        KeyCode::Char('j') | KeyCode::Down => select(state, 1),
        KeyCode::Char('k') | KeyCode::Up => select(state, -1),
        _ => ReviewOutcome::Stay,
    }
}

fn remove_attachment(state: &mut ComposeState) -> ReviewOutcome {
    state.remove_selected_attachment();
    ReviewOutcome::Stay
}

fn select(state: &mut ComposeState, step: i32) -> ReviewOutcome {
    state.select_attachment(step);
    ReviewOutcome::Stay
}

fn toggle_sign(state: &mut ComposeState) -> ReviewOutcome {
    state.sign_override = Some(!state.plan().sign);
    ReviewOutcome::Stay
}

fn toggle_encrypt(state: &mut ComposeState) -> ReviewOutcome {
    state.encrypt_override = Some(!state.plan().encrypt);
    ReviewOutcome::Stay
}

pub(super) fn draw_review(frame: &mut Frame, app: &App, area: Rect) {
    let Some(state) = &app.compose else {
        return;
    };
    let theme = app.theme;
    let mut lines = vec![
        header_line(theme, "From:", state.sender_line()),
        header_line(theme, "To:", state.fields.to.clone()),
        header_line(theme, "Cc:", state.fields.cc.clone()),
        header_line(theme, "Bcc:", state.fields.bcc.clone()),
        header_line(theme, "Subject:", state.fields.subject.clone()),
        header_line(theme, "Plan:", plan_label(state)),
    ];
    lines.extend(attachment_lines(app, state));
    lines.push(Line::default());
    lines.extend(body_preview(app, state));
    let footer_row = area.height.saturating_sub(1);
    let body_area = Rect {
        height: footer_row,
        ..area
    };
    frame.render_widget(Paragraph::new(lines), body_area);
    let footer = Line::from(Span::styled(
        FOOTER,
        Style::new().fg(theme.text_muted),
    ));
    let footer_area = Rect {
        y: area.y + footer_row,
        height: area.height.min(1),
        ..area
    };
    frame.render_widget(Paragraph::new(footer), footer_area);
}

fn plan_label(state: &ComposeState) -> String {
    state.plan().label().unwrap_or(PLAIN_PLAN).to_string()
}

/// The attachments block: a count line, then one row per
/// file with the selection marker d acts on.
fn attachment_lines(
    app: &App,
    state: &ComposeState,
) -> Vec<Line<'static>> {
    let theme = app.theme;
    let count = state.attachments.len();
    let heading = match count {
        0 => "none".to_string(),
        _ => format!("{count}"),
    };
    let mut lines = vec![header_line(theme, "Attachments:", heading)];
    for (index, attachment) in state.attachments.iter().enumerate() {
        let selected = index == state.selected_attachment;
        let marker = if selected {
            SELECTED_MARK
        } else {
            UNSELECTED_MARK
        };
        let mut style = Style::new().fg(theme.text_primary);
        if selected {
            style = style.fg(theme.accent_strong);
        }
        lines.push(Line::from(Span::styled(
            format!("{marker}{}", attachment.label()),
            style,
        )));
    }
    lines
}

fn body_preview(app: &App, state: &ComposeState) -> Vec<Line<'static>> {
    let muted = Style::new().fg(app.theme.text_muted);
    let mut lines: Vec<Line<'static>> = state
        .body
        .lines()
        .take(BODY_PREVIEW_LINES)
        .map(|line| {
            Line::from(Span::styled(
                line.to_string(),
                Style::new().fg(app.theme.text_primary),
            ))
        })
        .collect();
    let total = state.body.lines().count();
    if total > BODY_PREVIEW_LINES {
        lines.push(Line::from(Span::styled(
            format!(
                "\u{2026} {} more body line(s)",
                total - BODY_PREVIEW_LINES
            ),
            muted,
        )));
    }
    lines
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::super::compose::test_state;
    use super::super::crypto::PgpPlan;
    use super::super::testkit::app_with_messages;
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn review_keys_map_to_outcomes_per_table() {
        use KeyCode::Char;
        use ReviewOutcome::*;

        let cases: &[(KeyCode, ReviewOutcome)] = &[
            (Char('y'), Send),
            (Char('e'), EditBody),
            (Char('h'), EditHeaders),
            (Char('a'), PromptAttachment),
            (Char('d'), Stay),
            (Char('j'), Stay),
            (Char('k'), Stay),
            (Char('q'), SaveDraft),
            (Char('s'), Stay),
            (Char('x'), Stay),
            (Char('z'), Stay),
            (KeyCode::Esc, Stay),
            (KeyCode::Enter, Stay),
        ];
        for (code, expected) in cases {
            let mut state = test_state();
            assert_eq!(
                feed(&mut state, key(*code)),
                *expected,
                "{code:?}"
            );
        }
    }

    #[test]
    fn ctrl_c_discards_nothing_and_stays() {
        let mut state = test_state();
        state.body = "precious".to_string();
        let outcome = feed(
            &mut state,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );
        assert_eq!(outcome, ReviewOutcome::Stay);
        assert_eq!(state.body, "precious");
    }

    #[test]
    fn toggles_flip_the_plan_and_survive_repeats() {
        let mut state = test_state();
        feed(&mut state, key(KeyCode::Char('s')));
        assert_eq!(
            state.plan(),
            PgpPlan {
                sign: true,
                encrypt: false
            }
        );
        feed(&mut state, key(KeyCode::Char('x')));
        assert!(state.plan().encrypt);
        feed(&mut state, key(KeyCode::Char('s')));
        feed(&mut state, key(KeyCode::Char('x')));
        assert_eq!(state.plan(), PgpPlan::default());
    }

    #[test]
    fn the_review_screen_shows_fields_plan_and_preview() {
        let mut app = app_with_messages(1);
        let mut state = test_state();
        state.fields.to = "alba@example.com".to_string();
        state.fields.subject = "Rehearsal".to_string();
        state.encrypt_override = Some(true);
        state.body = (1..=10)
            .map(|line| format!("body line {line}\n"))
            .collect();
        app.compose = Some(state);

        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw_review(frame, &app, frame.area()))
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
            row(0).starts_with("From: Tester <tester@example.com>"),
            "{:?}",
            row(0)
        );
        assert!(row(1).contains("alba@example.com"), "{:?}", row(1));
        assert!(row(4).contains("Rehearsal"), "{:?}", row(4));
        assert!(row(5).contains("[encrypt]"), "{:?}", row(5));
        assert!(row(6).contains("Attachments: none"), "{:?}", row(6));
        assert!(row(8).contains("body line 1"), "{:?}", row(8));
        assert!(
            row(16).contains("2 more body line(s)"),
            "{:?}",
            row(16)
        );
        assert!(row(19).contains("y send"), "{:?}", row(19));
    }

    #[test]
    fn attachment_rows_render_with_a_selection_marker() {
        use super::super::attach::Attachment;

        let mut app = app_with_messages(1);
        let mut state = test_state();
        for name in ["a.pdf", "b.txt"] {
            state.add_attachment(Attachment {
                path: name.into(),
                filename: name.to_string(),
                content_type: antiphon_render::content_type_for(name),
                bytes: vec![0; 3],
            });
        }
        state.select_attachment(-1);
        app.compose = Some(state);

        let backend = TestBackend::new(60, 16);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw_review(frame, &app, frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let row = |y: u16| -> String {
            (0..buffer.area.width)
                .map(|x| {
                    buffer.cell((x, y)).unwrap().symbol().to_string()
                })
                .collect()
        };
        assert!(row(6).contains("Attachments: 2"), "{:?}", row(6));
        assert!(
            row(7).starts_with(
                "\u{25b8} a.pdf (application/pdf, 3 bytes)"
            ),
            "{:?}",
            row(7)
        );
        assert!(
            row(8).starts_with("  b.txt (text/plain, 3 bytes)"),
            "{:?}",
            row(8)
        );
    }
}
