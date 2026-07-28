use antiphon_ui::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem};

use super::app::App;
use super::scope::ViewScope;
use super::sidebar::SidebarEntry;

const ACTIVE_MARK: &str = "\u{25b8} ";
const INACTIVE_MARK: &str = "  ";
const SIDEBAR_BORDER_COLS: usize = 1;

pub(super) fn draw_sidebar(frame: &mut Frame, app: &App, area: Rect) {
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
