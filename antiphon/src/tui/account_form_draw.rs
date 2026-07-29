use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::account_form::AccountFormState;
use antiphon_config::OauthProvider;

use super::account_form_fields::{
    CLIENT_ID_MS_HINT, FROM_ADDRESS_HINT, Field, PASSWORD_HINT,
};
use super::account_form_identity;
use super::app::App;
use super::draw::segmented::{self, SegmentStyle};
use super::headers::with_cursor;

/// Narrower than the settings modal (78) so the form nests
/// inside it rather than overhanging, keeping it visibly part of
/// the settings screen behind.
const MODAL_WIDTH: u16 = 72;
const LABEL_COLS: usize = 24;
const BORDER_ROWS: u16 = 2;
const HINT: &str = " tab move \u{b7} \u{2190}/\u{2192}/space toggle \
     \u{b7} enter/^s save \u{b7} esc cancel ";
const BULLET: char = '\u{2022}';

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
    let width = MODAL_WIDTH.min(area.width.saturating_sub(6));
    let inner_width = width.saturating_sub(2) as usize;
    let error_lines = form
        .error
        .as_deref()
        .map(|error| wrapped(error, inner_width))
        .unwrap_or_default();
    let extra_rows = 1 + error_lines.len() as u16;
    let height = (form.field_count() as u16 + BORDER_ROWS + extra_rows)
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

    let mut lines: Vec<Line<'static>> = (0..form.field_count())
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
    form: &AccountFormState,
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
    let mut spans = vec![Span::styled(label, label_style)];
    if let Some(options) = form.field_segments(index) {
        spans.extend(segment_spans(app, form, index, options));
        return Line::from(spans);
    }
    if form.field_id(index) == Field::Identities {
        spans.push(Span::styled(
            account_form_identity::summary(&form.identities),
            Style::new().fg(theme.text_primary),
        ));
        spans.push(Span::styled(
            " (enter to manage)",
            Style::new().fg(theme.text_muted),
        ));
        return Line::from(spans);
    }
    let displayed = shown_value(form, index);
    let value = if active {
        with_cursor(&displayed, form.cursor)
    } else {
        displayed
    };
    spans
        .push(Span::styled(value, Style::new().fg(theme.text_primary)));
    if let Some(hint) = field_hint(form, index) {
        spans.push(Span::styled(
            format!(" ({hint})"),
            Style::new().fg(theme.text_muted),
        ));
    }
    Line::from(spans)
}

/// The account type's selected segment wears its own
/// brand-evocative accent; every other toggle uses the house
/// accent.
fn segment_spans(
    app: &App,
    form: &AccountFormState,
    index: usize,
    options: &'static [&'static str],
) -> Vec<Span<'static>> {
    let theme = app.theme;
    let selected_bg = if form.field_id(index) == Field::AccountType {
        theme.account_type_accent(form.type_accent())
    } else {
        theme.accent
    };
    segmented::segments(
        options,
        form.field_selected(index),
        SegmentStyle {
            selected_bg,
            selected_fg: theme.background,
            unselected_fg: theme.text_muted,
        },
    )
}

fn field_hint(form: &AccountFormState, index: usize) -> Option<String> {
    match form.field_id(index) {
        Field::PasswordCmd
            if cfg!(target_os = "macos")
                && form.field_value(index).is_empty() =>
        {
            Some(PASSWORD_HINT.to_string())
        }
        Field::ClientId
            if form.provider() == Some(OauthProvider::Microsoft)
                && form.field_value(index).is_empty() =>
        {
            Some(CLIENT_ID_MS_HINT.to_string())
        }
        Field::FromAddress if form.field_value(index).is_empty() => {
            Some(FROM_ADDRESS_HINT.to_string())
        }
        _ => None,
    }
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
    use antiphon_config::GraphAuth;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::super::account_form_fields::AccountType;
    use super::super::testkit::app_with_messages;
    use super::*;

    fn rendered(app: &App) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw_form(frame, app, frame.area()))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn form_of(account_type: AccountType) -> AccountFormState {
        AccountFormState {
            account_type,
            graph_send: account_type == AccountType::Microsoft,
            graph_auth: GraphAuth::AppOnly,
            error: None,
            ..AccountFormState::default()
        }
    }

    fn has_bg(
        buffer: &ratatui::buffer::Buffer,
        bg: ratatui::style::Color,
    ) -> bool {
        (0..buffer.area.height).any(|y| {
            (0..buffer.area.width)
                .any(|x| buffer.cell((x, y)).unwrap().bg == bg)
        })
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

    #[test]
    fn the_type_toggle_draws_every_type_inline() {
        let mut app = app_with_messages(1);
        app.account_form = Some(form_of(AccountType::Imap));
        let shown = text(&rendered(&app));
        assert!(shown.contains("account type"));
        assert!(shown.contains("IMAP"));
        assert!(shown.contains("Microsoft 365"));
        assert!(shown.contains("Google"));
    }

    #[test]
    fn an_imap_form_shows_the_password_rows_not_oauth() {
        let mut app = app_with_messages(1);
        app.account_form = Some(form_of(AccountType::Imap));
        let shown = text(&rendered(&app));
        assert!(shown.contains("password command"));
        assert!(shown.contains("imap host"));
        assert!(shown.contains("smtp host"));
        assert!(shown.contains("from name"));
        assert!(shown.contains("from address"));
        assert!(!shown.contains("oauth client id"));
        assert!(!shown.contains("graph mode"));
    }

    #[test]
    fn a_google_form_shows_the_client_id_without_an_env_hint() {
        let mut app = app_with_messages(1);
        app.account_form = Some(form_of(AccountType::Google));
        let shown = text(&rendered(&app));
        assert!(shown.contains("oauth client id"));
        assert!(!shown.contains("ANTIPHON_GOOGLE_CLIENT_ID"));
        assert!(shown.contains("from name"));
        assert!(!shown.contains("password command"));
        assert!(!shown.contains("imap host"));
        assert!(!shown.contains("smtp host"));
        assert!(!shown.contains("graph mode"));
    }

    #[test]
    fn a_microsoft_form_shows_the_graph_toggles() {
        let mut app = app_with_messages(1);
        app.account_form = Some(form_of(AccountType::Microsoft));
        let shown = text(&rendered(&app));
        assert!(shown.contains("graph mode"));
        assert!(shown.contains("auth type"));
        assert!(shown.contains("delegated"));
        assert!(shown.contains("app-only"));
        assert!(shown.contains("graph secret command"));
        assert!(!shown.contains("password command"));
    }

    #[test]
    fn the_selected_type_wears_its_own_accent() {
        let mut app = app_with_messages(1);
        app.account_form = Some(form_of(AccountType::Microsoft));
        let buffer = rendered(&app);
        let accent = app
            .theme
            .account_type_accent(antiphon_ui::AccountAccent::Microsoft);
        assert!(
            has_bg(&buffer, accent),
            "the Microsoft segment carries its warm accent"
        );
    }
}
