use antiphon_config::ReadingPane;
use antiphon_ui::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use tui_term::widget::PseudoTerminal;

use super::app::{App, View};
use super::message_list::{draw_list, format_date};
use super::pager::draw_pager;
use super::scope::ViewScope;
use super::sidebar::SidebarEntry;
use super::status::draw_status;

const SIDEBAR_WIDTH: u16 = 20;
const STATUS_HEIGHT: u16 = 1;
const READING_PANE_SHARE: u16 = 40;
const ACTIVE_MARK: &str = "\u{25b8} ";
const INACTIVE_MARK: &str = "  ";
const SIDEBAR_BORDER_COLS: usize = 1;
const SIDEBAR_RULE_COLS: usize =
    SIDEBAR_WIDTH as usize - SIDEBAR_BORDER_COLS;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(
        Block::new().style(Style::new().bg(app.theme.background)),
        area,
    );
    let (content, status) = split_status(area);
    if app.view == View::Editor {
        draw_editor(frame, app, content);
        draw_status(frame, app, status);
        return;
    }
    if app.view == View::Pager {
        draw_pager(frame, app, content);
        draw_status(frame, app, status);
        return;
    }
    let (sidebar, main) = split_sidebar(content, app.sidebar);
    if let Some(sidebar) = sidebar {
        draw_sidebar(frame, app, sidebar);
    }
    let (list, pane) = split_reading_pane(main, app.reading_pane);
    draw_list(frame, app, list);
    if let Some(pane) = pane {
        draw_reading_pane(frame, app, pane);
    }
    draw_status(frame, app, status);
}

fn split_status(area: Rect) -> (Rect, Rect) {
    let [content, status] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(STATUS_HEIGHT),
    ])
    .areas(area);
    (content, status)
}

fn split_sidebar(area: Rect, shown: bool) -> (Option<Rect>, Rect) {
    if !shown {
        return (None, area);
    }
    let [sidebar, main] = Layout::horizontal([
        Constraint::Length(SIDEBAR_WIDTH),
        Constraint::Min(0),
    ])
    .areas(area);
    (Some(sidebar), main)
}

fn split_reading_pane(
    area: Rect,
    pane: ReadingPane,
) -> (Rect, Option<Rect>) {
    match pane {
        ReadingPane::Off => (area, None),
        ReadingPane::Below => {
            let [list, pane] = Layout::vertical([
                Constraint::Min(0),
                Constraint::Percentage(READING_PANE_SHARE),
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
            items.push(sidebar_separator(theme));
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

fn sidebar_separator(theme: &Theme) -> ListItem<'static> {
    let rule = "\u{2500}".repeat(SIDEBAR_RULE_COLS);
    ListItem::new(Line::from(Span::styled(
        rule,
        Style::new().fg(theme.border),
    )))
}

fn sidebar_item(
    app: &App,
    index: usize,
    entry: &SidebarEntry,
) -> ListItem<'static> {
    let theme = app.theme;
    let active = entry_active(app, entry);
    let marker = if active { ACTIVE_MARK } else { INACTIVE_MARK };
    let mut style = if active {
        Style::new()
            .fg(theme.accent_strong)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(theme.text_muted)
    };
    if index == app.sidebar_selected {
        style = style.bg(theme.selection_bg).fg(theme.selection_fg);
    }
    let indent = if entry.is_folder() { "  " } else { "" };
    ListItem::new(Line::from(Span::styled(
        format!("{marker}{indent}{}", entry.label()),
        style,
    )))
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
    let lines = vec![
        header_line(theme, "From:", message.from.clone()),
        header_line(
            theme,
            "Date:",
            format_date(message.date_unix, &app.date_format),
        ),
        header_line(theme, "Subject:", message.subject.clone()),
        header_line(theme, "Tags:", message.tags.join(", ")),
        Line::default(),
        Line::from(Span::styled(
            "open the message for the full body",
            Style::new().fg(theme.text_muted),
        )),
    ];
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

pub(super) fn header_line(
    theme: &Theme,
    label: &'static str,
    value: String,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{label} "),
            Style::new().fg(theme.accent),
        ),
        Span::styled(value, Style::new().fg(theme.text_primary)),
    ])
}

/// Rows left for the editor pane once the statusline has
/// taken its line; the pty is kept exactly this size.
pub(super) fn editor_rows(height: u16) -> u16 {
    height.saturating_sub(STATUS_HEIGHT)
}

fn draw_editor(frame: &mut Frame, app: &App, area: Rect) {
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

    use super::super::app::app_with_folders;
    use super::*;

    fn row_text(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer.cell((x, y)).unwrap().symbol().to_string())
            .collect()
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
}
