use antiphon_config::ReadingPane;
use antiphon_ui::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, List, ListItem, Paragraph, Wrap,
};
use tui_term::widget::PseudoTerminal;

use super::app::{App, View};
use super::headers;
use super::help::draw_help;
use super::message_list::draw_list;
use super::pager::draw_pager;
use super::review;
use super::scope::ViewScope;
use super::settings_draw::draw_settings;
use super::sidebar::SidebarEntry;
use super::status::draw_status;

#[path = "segmented.rs"]
pub(in crate::tui) mod segmented;

const STATUS_HEIGHT: u16 = 1;
const FIELD_SUMMARY_ROWS: u16 = headers::FIELD_COUNT as u16;
const READING_PANE_SHARE: u16 = 40;
const LIST_HEADER_ROWS: u16 = 1;
pub(super) const SIDEBAR_WIDTH_MIN: u16 = 10;
pub(super) const SIDEBAR_WIDTH_MAX: u16 = 40;
const ACTIVE_MARK: &str = "\u{25b8} ";
const INACTIVE_MARK: &str = "  ";
const SIDEBAR_BORDER_COLS: usize = 1;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(
        Block::new().style(Style::new().bg(app.theme.background)),
        area,
    );
    let (content, status) = split_status(area);
    if app.view == View::Compose {
        if app.editor.is_some() {
            let [fields, pane] = Layout::vertical([
                Constraint::Length(FIELD_SUMMARY_ROWS),
                Constraint::Min(0),
            ])
            .areas(content);
            headers::draw_headers(frame, app, fields);
            draw_editor_screen(frame, app, pane);
        } else {
            headers::draw_headers(frame, app, content);
        }
        headers::draw_completion(frame, app, content);
        draw_status(frame, app, status);
        return;
    }
    if app.view == View::Review {
        review::draw_review(frame, app, content);
        draw_status(frame, app, status);
        return;
    }
    if app.view == View::Settings {
        draw_settings(frame, app, content);
        super::settings_draw::draw_alias_modal(frame, app, content);
        draw_status(frame, app, status);
        super::account_form_draw::draw_form(frame, app, area);
        super::account_form_identity_draw::draw_overlay(
            frame, app, area,
        );
        return;
    }
    if app.view == View::Editor {
        draw_editor(frame, app, content);
        draw_status(frame, app, status);
        return;
    }
    if app.view == View::Image {
        super::image_view::draw(frame, app, content);
        draw_status(frame, app, status);
        return;
    }
    if app.view == View::Pager {
        draw_pager(frame, app, content);
        draw_status(frame, app, status);
        super::link_picker::draw_picker(frame, app, area);
        super::folder_picker::draw_picker(frame, app, area);
        return;
    }
    let (sidebar, main) =
        split_sidebar(content, app.sidebar, app.sidebar_width);
    if let Some(sidebar) = sidebar {
        draw_sidebar(frame, app, sidebar);
    }
    let (list, pane) =
        split_reading_pane(main, app.reading_pane, app.list_rows);
    draw_list(frame, app, list);
    if let Some(pane) = pane {
        draw_reading_pane(frame, app, pane);
    }
    draw_status(frame, app, status);
    super::folder_picker::draw_picker(frame, app, area);
    if app.help {
        draw_help(frame, app, area);
    }
}

pub(super) fn split_status(area: Rect) -> (Rect, Rect) {
    let [content, status] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(STATUS_HEIGHT),
    ])
    .areas(area);
    (content, status)
}

fn split_sidebar(
    area: Rect,
    shown: bool,
    width: u16,
) -> (Option<Rect>, Rect) {
    if !shown {
        return (None, area);
    }
    let width = width.clamp(SIDEBAR_WIDTH_MIN, SIDEBAR_WIDTH_MAX);
    let [sidebar, main] = Layout::horizontal([
        Constraint::Length(width),
        Constraint::Min(0),
    ])
    .areas(area);
    (Some(sidebar), main)
}

/// With the pane below, the list holds exactly the configured
/// row count (plus its header) and the pane takes the rest;
/// right or off, the list keeps filling the height.
fn split_reading_pane(
    area: Rect,
    pane: ReadingPane,
    list_rows: u16,
) -> (Rect, Option<Rect>) {
    match pane {
        ReadingPane::Off => (area, None),
        ReadingPane::Below => {
            let [list, pane] = Layout::vertical([
                Constraint::Length(list_rows + LIST_HEADER_ROWS),
                Constraint::Min(0),
            ])
            .areas(area);
            (list, Some(pane))
        }
        ReadingPane::Right => {
            let [list, pane] = Layout::horizontal([
                Constraint::Min(0),
                Constraint::Percentage(READING_PANE_SHARE),
            ])
            .areas(area);
            (list, Some(pane))
        }
    }
}

fn draw_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let mut items = Vec::new();
    for (index, entry) in app.sidebar_entries.iter().enumerate() {
        if saved_section_starts(&app.sidebar_entries, index) {
            items.push(sidebar_separator(theme, area.width));
        }
        items.push(sidebar_item(app, index, entry));
    }
    let block = Block::new()
        .borders(Borders::RIGHT)
        .border_style(Style::new().fg(theme.border));
    frame.render_widget(List::new(items).block(block), area);
}

fn saved_section_starts(
    entries: &[SidebarEntry],
    index: usize,
) -> bool {
    if !entries[index].is_saved() {
        return false;
    }
    index == 0 || !entries[index - 1].is_saved()
}

fn sidebar_separator(theme: &Theme, width: u16) -> ListItem<'static> {
    let cols = (width as usize).saturating_sub(SIDEBAR_BORDER_COLS);
    let rule = "\u{2500}".repeat(cols);
    ListItem::new(Line::from(Span::styled(
        rule,
        Style::new().fg(theme.border),
    )))
}

/// A folder holding unread mail steps out of the muted rank
/// and wears its count, so the sidebar says where new mail
/// sits without being opened.
fn sidebar_item(
    app: &App,
    index: usize,
    entry: &SidebarEntry,
) -> ListItem<'static> {
    let theme = app.theme;
    let active = entry_active(app, entry);
    let unread = super::sidebar::unread_of(entry);
    let marker = if active { ACTIVE_MARK } else { INACTIVE_MARK };
    let mut style = if active {
        Style::new()
            .fg(theme.accent_strong)
            .add_modifier(Modifier::BOLD)
    } else if unread > 0 {
        Style::new()
            .fg(theme.text_primary)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(theme.text_muted)
    };
    let mut count_style = Style::new()
        .fg(theme.unread_marker)
        .add_modifier(Modifier::BOLD);
    if index == app.sidebar_selected {
        style = style.bg(theme.selection_bg).fg(theme.selection_fg);
        count_style = count_style.bg(theme.selection_bg);
    }
    let indent = if entry.is_folder() { "  " } else { "" };
    let label = match entry {
        SidebarEntry::Folder { account, name, .. } => {
            app.alias_for(account, name).unwrap_or(entry.label())
        }
        _ => entry.label(),
    };
    let mut spans =
        vec![Span::styled(format!("{marker}{indent}{label}"), style)];
    if unread > 0 {
        spans.push(Span::styled(format!(" {unread}"), count_style));
    }
    ListItem::new(Line::from(spans))
}

fn entry_active(app: &App, entry: &SidebarEntry) -> bool {
    match entry {
        SidebarEntry::Unified => app.scope == ViewScope::Unified,
        SidebarEntry::Account(account) => matches!(
            &app.scope,
            ViewScope::Account(current) if current == account
        ),
        SidebarEntry::Folder { account, query, .. } => {
            app.current_query == *query
                && matches!(
                    &app.scope,
                    ViewScope::Account(current) if current == account
                )
        }
        SidebarEntry::Saved { name, query } => {
            app.active_search.as_deref() == Some(name)
                && app.current_query == *query
        }
    }
}

fn draw_reading_pane(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let block = Block::new()
        .borders(Borders::TOP)
        .border_style(Style::new().fg(theme.border));
    let Some(message) = app.selected_message() else {
        let empty = Paragraph::new(Line::from(Span::styled(
            "no message selected",
            Style::new().fg(theme.text_muted),
        )))
        .block(block);
        frame.render_widget(empty, area);
        return;
    };
    let mut lines = Vec::new();
    if let Some(preview) = &app.preview {
        let headers = if app.headers_all {
            &preview.headers_all
        } else {
            &preview.headers
        };
        lines.extend(super::pager::header_block(
            theme,
            headers,
            &message.tags,
            area.width,
        ));
        lines.push(Line::default());
    }
    lines.extend(preview_lines(app));
    let pane = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.preview_scroll, 0));
    frame.render_widget(pane, area);
}

fn preview_lines(app: &App) -> Vec<Line<'static>> {
    let theme = app.theme;
    let Some(preview) = &app.preview else {
        return Vec::new();
    };
    preview
        .lines
        .iter()
        .map(|line| {
            let colour = super::pager_body::prose_colour(theme, line);
            Line::from(Span::styled(
                line.clone(),
                Style::new().fg(colour),
            ))
        })
        .collect()
}

/// Rows left for the editor pane once the statusline and the
/// header field summary have taken theirs; the pty is kept
/// exactly this size.
pub(super) fn editor_rows(height: u16) -> u16 {
    height.saturating_sub(STATUS_HEIGHT + FIELD_SUMMARY_ROWS)
}

/// The body editor with the header fields summarised above
/// it, aerc style: the fields stay ours, the pty gets only
/// the body.
fn draw_editor(frame: &mut Frame, app: &App, area: Rect) {
    let [summary, editor] = Layout::vertical([
        Constraint::Length(FIELD_SUMMARY_ROWS),
        Constraint::Min(0),
    ])
    .areas(area);
    if let Some(state) = &app.compose {
        let lines = headers::field_lines(app.theme, state, false);
        frame.render_widget(Paragraph::new(lines), summary);
    }
    draw_editor_screen(frame, app, editor);
}

fn draw_editor_screen(frame: &mut Frame, app: &App, area: Rect) {
    let Some(pane) = &app.editor else {
        return;
    };
    frame.render_widget(
        PseudoTerminal::new(pane.session.screen()),
        area,
    );
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    use super::super::testkit::{app_with_folders, app_with_messages};
    use super::*;

    fn row_text(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer.cell((x, y)).unwrap().symbol().to_string())
            .collect()
    }

    fn rendered(app: &App, width: u16, height: u16) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    /// List rows only: the reading pane's own header line
    /// ("Subject: ...") starts the row and is not counted.
    fn subject_rows(buffer: &Buffer) -> usize {
        (0..buffer.area.height)
            .map(|y| row_text(buffer, y))
            .filter(|row| {
                row.contains("subject-")
                    && !row.trim_start().starts_with("Subject:")
            })
            .count()
    }

    fn crowded_app(count: usize) -> App {
        let mut app = app_with_messages(count);
        for (index, message) in app.messages.iter_mut().enumerate() {
            message.subject = format!("subject-{index}");
        }
        app.sidebar = false;
        app
    }

    #[test]
    fn sidebar_folders_render_indented_under_their_account() {
        let app = app_with_folders(&[(
            "work",
            &["archive", "lists/aerc"][..],
        )]);
        let backend = TestBackend::new(60, 14);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let buffer = terminal.backend().buffer();
        let rows: Vec<String> =
            (0..5).map(|y| row_text(buffer, y)).collect();
        assert!(rows[0].contains("unified"), "{:?}", rows[0]);
        assert!(rows[1].contains("work"), "{:?}", rows[1]);
        assert!(rows[2].starts_with("    inbox"), "{:?}", rows[2]);
        assert!(rows[3].starts_with("    archive"), "{:?}", rows[3]);
        assert!(rows[4].starts_with("    lists/aerc"), "{:?}", rows[4]);
    }

    #[test]
    fn unread_folders_wear_their_count_in_the_sidebar() {
        let mut app = app_with_folders(&[("work", &["archive"][..])]);
        super::super::sidebar::fill_unread(
            &mut app.sidebar_entries,
            |query| query.contains("archive").then_some(7),
        );
        let backend = TestBackend::new(60, 14);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let buffer = terminal.backend().buffer();
        let rows: Vec<String> =
            (0..5).map(|y| row_text(buffer, y)).collect();
        assert!(rows[2].starts_with("    inbox"), "{:?}", rows[2]);
        assert!(!rows[2].contains('0'), "no count when read");
        assert!(rows[3].starts_with("    archive 7"), "{:?}", rows[3]);
    }

    #[test]
    fn pane_below_shows_exactly_list_rows_messages() {
        let mut app = crowded_app(40);
        app.list_rows = 7;
        let buffer = rendered(&app, 80, 30);
        assert_eq!(subject_rows(&buffer), 7);
        let border = row_text(&buffer, 1 + 7);
        assert!(
            border.contains("\u{2500}"),
            "pane border expected below the list: {border:?}"
        );
    }

    #[test]
    fn without_a_pane_below_the_list_fills_the_height() {
        let mut app = crowded_app(40);
        app.list_rows = 7;
        app.reading_pane = ReadingPane::Off;
        let full_height = rendered(&app, 80, 30);
        let expected =
            30 - STATUS_HEIGHT as usize - LIST_HEADER_ROWS as usize;
        assert_eq!(subject_rows(&full_height), expected);

        app.reading_pane = ReadingPane::Right;
        let split_right = rendered(&app, 80, 30);
        assert_eq!(subject_rows(&split_right), expected);
    }

    #[test]
    fn tabs_mode_reserves_no_bar_row_so_the_list_reclaims_it() {
        let mut app = crowded_app(40);
        app.list_rows = 7;
        app.reading_pane = ReadingPane::Off;
        let sidebar_mode = subject_rows(&rendered(&app, 80, 30));

        app.accounts_bar = antiphon_config::AccountsBar::Tabs;
        let expected =
            30 - STATUS_HEIGHT as usize - LIST_HEADER_ROWS as usize;
        assert_eq!(
            subject_rows(&rendered(&app, 80, 30)),
            expected,
            "no tab-bar row is reserved above the list"
        );
        assert_eq!(
            subject_rows(&rendered(&app, 80, 30)),
            sidebar_mode,
            "tabs mode fits the same rows as sidebar mode"
        );
    }

    #[test]
    fn the_sidebar_is_as_wide_as_configured_and_clamped() {
        let cases = [
            (12u16, 12u16),
            (2, SIDEBAR_WIDTH_MIN),
            (200, SIDEBAR_WIDTH_MAX),
        ];
        for (configured, effective) in cases {
            let mut app = crowded_app(3);
            app.sidebar = true;
            app.sidebar_width = configured;
            let buffer = rendered(&app, 80, 12);
            let border_x = effective - 1;
            let row = row_text(&buffer, 0);
            let border_col: Vec<char> = row.chars().collect();
            assert_eq!(
                border_col[border_x as usize], '\u{2502}',
                "width {configured}: {row:?}"
            );
        }
    }
}
