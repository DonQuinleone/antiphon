use antiphon_ui::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::app::App;
use super::compose::ComposeState;

const BODY_PREVIEW_LINES: usize = 8;
const PLAIN_PLAN: &str = "[plain]";
const SELECTED_MARK: &str = "\u{25b8} ";
const UNSELECTED_MARK: &str = "  ";
const LABEL_WIDTH: usize = 10;
const BYTES_PER_K: u32 = 1024;

/// The review screen wears the same clothes as the fields
/// stage: accent labels in the shared column, muted section
/// headings, and the keys down in the status line with every
/// other view's.
pub(super) fn draw_review(frame: &mut Frame, app: &App, area: Rect) {
    let Some(state) = &app.compose else {
        return;
    };
    let theme = app.theme;
    let mut lines = Vec::new();
    let labelled = [
        ("To:", state.fields.to.clone()),
        ("Cc:", state.fields.cc.clone()),
        ("Bcc:", state.fields.bcc.clone()),
        ("Subject:", state.fields.subject.clone()),
        ("From:", state.sender_line()),
        ("Fcc:", "sent".to_string()),
        ("Security:", plan_label(state)),
        (
            "Size:",
            format!(
                "{} \u{b7} {} attachment(s)",
                approx_size(state),
                state.attachments.len()
            ),
        ),
        ("Send:", super::schedule::label(state.schedule)),
    ];
    for (label, value) in labelled {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{label:<LABEL_WIDTH$}"),
                Style::new().fg(theme.accent),
            ),
            Span::styled(value, Style::new().fg(theme.text_primary)),
        ]));
    }
    lines.push(Line::default());
    lines.push(section(theme, "ATTACHMENTS"));
    lines.extend(attachment_lines(app, state));
    lines.push(Line::default());
    lines.push(section(theme, "PREVIEW"));
    lines.extend(body_preview(app, state));
    frame.render_widget(Paragraph::new(lines), area);
}

fn section(theme: &Theme, title: &str) -> Line<'static> {
    Line::from(Span::styled(
        title.to_string(),
        Style::new()
            .fg(theme.text_muted)
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

/// One row per file with the selection marker d acts on;
/// an empty list says so quietly.
fn attachment_lines(
    app: &App,
    state: &ComposeState,
) -> Vec<Line<'static>> {
    let theme = app.theme;
    if state.attachments.is_empty() {
        return vec![Line::from(Span::styled(
            "none \u{b7} a attaches".to_string(),
            Style::new().fg(theme.text_muted),
        ))];
    }
    let mut lines = Vec::new();
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
    use super::super::testkit::app_with_messages;
    use super::*;

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
        assert!(row(0).starts_with("To:"), "{:?}", row(0));
        assert!(row(0).contains("alba@example.com"), "{:?}", row(0));
        assert!(row(3).contains("Rehearsal"), "{:?}", row(3));
        assert!(
            row(4).contains("Tester <tester@example.com>"),
            "{:?}",
            row(4)
        );
        assert!(row(5).contains("sent"), "{:?}", row(5));
        assert!(row(6).contains("[encrypt]"), "{:?}", row(6));
        assert!(row(7).starts_with("Size:"), "{:?}", row(7));
        assert!(row(8).starts_with("Send:"), "{:?}", row(8));
        assert!(row(10).starts_with("ATTACHMENTS"), "{:?}", row(10));
        assert!(row(11).contains("none"), "{:?}", row(11));
        assert!(row(13).starts_with("PREVIEW"), "{:?}", row(13));
        assert!(row(14).contains("body line 1"), "{:?}", row(14));
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
        assert!(row(7).contains("2 attachment(s)"), "{:?}", row(7));
        assert!(
            row(11).starts_with(
                "\u{25b8} a.pdf (application/pdf, 3 bytes)"
            ),
            "{:?}",
            row(11)
        );
        assert!(
            row(12).starts_with("  b.txt (text/plain, 3 bytes)"),
            "{:?}",
            row(12)
        );
    }
}
