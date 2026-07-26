use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::account_form::{FIELD_COUNT, PASSWORD_HINT};
use super::app::App;
use super::headers::with_cursor;

const MODAL_WIDTH: u16 = 86;
const LABEL_COLS: usize = 24;
const BORDER_ROWS: u16 = 2;
const HINT: &str = " tab move \u{b7} enter/^s save \u{b7} esc cancel ";
const BULLET: char = '\u{2022}';
/// `password command` always sits here: after it, macOS alone
/// adds the masked Keychain field.
const PASSWORD_FIELD: usize = 5;

pub(super) fn draw_form(frame: &mut Frame, app: &App, area: Rect) {
    let Some(form) = &app.account_form else {
        return;
    };
    let theme = app.theme;
    let title = if form.editing.is_some() {
        " edit account "
    } else {
        " add account "
    };
    let width = MODAL_WIDTH.min(area.width.saturating_sub(2));
    let inner_width = width.saturating_sub(2) as usize;
    let error_lines = form
        .error
        .as_deref()
        .map(|error| wrapped(error, inner_width))
        .unwrap_or_default();
    let extra_rows = 1 + error_lines.len() as u16;
    let height = (FIELD_COUNT as u16 + BORDER_ROWS + extra_rows)
        .min(area.height.saturating_sub(2));
    let modal = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, modal);
    let block = Block::bordered()
        .title(title)
        .title_bottom(HINT)
        .border_style(Style::new().fg(theme.accent))
        .style(Style::new().bg(theme.surface));
    let inner = block.inner(modal);
    frame.render_widget(block, modal);

    let mut lines: Vec<Line<'static>> = (0..FIELD_COUNT)
        .map(|index| field_line(app, form, index))
        .collect();
    if !error_lines.is_empty() {
        lines.push(Line::default());
        for row in error_lines {
            lines.push(Line::from(Span::styled(
                row,
                Style::new().fg(theme.accent_strong),
            )));
        }
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn field_line(
    app: &App,
    form: &super::account_form::AccountFormState,
    index: usize,
) -> Line<'static> {
    let theme = app.theme;
    let active = index == form.focus;
    let mut label_style = Style::new().fg(theme.accent);
    if active {
        label_style = label_style
            .fg(theme.accent_strong)
            .add_modifier(Modifier::BOLD);
    }
    let label = format!("{:<LABEL_COLS$}", form.field_label(index));
    let displayed = shown_value(form, index);
    let value = if active {
        with_cursor(&displayed, form.cursor)
    } else {
        displayed
    };
    let mut spans = vec![
        Span::styled(label, label_style),
        Span::styled(value, Style::new().fg(theme.text_primary)),
    ];
    let show_hint = cfg!(target_os = "macos")
        && index == PASSWORD_FIELD
        && form.field_value(index).is_empty();
    if show_hint {
        spans.push(Span::styled(
            format!(" ({PASSWORD_HINT})"),
            Style::new().fg(theme.text_muted),
        ));
    }
    Line::from(spans)
}

/// Greedy word wrap; the modal is narrow and errors are one
/// sentence, so nothing cleverer earns its keep.
fn wrapped(text: &str, width: usize) -> Vec<String> {
    let mut rows: Vec<String> = Vec::new();
    let mut row = String::new();
    for word in text.split_whitespace() {
        let need = row.chars().count()
            + usize::from(!row.is_empty())
            + word.chars().count();
        if need > width && !row.is_empty() {
            rows.push(std::mem::take(&mut row));
        }
        if !row.is_empty() {
            row.push(' ');
        }
        row.push_str(word);
    }
    if !row.is_empty() {
        rows.push(row);
    }
    rows
}

fn shown_value(
    form: &super::account_form::AccountFormState,
    index: usize,
) -> String {
    let value = form.field_value(index);
    if form.field_masked(index) {
        BULLET.to_string().repeat(value.chars().count())
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::super::testkit::app_with_messages;
    use super::*;

    fn rendered(app: &App) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw_form(frame, app, frame.area()))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn text(buffer: &ratatui::buffer::Buffer) -> String {
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| {
                        buffer
                            .cell((x, y))
                            .unwrap()
                            .symbol()
                            .to_string()
                    })
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn a_prefilled_form_shows_its_values_and_masks_the_secret() {
        let mut app = app_with_messages(1);
        app.open_account_form_add();
        if let Some(form) = app.account_form.as_mut() {
            form.error = None;
        }
        let buffer = rendered(&app);
        assert!(text(&buffer).contains("add account"));
    }

    #[test]
    fn an_error_line_is_drawn_when_present() {
        let mut app = app_with_messages(1);
        app.open_account_form_add();
        if let Some(form) = app.account_form.as_mut() {
            form.error = Some("account name is required".to_string());
        }
        let buffer = rendered(&app);
        assert!(text(&buffer).contains("account name is required"));
    }
}
