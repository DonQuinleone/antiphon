use antiphon_ui::Theme;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::app::App;
use super::compose::ComposeState;
use super::draw::header_line;

const BODY_PREVIEW_LINES: usize = 8;
const PLAIN_PLAN: &str = "[plain]";
const SELECTED_MARK: &str = "\u{25b8} ";
const UNSELECTED_MARK: &str = "  ";
const KEYBAR: &str = "y:Send  q:Draft  e:Edit  h:Headers  \
                      a:Attach  d:Remove  s:Sign  x:Encrypt  \
                      ?:Help";
const LABEL_WIDTH: usize = 9;
const BYTES_PER_K: u32 = 1024;

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
    let bar = Style::new()
        .fg(theme.text_primary)
        .bg(theme.surface)
        .add_modifier(Modifier::BOLD);
    let mut lines = vec![Line::from(Span::styled(
        format!("{KEYBAR:<width$}", width = area.width as usize),
        bar,
    ))];
    let labelled = [
        ("From:", state.sender_line()),
        ("To:", state.fields.to.clone()),
        ("Cc:", state.fields.cc.clone()),
        ("Bcc:", state.fields.bcc.clone()),
        ("Subject:", state.fields.subject.clone()),
        ("Fcc:", "sent".to_string()),
        ("Security:", plan_label(state)),
    ];
    for (label, value) in labelled {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{label:>LABEL_WIDTH$} "),
                Style::new().fg(theme.accent),
            ),
            Span::styled(value, Style::new().fg(theme.text_primary)),
        ]));
    }
    lines.push(section(theme, area.width, "-- Attachments"));
    lines.extend(attachment_lines(app, state));
    lines.push(section(theme, area.width, "-- Preview"));
    lines.extend(body_preview(app, state));
    frame.render_widget(Paragraph::new(lines), area);
    let status = Line::from(Span::styled(
        format!(
            "-- Antiphon: Compose  [Approx. msg size: {}  \
             Atts: {}]",
            approx_size(state),
            state.attachments.len(),
        ),
        bar,
    ));
    let footer_area = Rect {
        y: area.y + area.height.saturating_sub(1),
        height: area.height.min(1),
        ..area
    };
    frame.render_widget(Paragraph::new(status), footer_area);
}

fn section(theme: &Theme, width: u16, title: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("{title:<width$}", width = width as usize),
        Style::new()
            .fg(theme.text_primary)
            .bg(theme.surface)
            .add_modifier(Modifier::BOLD),
    ))
}

fn approx_size(state: &ComposeState) -> String {
    let attachments: usize = state
        .attachments
        .iter()
        .map(|attachment| attachment.bytes.len())
        .sum();
    let total = state.body.len() + attachments;
    if total < BYTES_PER_K as usize {
        return format!("{total}B");
    }
    format!("{:.1}K", total as f64 / f64::from(BYTES_PER_K))
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
        assert!(row(0).starts_with("y:Send"), "{:?}", row(0));
        assert!(
            row(1).contains("From: Tester <tester@example.com>"),
            "{:?}",
            row(1)
        );
        assert!(row(2).contains("alba@example.com"), "{:?}", row(2));
        assert!(row(5).contains("Rehearsal"), "{:?}", row(5));
        assert!(row(6).contains("Fcc: sent"), "{:?}", row(6));
        assert!(row(7).contains("[encrypt]"), "{:?}", row(7));
        assert!(row(8).starts_with("-- Attachments"), "{:?}", row(8));
        assert!(row(9).contains("Attachments: none"), "{:?}", row(9));
        assert!(row(10).starts_with("-- Preview"), "{:?}", row(10));
        assert!(row(11).contains("body line 1"), "{:?}", row(11));
        assert!(
            row(19).starts_with("-- Antiphon: Compose"),
            "{:?}",
            row(19)
        );
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
        assert!(row(9).contains("Attachments: 2"), "{:?}", row(9));
        assert!(
            row(10).starts_with(
                "\u{25b8} a.pdf (application/pdf, 3 bytes)"
            ),
            "{:?}",
            row(10)
        );
        assert!(
            row(11).starts_with("  b.txt (text/plain, 3 bytes)"),
            "{:?}",
            row(11)
        );
    }
}
