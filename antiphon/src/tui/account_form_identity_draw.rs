//! Drawing the identity sub-editor over the account form: the
//! identity list, or the per-identity field editor, centred as
//! its own bordered modal.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::account_form::AccountFormState;
use super::account_form_fields::ON_OFF_OPTIONS;
use super::account_form_identity::{
    EDITOR_FIELDS, FormIdentity, IdentityEditor, IdentityUi, descriptor,
};
use super::app::App;
use super::draw::segmented::{self, SegmentStyle};
use super::headers::with_cursor;

/// Narrower than the account form (72) so the identity editor
/// nests inside it rather than covering it, with the form's
/// fields framing it.
const MODAL_WIDTH: u16 = 62;
const LABEL_COLS: usize = 16;
const BORDER_ROWS: u16 = 2;
const LIST_HINT: &str = " a add \u{b7} e edit \u{b7} d remove \u{b7} \
     esc back ";
const EDIT_HINT: &str = " tab move \u{b7} \u{2190}/\u{2192}/space \
     toggle \u{b7} enter/^s save \u{b7} esc cancel ";

pub(super) fn draw_overlay(frame: &mut Frame, app: &App, area: Rect) {
    let Some(form) = &app.account_form else {
        return;
    };
    match &form.identity_ui {
        Some(IdentityUi::List { selected }) => {
            draw_list(frame, app, area, form, *selected)
        }
        Some(IdentityUi::Edit(editor)) => {
            draw_editor(frame, app, area, editor)
        }
        None => {}
    }
}

fn draw_list(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    form: &AccountFormState,
    selected: usize,
) {
    let rows = form.identities.len() as u16;
    let modal = modal_rect(area, rows.max(1) + BORDER_ROWS + 1);
    let block = bordered(app, " identities ", LIST_HINT);
    let inner = block.inner(modal);
    frame.render_widget(Clear, modal);
    frame.render_widget(block, modal);
    let lines: Vec<Line<'static>> = form
        .identities
        .iter()
        .enumerate()
        .map(|(index, identity)| {
            list_row(app, identity, index == selected)
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn list_row(
    app: &App,
    identity: &FormIdentity,
    active: bool,
) -> Line<'static> {
    let theme = app.theme;
    let marker = if active { "\u{25b8} " } else { "  " };
    let mut style = Style::new().fg(theme.text_primary);
    if active {
        style =
            style.fg(theme.accent_strong).add_modifier(Modifier::BOLD);
    }
    Line::from(Span::styled(
        format!("{marker}{}", row_label(identity)),
        style,
    ))
}

fn row_label(identity: &FormIdentity) -> String {
    let name = descriptor(identity);
    if identity.address.trim().is_empty()
        || name == identity.address.trim()
    {
        return name;
    }
    format!("{name} <{}>", identity.address.trim())
}

fn draw_editor(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    editor: &IdentityEditor,
) {
    let title = if editor.target.is_some() {
        " edit identity "
    } else {
        " add identity "
    };
    let rows = EDITOR_FIELDS.len() as u16;
    let modal = modal_rect(area, rows + BORDER_ROWS);
    let block = bordered(app, title, EDIT_HINT);
    let inner = block.inner(modal);
    frame.render_widget(Clear, modal);
    frame.render_widget(block, modal);
    let lines: Vec<Line<'static>> = (0..EDITOR_FIELDS.len())
        .map(|index| editor_line(app, editor, index))
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn editor_line(
    app: &App,
    editor: &IdentityEditor,
    index: usize,
) -> Line<'static> {
    let theme = app.theme;
    let spec = &EDITOR_FIELDS[index];
    let active = index == editor.focus;
    let mut label_style = Style::new().fg(theme.accent);
    if active {
        label_style = label_style
            .fg(theme.accent_strong)
            .add_modifier(Modifier::BOLD);
    }
    let label = format!("{:<LABEL_COLS$}", spec.label);
    let mut spans = vec![Span::styled(label, label_style)];
    if spec.toggle {
        spans.extend(segmented::segments(
            &ON_OFF_OPTIONS,
            usize::from(editor.draft.pgp_sign),
            SegmentStyle {
                selected_bg: theme.accent,
                selected_fg: theme.background,
                unselected_fg: theme.text_muted,
            },
        ));
        return Line::from(spans);
    }
    let value = (spec.get)(&editor.draft).to_string();
    let shown = if active {
        with_cursor(&value, editor.cursor)
    } else {
        value
    };
    spans
        .push(Span::styled(shown, Style::new().fg(theme.text_primary)));
    Line::from(spans)
}

fn bordered(
    app: &App,
    title: &'static str,
    hint: &'static str,
) -> Block<'static> {
    let theme = app.theme;
    Block::bordered()
        .title(title)
        .title_bottom(hint)
        .border_style(Style::new().fg(theme.accent))
        .style(Style::new().bg(theme.surface))
}

fn modal_rect(area: Rect, rows: u16) -> Rect {
    let width = MODAL_WIDTH.min(area.width.saturating_sub(10));
    let height = rows.min(area.height.saturating_sub(2));
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::super::account_form_identity::IdentityEditor;
    use super::super::testkit::app_with_messages;
    use super::*;

    fn text(app: &App) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw_overlay(frame, app, frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
            .map(|(x, y)| {
                buffer.cell((x, y)).unwrap().symbol().to_string()
            })
            .collect()
    }

    fn seeded_form() -> AccountFormState {
        AccountFormState {
            identities: vec![
                FormIdentity::seed("Quin", "quin@example.com"),
                FormIdentity::seed("Side", "side@example.com"),
            ],
            ..AccountFormState::default()
        }
    }

    #[test]
    fn the_list_draws_every_identity() {
        let mut app = app_with_messages(1);
        let mut form = seeded_form();
        form.identity_ui = Some(IdentityUi::List { selected: 0 });
        app.account_form = Some(form);
        let shown = text(&app);
        assert!(shown.contains("identities"));
        assert!(shown.contains("Quin"));
        assert!(shown.contains("Side"));
    }

    #[test]
    fn the_editor_draws_every_field() {
        let mut app = app_with_messages(1);
        let mut form = seeded_form();
        form.identity_ui = Some(IdentityUi::Edit(IdentityEditor {
            target: Some(0),
            origin: 0,
            draft: FormIdentity::seed("Quin", "quin@example.com"),
            focus: 0,
            cursor: 0,
        }));
        app.account_form = Some(form);
        let shown = text(&app);
        assert!(shown.contains("edit identity"));
        assert!(shown.contains("from name"));
        assert!(shown.contains("from address"));
        assert!(shown.contains("auto-sign"));
        assert!(shown.contains("match patterns"));
    }
}
