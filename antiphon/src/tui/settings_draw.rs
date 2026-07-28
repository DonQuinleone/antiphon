use antiphon_ui::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::app::App;
use super::folders::FolderRow;
use super::headers::with_cursor;
use super::settings::{SettingsState, SettingsTab};
use super::settingscmd;

const SELECTED_MARK: &str = "\u{25b8} ";
const UNSELECTED_MARK: &str = "  ";
const NAME_WIDTH: usize = 16;
const ADDRESS_WIDTH: usize = 28;
const LABEL_WIDTH: usize = 26;

pub(super) fn draw_settings(frame: &mut Frame, app: &App, area: Rect) {
    let Some(state) = &app.settings else {
        return;
    };
    let [tabs_area, body_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)])
            .areas(area);
    frame.render_widget(
        Paragraph::new(tabs_line(app.theme, state.tab)),
        tabs_area,
    );
    match state.tab {
        SettingsTab::Accounts => {
            draw_accounts(frame, app.theme, state, body_area)
        }
        SettingsTab::Essentials => {
            draw_essentials(frame, app, state, body_area)
        }
        SettingsTab::Folders => {
            draw_folders(frame, app, state, body_area)
        }
    }
}

fn tabs_line(theme: &Theme, active: SettingsTab) -> Line<'static> {
    let tabs = [
        (SettingsTab::Accounts, "Accounts"),
        (SettingsTab::Essentials, "Essentials"),
        (SettingsTab::Folders, "Folders"),
    ];
    let spans = tabs
        .into_iter()
        .map(|(tab, label)| tab_span(theme, active, tab, label))
        .collect::<Vec<_>>();
    Line::from(spans)
}

fn tab_span(
    theme: &Theme,
    active: SettingsTab,
    tab: SettingsTab,
    label: &str,
) -> Span<'static> {
    let style = if tab == active {
        Style::new()
            .fg(theme.accent_strong)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(theme.text_muted)
    };
    Span::styled(format!(" {label} "), style)
}

fn draw_accounts(
    frame: &mut Frame,
    theme: &Theme,
    state: &SettingsState,
    area: Rect,
) {
    let mut lines = Vec::new();
    if state.accounts.is_empty() {
        lines.push(Line::from(Span::styled(
            "no accounts configured \u{b7} a adds one",
            Style::new().fg(theme.text_muted),
        )));
    }
    for (index, account) in state.accounts.iter().enumerate() {
        lines.push(account_line(
            theme,
            account,
            index,
            index == state.account_selected,
        ));
    }
    push_oauth_detail(&mut lines, theme, state);
    if let Some(name) = &state.pending_delete {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!("delete {name}? y/n"),
            Style::new().fg(theme.accent_strong),
        )));
    }
    if let Some(name) = &state.pending_revoke {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!("revoke the sign-in for {name}? y/n"),
            Style::new().fg(theme.accent_strong),
        )));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

/// The selected account's grant scopes and expiries, under
/// the list, so the row itself stays one line.
fn push_oauth_detail(
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
    state: &SettingsState,
) {
    let detail = state
        .accounts
        .get(state.account_selected)
        .and_then(|account| account.oauth.as_ref())
        .map(|info| info.detail.clone())
        .unwrap_or_default();
    if detail.is_empty() {
        return;
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        detail,
        Style::new().fg(theme.text_muted),
    )));
}

fn account_line(
    theme: &Theme,
    account: &super::settings::AccountSummary,
    index: usize,
    selected: bool,
) -> Line<'static> {
    let marker = mark(selected);
    let mut style = Style::new().fg(theme.text_primary);
    if selected {
        style = style.bg(theme.selection_bg).fg(theme.selection_fg);
    }
    let position = index + 1;
    let oauth = account
        .oauth
        .as_ref()
        .map(|info| format!("  {}", info.label()))
        .unwrap_or_default();
    Line::from(Span::styled(
        format!(
            "{marker}{position:>2} {:<NAME_WIDTH$}\
             {:<ADDRESS_WIDTH$}{}{oauth}",
            account.name, account.address, account.host
        ),
        style,
    ))
}

fn draw_essentials(
    frame: &mut Frame,
    app: &App,
    state: &SettingsState,
    area: Rect,
) {
    let theme = app.theme;
    let mut lines = Vec::new();
    for (index, row) in settingscmd::ESSENTIAL_ROWS.iter().enumerate() {
        let selected = index == state.essentials_selected;
        let marker = mark(selected);
        let mut style = Style::new().fg(theme.text_primary);
        if selected {
            style = style.bg(theme.selection_bg).fg(theme.selection_fg);
        }
        let value = (row.render)(app);
        lines.push(Line::from(Span::styled(
            format!("{marker}{:<LABEL_WIDTH$}{value}", row.label),
            style,
        )));
    }
    if let Some(hint) = &state.daemon_hint {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            hint.clone(),
            Style::new().fg(theme.text_muted),
        )));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_folders(
    frame: &mut Frame,
    app: &App,
    state: &SettingsState,
    area: Rect,
) {
    let theme = app.theme;
    let mut lines = Vec::new();
    if state.folders.is_empty() {
        lines.push(Line::from(Span::styled(
            "no folders discovered yet",
            Style::new().fg(theme.text_muted),
        )));
    }
    for (index, row) in state.folders.iter().enumerate() {
        lines.push(folder_line(
            app,
            row,
            index == state.folder_selected,
        ));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn folder_line(
    app: &App,
    row: &FolderRow,
    selected: bool,
) -> Line<'static> {
    let theme = app.theme;
    let marker = mark(selected);
    let mut style = Style::new().fg(theme.text_primary);
    if selected {
        style = style.bg(theme.selection_bg).fg(theme.selection_fg);
    }
    let alias = alias_text(row);
    Line::from(Span::styled(
        format!(
            "{marker}{:<NAME_WIDTH$}{:<ADDRESS_WIDTH$}{alias}",
            row.account, row.folder,
        ),
        style,
    ))
}

/// The alias column doubles as the hidden marker, so a hidden
/// folder says so on its own row.
fn alias_text(row: &FolderRow) -> String {
    match (row.hidden, row.alias.is_empty()) {
        (false, _) => row.alias.clone(),
        (true, true) => "(hidden)".to_string(),
        (true, false) => format!("{} (hidden)", row.alias),
    }
}

const ALIAS_MODAL_WIDTH: u16 = 56;
const ALIAS_HINT: &str =
    " enter saves \u{b7} empty removes \u{b7} esc cancels ";

/// The alias edits in a small modal of its own, named after
/// the folder, so the mode is unmistakable and the keys sit
/// right under the text being typed.
pub(super) fn draw_alias_modal(
    frame: &mut Frame,
    app: &App,
    area: Rect,
) {
    let Some(edit) = &app.folder_alias_edit else {
        return;
    };
    let theme = app.theme;
    let width = ALIAS_MODAL_WIDTH.min(area.width.saturating_sub(2));
    let height = 3u16.min(area.height);
    let modal = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(ratatui::widgets::Clear, modal);
    let title = format!(
        " alias for {}/{} ",
        edit.account,
        match edit.folder.is_empty() {
            true => "inbox",
            false => edit.folder.as_str(),
        }
    );
    let block = ratatui::widgets::Block::bordered()
        .title(title)
        .title_bottom(ALIAS_HINT)
        .border_style(Style::new().fg(theme.accent))
        .style(Style::new().bg(theme.surface));
    let inner = block.inner(modal);
    frame.render_widget(block, modal);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            with_cursor(&edit.text, edit.cursor),
            Style::new().fg(theme.text_primary),
        ))),
        inner,
    );
}

fn mark(selected: bool) -> &'static str {
    if selected {
        SELECTED_MARK
    } else {
        UNSELECTED_MARK
    }
}

#[cfg(test)]
#[path = "settings_draw_tests.rs"]
mod tests;
