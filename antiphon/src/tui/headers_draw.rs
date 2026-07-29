use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::app::App;
use super::compose::ComposeState;
use super::headers::{FIELD_COUNT, with_cursor};

const LABELS: [&str; FIELD_COUNT] =
    ["To:", "Cc:", "Bcc:", "Subject:", "From:"];
const LABEL_COLS: usize = 10;
const POPOVER_MAX_WIDTH: u16 = 46;

pub(super) fn draw_headers(frame: &mut Frame, app: &App, area: Rect) {
    let Some(state) = &app.compose else {
        return;
    };
    let lines = field_lines(app.theme, state, true);
    frame.render_widget(Paragraph::new(lines), area);
}

/// The contact suggestions as a popover under the focused
/// field, overlaying whatever sits beneath so the layout never
/// reflows; drawn last in the compose view.
pub(super) fn draw_completion(
    frame: &mut Frame,
    app: &App,
    area: Rect,
) {
    let Some(state) = &app.compose else {
        return;
    };
    let Some(completion) = &state.completion else {
        return;
    };
    let anchor_row = state.fields.focus as u16 + 1;
    if anchor_row >= area.height {
        return;
    }
    let popover = Rect {
        x: area.x + LABEL_COLS as u16,
        y: area.y + anchor_row,
        width: POPOVER_MAX_WIDTH
            .min(area.width.saturating_sub(LABEL_COLS as u16)),
        height: (completion.items.len() as u16)
            .min(area.height - anchor_row),
    };
    frame.render_widget(ratatui::widgets::Clear, popover);
    let theme = app.theme;
    let lines: Vec<Line<'static>> = completion
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let style = if index == completion.selected {
                Style::new()
                    .fg(theme.accent_strong)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(theme.text_primary)
            };
            Line::from(Span::styled(format!(" {item}"), style))
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).style(Style::new().bg(theme.surface)),
        popover,
    );
}

/// The five header rows, shared by the fields stage (with focus
/// and cursor) and the summary above the body editor.
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

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::super::compose::test_state;
    use super::super::testkit::app_with_messages;
    use super::*;

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
            row(0).starts_with("To:       alba@example.com\u{258c}"),
            "{:?}",
            row(0)
        );
        assert!(row(1).starts_with("Cc:"), "{:?}", row(1));
        assert!(row(2).starts_with("Bcc:"), "{:?}", row(2));
        assert!(row(3).starts_with("Subject:"), "{:?}", row(3));
        assert!(
            row(4).starts_with("From:     Tester <tester@example.com>"),
            "{:?}",
            row(4)
        );
        assert_eq!(row(5).trim(), "", "no hint row below the fields");
    }
}
